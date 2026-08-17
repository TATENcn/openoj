//! P0-C Judge Node UDS client and explicit development-only worker assembly.

use std::fmt::{self, Display, Formatter};
use std::path::Path;

use openoj_application::{JudgeRenewDirective, StoreError, TaskLease};
use openoj_domain::{
    Capability, ClaimOperationId, EvaluationResult, LeaseDuration, LeaseToken, NodeId,
    ResultOperationId, UnixMillis,
};
use openoj_judge_core::{AsyncJudgeControlClient, WorkerClaim};
use openoj_judge_protocol::CLIENT_PROTOCOL_VERSION;
use openoj_judge_protocol::wire::{
    ClaimRequest, RenewLeaseRequest, SubmitResultRequest, claim_response,
    judge_control_client::JudgeControlClient, renew_lease_response,
};
use openoj_protocol::{decode_evaluation_request, encode_evaluation_result};

/// A connected P0-C Judge Control client bound to one configured node identity.
pub struct UdsJudgeControlClient {
    client: JudgeControlClient<tonic::transport::Channel>,
    node_id: NodeId,
    capabilities: Vec<Capability>,
    lease_duration: LeaseDuration,
    no_task_backoff: std::time::Duration,
}

impl UdsJudgeControlClient {
    /// Connects over an absolute UDS path and fails closed during protocol negotiation.
    ///
    /// # Errors
    ///
    /// Returns [`JudgeNodeError`] for invalid local configuration, unavailable transport, or an
    /// incompatible server response.
    pub async fn connect(
        socket_path: impl AsRef<Path>,
        node_id: NodeId,
        capabilities: Vec<Capability>,
    ) -> Result<Self, JudgeNodeError> {
        let socket_path = socket_path.as_ref();
        if !socket_path.is_absolute() || capabilities.is_empty() {
            return Err(JudgeNodeError::InvalidConfiguration);
        }
        let endpoint =
            tonic::transport::Endpoint::from_shared(format!("unix://{}", socket_path.display()))
                .map_err(|_| JudgeNodeError::Unavailable)?;
        let mut client = JudgeControlClient::new(
            endpoint
                .connect()
                .await
                .map_err(|_| JudgeNodeError::Unavailable)?,
        );
        let response = client
            .negotiate(openoj_judge_protocol::wire::NegotiateRequest {
                version: CLIENT_PROTOCOL_VERSION.to_owned(),
                node_id: node_id.as_str().to_owned(),
                capabilities: capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_owned())
                    .collect(),
                client_limits: None,
            })
            .await
            .map_err(|_| JudgeNodeError::IncompatibleServer)?
            .into_inner();
        if response.accepted_version != CLIENT_PROTOCOL_VERSION
            || response.allowed_capabilities.len() != capabilities.len()
            || response.lease_duration_ms == 0
            || response.renew_after_ms == 0
            || response.renew_after_ms > response.lease_duration_ms / 2
            || response.no_task_backoff_ms == 0
        {
            return Err(JudgeNodeError::IncompatibleServer);
        }
        for capability in &capabilities {
            if !response
                .allowed_capabilities
                .iter()
                .any(|allowed| allowed == capability.as_str())
            {
                return Err(JudgeNodeError::IncompatibleServer);
            }
        }
        let lease_duration = LeaseDuration::new(u64::from(response.lease_duration_ms))
            .map_err(|_| JudgeNodeError::IncompatibleServer)?;
        Ok(Self {
            client,
            node_id,
            capabilities,
            lease_duration,
            no_task_backoff: std::time::Duration::from_millis(u64::from(
                response.no_task_backoff_ms,
            )),
        })
    }

    /// Returns the server-negotiated lease duration.
    #[must_use]
    pub const fn lease_duration(&self) -> LeaseDuration {
        self.lease_duration
    }

    /// Returns the server-negotiated empty-queue polling interval.
    #[must_use]
    pub const fn no_task_backoff(&self) -> std::time::Duration {
        self.no_task_backoff
    }
}

