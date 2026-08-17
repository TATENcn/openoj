//! P0-C control-plane process configuration and UDS boundary checks.

use std::fmt::{self, Display, Formatter};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use openoj_application::{
    JudgeClaim, JudgeRenew, JudgeRenewDirective, JudgeSubmitResult, LeasePolicy, NodePolicy,
    NodePolicyError, StoreError,
};
use openoj_domain::{
    AttemptId, ClaimOperationId, EvaluationId, LeaseToken, NodeId, ResultOperationId, UnixMillis,
};
use openoj_judge_protocol::wire::{
    CancelLease, ClaimRequest, ClaimResponse, ContinueLease, EvaluationSnapshot, LeasedTask,
    NegotiateRequest, NegotiateResponse, NoTask, RenewLeaseRequest, RenewLeaseResponse,
    SubmitResultRequest, SubmitResultResponse, claim_response,
    judge_control_server::{JudgeControl, JudgeControlServer},
    renew_lease_response,
};
use openoj_judge_protocol::{
    CLIENT_PROTOCOL_VERSION, validate_capabilities, validate_protocol_version,
};
use openoj_protocol::{decode_evaluation_result_domain, encode_evaluation_request};
use openoj_storage::PostgresEvaluationStore;

/// Largest accepted P0-C gRPC message including canonical result payload framing.
pub const MAX_JUDGE_CONTROL_MESSAGE_BYTES: usize = 1_100_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketPathError {
    RelativePath,
    InvalidParent,
    InsecureParent,
    ExistingPath,
    CreateFailed,
}

impl Display for SocketPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RelativePath => "Judge Control socket path must be absolute",
            Self::InvalidParent => "Judge Control socket parent must be an existing directory",
            Self::InsecureParent => {
                "Judge Control socket parent must not be accessible by group or other users"
            }
            Self::ExistingPath => "Judge Control socket path must not already exist",
            Self::CreateFailed => "Judge Control socket could not be created",
        })
    }
}

impl std::error::Error for SocketPathError {}

/// P0-C transport adapter for server-owned deployment and lease policy.
pub struct JudgeControlService {
    store: Option<PostgresEvaluationStore>,
    node_policy: NodePolicy,
    lease_policy: LeasePolicy,
    source: Arc<dyn ControlSource>,
}

impl JudgeControlService {
    /// Creates an adapter that only serves policy negotiation.
    ///
    /// Production assembly must use [`Self::with_store`] before exposing the service.
    #[must_use]
    pub fn new(node_policy: NodePolicy, lease_policy: LeasePolicy) -> Self {
        Self {
            store: None,
            node_policy,
            lease_policy,
            source: Arc::new(SystemControlSource),
        }
    }

    /// Attaches the control-plane-owned persistence port and clock/token source.
    #[must_use]
    pub fn with_store(mut self, store: PostgresEvaluationStore) -> Self {
        self.store = Some(store);
        self
    }

    fn store(&self) -> Result<&PostgresEvaluationStore, tonic::Status> {
        self.store
            .as_ref()
            .ok_or_else(|| tonic::Status::unavailable("control_plane_unavailable"))
    }

    fn negotiate_request(&self, request: NegotiateRequest) -> Result<NodeSession, tonic::Status> {
        validate_protocol_version(&request.version)
            .map_err(|_| tonic::Status::failed_precondition("unsupported_version"))?;
        let node_id = NodeId::parse(request.node_id)
            .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?;
        let capabilities = validate_capabilities(&request.capabilities)
            .map_err(|_| tonic::Status::invalid_argument("capability_denied"))?;
        self.node_policy
            .authorize(&node_id, &capabilities)
            .map_err(map_node_policy_error)?;
        Ok(NodeSession {
            node_id,
            capabilities,
        })
    }

    fn lease_duration_ms(&self) -> Result<u32, tonic::Status> {
        u32::try_from(self.lease_policy.lease_duration().value())
            .map_err(|_| tonic::Status::internal("invalid_configuration"))
    }

    fn renew_after_ms(&self) -> Result<u32, tonic::Status> {
        u32::try_from(self.lease_policy.renew_after().value())
            .map_err(|_| tonic::Status::internal("invalid_configuration"))
    }
}

/// Server-owned source of time and opaque lease tokens.
pub trait ControlSource: Send + Sync {
    /// Obtains the current bounded server time.
    ///
    /// # Errors
    ///
    /// Returns an unavailable status when the system clock cannot provide a supported timestamp.
    fn now(&self) -> Result<UnixMillis, tonic::Status>;

    /// Generates a fresh opaque lease token.
    ///
    /// # Errors
    ///
    /// Returns an unavailable status when the operating system random source is unavailable.
    fn lease_token(&self) -> Result<LeaseToken, tonic::Status>;
}

struct SystemControlSource;