impl AsyncJudgeControlClient for UdsJudgeControlClient {
    async fn claim(&mut self, operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        let response = self
            .client
            .claim(ClaimRequest {
                version: CLIENT_PROTOCOL_VERSION.to_owned(),
                node_id: self.node_id.as_str().to_owned(),
                claim_operation_id: operation_id.as_str().to_owned(),
                capabilities: self
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_owned())
                    .collect(),
            })
            .await
            .map_err(|status| map_status(&status))?
            .into_inner();
        match response.outcome {
            Some(claim_response::Outcome::NoTask(_)) => Ok(WorkerClaim::NoTask),
            Some(claim_response::Outcome::Leased(leased)) => {
                let request = decode_evaluation_request(&leased.canonical_request)
                    .map_err(|_| StoreError::CorruptData)?;
                if request.evaluation_id().as_str() != leased.evaluation_id
                    || request.attempt_id().as_str() != leased.attempt_id
                    || request.attempt_number() != leased.attempt_number
                {
                    return Err(StoreError::IdentityConflict);
                }
                Ok(WorkerClaim::Lease(Box::new(TaskLease {
                    request,
                    node_id: self.node_id.clone(),
                    lease_token: LeaseToken::parse(leased.lease_token)
                        .map_err(|_| StoreError::CorruptData)?,
                    expires_at: UnixMillis::new(leased.lease_expires_at_unix_ms)
                        .map_err(|_| StoreError::CorruptData)?,
                })))
            }
            None => Err(StoreError::CorruptData),
        }
    }

    async fn renew(&mut self, lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        let response = self
            .client
            .renew_lease(RenewLeaseRequest {
                version: CLIENT_PROTOCOL_VERSION.to_owned(),
                node_id: self.node_id.as_str().to_owned(),
                evaluation_id: lease.request.evaluation_id().as_str().to_owned(),
                attempt_id: lease.request.attempt_id().as_str().to_owned(),
                lease_token: lease.lease_token.as_str().to_owned(),
            })
            .await
            .map_err(|status| map_status(&status))?
            .into_inner();
        match response.directive {
            Some(renew_lease_response::Directive::ContinueLease(continue_lease)) => {
                Ok(JudgeRenewDirective::Continue {
                    expires_at: UnixMillis::new(continue_lease.lease_expires_at_unix_ms)
                        .map_err(|_| StoreError::CorruptData)?,
                })
            }
            Some(renew_lease_response::Directive::CancelLease(_)) => {
                Ok(JudgeRenewDirective::Cancel)
            }
            None => Err(StoreError::CorruptData),
        }
    }

    async fn submit(
        &mut self,
        lease: &TaskLease,
        operation_id: ResultOperationId,
        result: EvaluationResult,
    ) -> Result<(), StoreError> {
        let response = self
            .client
            .submit_result(SubmitResultRequest {
                version: CLIENT_PROTOCOL_VERSION.to_owned(),
                node_id: self.node_id.as_str().to_owned(),
                evaluation_id: lease.request.evaluation_id().as_str().to_owned(),
                attempt_id: lease.request.attempt_id().as_str().to_owned(),
                lease_token: lease.lease_token.as_str().to_owned(),
                result_operation_id: operation_id.as_str().to_owned(),
                canonical_result: encode_evaluation_result(&result)
                    .map_err(|_| StoreError::CorruptData)?,
            })
            .await
            .map_err(|status| map_status(&status))?
            .into_inner();
        let snapshot = response.evaluation.ok_or(StoreError::CorruptData)?;
        if snapshot.evaluation_id != lease.request.evaluation_id().as_str() {
            return Err(StoreError::IdentityConflict);
        }
        Ok(())
    }
}

/// A bounded error returned while configuring a P0-C Judge Node process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JudgeNodeError {
    InvalidConfiguration,
    IncompatibleServer,
    Unavailable,
}

impl Display for JudgeNodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "judge node configuration is invalid",
            Self::IncompatibleServer => "Judge Control server negotiation was rejected",
            Self::Unavailable => "Judge Control UDS is unavailable",
        })
    }
}

impl std::error::Error for JudgeNodeError {}

fn map_status(status: &tonic::Status) -> StoreError {
    match status.code() {
        tonic::Code::NotFound => StoreError::NotFound,
        tonic::Code::AlreadyExists => StoreError::IdempotencyConflict,
        tonic::Code::FailedPrecondition | tonic::Code::PermissionDenied => StoreError::StaleLease,
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => StoreError::Unavailable,
        _ => StoreError::CorruptData,
    }
}