impl ControlSource for SystemControlSource {
    fn now(&self) -> Result<UnixMillis, tonic::Status> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| tonic::Status::unavailable("clock_unavailable"))?;
        let milliseconds = u64::try_from(elapsed.as_millis())
            .map_err(|_| tonic::Status::unavailable("clock_unavailable"))?;
        UnixMillis::new(milliseconds).map_err(|_| tonic::Status::unavailable("clock_unavailable"))
    }

    fn lease_token(&self) -> Result<LeaseToken, tonic::Status> {
        let mut bytes = [0_u8; 24];
        getrandom::fill(&mut bytes)
            .map_err(|_| tonic::Status::unavailable("random_unavailable"))?;
        LeaseToken::parse(format!("lease_{}", hex::encode(bytes)))
            .map_err(|_| tonic::Status::internal("lease_token_invalid"))
    }
}

struct NodeSession {
    node_id: NodeId,
    capabilities: Vec<openoj_domain::Capability>,
}

#[tonic::async_trait]
impl JudgeControl for JudgeControlService {
    async fn negotiate(
        &self,
        request: tonic::Request<NegotiateRequest>,
    ) -> Result<tonic::Response<NegotiateResponse>, tonic::Status> {
        let session = self.negotiate_request(request.into_inner())?;
        let allowed_capabilities = session
            .capabilities
            .iter()
            .map(|capability| capability.as_str().to_owned())
            .collect();
        Ok(tonic::Response::new(NegotiateResponse {
            accepted_version: CLIENT_PROTOCOL_VERSION.to_owned(),
            allowed_capabilities,
            lease_duration_ms: self.lease_duration_ms()?,
            renew_after_ms: self.renew_after_ms()?,
            no_task_backoff_ms: 500,
            method_limits: None,
        }))
    }

    async fn claim(
        &self,
        request: tonic::Request<ClaimRequest>,
    ) -> Result<tonic::Response<ClaimResponse>, tonic::Status> {
        let request = request.into_inner();
        let session = self.negotiate_request(NegotiateRequest {
            version: request.version,
            node_id: request.node_id,
            capabilities: request.capabilities,
            client_limits: None,
        })?;
        let operation_id = ClaimOperationId::parse(request.claim_operation_id)
            .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?;
        let lease = match self
            .store()?
            .judge_claim_task(JudgeClaim {
                node_id: session.node_id,
                declared_capabilities: session.capabilities,
                operation_id,
                lease_token: self.source.lease_token()?,
                now: self.source.now()?,
                lease_policy: self.lease_policy,
            })
            .await
        {
            Ok(lease) => lease,
            Err(StoreError::NoTaskAvailable) => {
                return Ok(tonic::Response::new(ClaimResponse {
                    outcome: Some(claim_response::Outcome::NoTask(NoTask {
                        retry_after_ms: 500,
                    })),
                }));
            }
            Err(error) => return Err(map_store_error(error)),
        };
        let canonical_request = encode_evaluation_request(&lease.request)
            .map_err(|_| tonic::Status::internal("stored_request_invalid"))?;
        Ok(tonic::Response::new(ClaimResponse {
            outcome: Some(claim_response::Outcome::Leased(LeasedTask {
                evaluation_id: lease.request.evaluation_id().as_str().to_owned(),
                attempt_id: lease.request.attempt_id().as_str().to_owned(),
                attempt_number: lease.request.attempt_number(),
                lease_token: lease.lease_token.as_str().to_owned(),
                lease_expires_at_unix_ms: lease.expires_at.value(),
                canonical_request,
            })),
        }))
    }

    async fn renew_lease(
        &self,
        request: tonic::Request<RenewLeaseRequest>,
    ) -> Result<tonic::Response<RenewLeaseResponse>, tonic::Status> {
        let request = request.into_inner();
        validate_protocol_version(&request.version)
            .map_err(|_| tonic::Status::failed_precondition("unsupported_version"))?;
        let node_id = parse_and_authorize_node(&self.node_policy, request.node_id)?;
        let directive = self
            .store()?
            .judge_renew_lease(JudgeRenew {
                node_id,
                evaluation_id: EvaluationId::parse(request.evaluation_id)
                    .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?,
                attempt_id: AttemptId::parse(request.attempt_id)
                    .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?,
                lease_token: LeaseToken::parse(request.lease_token)
                    .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?,
                now: self.source.now()?,
                lease_policy: self.lease_policy,
            })
            .await
            .map_err(map_store_error)?;
        let directive = match directive {
            JudgeRenewDirective::Continue { expires_at } => {
                renew_lease_response::Directive::ContinueLease(ContinueLease {
                    lease_expires_at_unix_ms: expires_at.value(),
                    renew_after_ms: self.renew_after_ms()?,
                })
            }
            JudgeRenewDirective::Cancel => {
                renew_lease_response::Directive::CancelLease(CancelLease {})
            }
        };
        Ok(tonic::Response::new(RenewLeaseResponse {
            directive: Some(directive),
        }))
    }

    async fn submit_result(
        &self,
        request: tonic::Request<SubmitResultRequest>,
    ) -> Result<tonic::Response<SubmitResultResponse>, tonic::Status> {
        let request = request.into_inner();
        validate_protocol_version(&request.version)
            .map_err(|_| tonic::Status::failed_precondition("unsupported_version"))?;
        let node_id = parse_and_authorize_node(&self.node_policy, request.node_id)?;
        let evaluation_id = EvaluationId::parse(request.evaluation_id)
            .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?;
        let attempt_id = AttemptId::parse(request.attempt_id)
            .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?;
        let result = decode_evaluation_result_domain(&request.canonical_result)
            .map_err(|_| tonic::Status::invalid_argument("invalid_canonical_result"))?;
        if result.identity().evaluation_id() != &evaluation_id
            || result.identity().attempt_id() != &attempt_id
        {
            return Err(tonic::Status::invalid_argument("result_identity_mismatch"));
        }
        let snapshot = self
            .store()?
            .judge_submit_result(JudgeSubmitResult {
                node_id,
                operation_id: ResultOperationId::parse(request.result_operation_id)
                    .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?,
                lease_token: LeaseToken::parse(request.lease_token)
                    .map_err(|_| tonic::Status::invalid_argument("invalid_message"))?,
                result,
                now: self.source.now()?,
            })
            .await
            .map_err(map_store_error)?;
        Ok(tonic::Response::new(SubmitResultResponse {
            evaluation: Some(EvaluationSnapshot {
                evaluation_id: snapshot.evaluation_id.as_str().to_owned(),
                state: snapshot.state.as_str().to_owned(),
                current_attempt_number: snapshot.attempt_number,
            }),
        }))
    }
}

fn map_node_policy_error(error: NodePolicyError) -> tonic::Status {
    match error {
        NodePolicyError::IdentityDenied => tonic::Status::permission_denied("identity_denied"),
        NodePolicyError::CapabilityDenied => tonic::Status::permission_denied("capability_denied"),
        NodePolicyError::InvalidRenewSchedule
        | NodePolicyError::DuplicateNode
        | NodePolicyError::InvalidCapabilities => tonic::Status::internal("invalid_configuration"),
    }
}

fn parse_and_authorize_node(
    node_policy: &NodePolicy,
    value: String,
) -> Result<NodeId, tonic::Status> {
    let node_id =
        NodeId::parse(value).map_err(|_| tonic::Status::invalid_argument("invalid_message"))?;
    node_policy
        .authorize_identity(&node_id)
        .map_err(map_node_policy_error)?;
    Ok(node_id)
}

fn map_store_error(error: StoreError) -> tonic::Status {
    match error {
        StoreError::NotFound => tonic::Status::not_found("evaluation_not_found"),
        StoreError::NoTaskAvailable => tonic::Status::unavailable("no_task_available"),
        StoreError::IdempotencyConflict | StoreError::TerminalConflict => {
            tonic::Status::already_exists("operation_conflict")
        }
        StoreError::IdentityConflict | StoreError::LeaseConflict | StoreError::StaleLease => {
            tonic::Status::failed_precondition("lease_rejected")
        }
        StoreError::InvalidTime
        | StoreError::InvalidTransition
        | StoreError::IncompatibleSchema
        | StoreError::CorruptData => tonic::Status::failed_precondition("state_rejected"),
        StoreError::Unavailable => tonic::Status::unavailable("storage_unavailable"),
        _ => tonic::Status::internal("storage_error"),
    }
}

/// Checks the non-destructive UDS parent preconditions before a listener is created.
///
/// # Errors
///
/// Returns [`SocketPathError`] when the path is relative or its parent is missing, a symlink, or
/// not a directory.
pub fn validate_socket_path(path: impl AsRef<Path>) -> Result<(), SocketPathError> {
    let path = path.as_ref();
    if !path.is_absolute() {
        return Err(SocketPathError::RelativePath);
    }
    let parent = path.parent().ok_or(SocketPathError::InvalidParent)?;
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| SocketPathError::InvalidParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SocketPathError::InvalidParent);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(SocketPathError::InsecureParent);
    }
    Ok(())
}

/// Creates the P0-C Unix Domain Socket listener with private filesystem permissions.
///
/// # Errors
///
/// Returns [`SocketPathError`] when preconditions fail, an existing object would be replaced, or
/// the socket cannot be created with mode `0600`.
pub async fn bind_socket(
    path: impl AsRef<Path>,
) -> Result<tokio::net::UnixListener, SocketPathError> {
    let path = path.as_ref();
    validate_socket_path(path)?;
    if path.exists() {
        return Err(SocketPathError::ExistingPath);
    }
    let listener =
        tokio::net::UnixListener::bind(path).map_err(|_| SocketPathError::CreateFailed)?;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|_| SocketPathError::CreateFailed)?;
    Ok(listener)
}

/// Serves the P0-C Judge Control gRPC service over an already-private UDS listener.
///
/// This function never creates a TCP listener.
///
/// # Errors
///
/// Returns a transport error when the UDS listener fails or graceful shutdown cannot complete.
pub async fn serve_with_shutdown<F>(
    listener: tokio::net::UnixListener,
    service: JudgeControlService,
    shutdown: F,
) -> Result<(), tonic::transport::Error>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let service = JudgeControlServer::new(service)
        .max_decoding_message_size(MAX_JUDGE_CONTROL_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_JUDGE_CONTROL_MESSAGE_BYTES);
    tonic::transport::Server::builder()
        .add_service(service)
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::UnixListenerStream::new(listener),
            shutdown,
        )
        .await
}
