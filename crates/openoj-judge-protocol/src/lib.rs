//! Judge Control protocol bindings and transport-boundary validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use openoj_domain::Capability;
use openoj_protocol::{MAX_EVALUATION_REQUEST_BYTES, decode_evaluation_request};

#[allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::too_many_lines
)]
pub mod wire {
    tonic::include_proto!("openoj.judge.control.v0alpha1");
}

pub const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("judge-control-v0alpha1");

pub const CLIENT_PROTOCOL_VERSION: &str = "openoj.judge.control/v0alpha1";
pub const MAX_CAPABILITIES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolContractError {
    UnsupportedVersion,
    InvalidCapabilities,
    MessageTooLarge,
    InvalidCanonicalPayload,
}

impl Display for ProtocolContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion => formatter.write_str("unsupported Judge Control version"),
            Self::InvalidCapabilities => formatter.write_str("invalid Judge Control capabilities"),
            Self::MessageTooLarge => formatter.write_str("Judge Control message exceeds its limit"),
            Self::InvalidCanonicalPayload => {
                formatter.write_str("invalid canonical evaluation payload")
            }
        }
    }
}

impl Error for ProtocolContractError {}

/// Verifies that a peer advertises the only P0-C Judge Control version.
///
/// # Errors
///
/// Returns [`ProtocolContractError::UnsupportedVersion`] for every other value.
pub fn validate_protocol_version(version: &str) -> Result<(), ProtocolContractError> {
    if version == CLIENT_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolContractError::UnsupportedVersion)
    }
}

/// Parses a bounded, unique Judge Control capability set.
///
/// # Errors
///
/// Returns [`ProtocolContractError::InvalidCapabilities`] if a token is invalid, duplicated, or
/// the set exceeds the P0-C limit.
pub fn validate_capabilities(
    capabilities: &[String],
) -> Result<Vec<Capability>, ProtocolContractError> {
    if capabilities.len() > MAX_CAPABILITIES {
        return Err(ProtocolContractError::InvalidCapabilities);
    }

    let parsed = capabilities
        .iter()
        .map(|capability| Capability::parse(capability.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProtocolContractError::InvalidCapabilities)?;
    let unique = parsed.iter().collect::<BTreeSet<_>>();
    if unique.len() != parsed.len() {
        return Err(ProtocolContractError::InvalidCapabilities);
    }
    Ok(parsed)
}

/// Validates a bounded canonical `EvaluationRequest` carried by a Judge Control message.
///
/// # Errors
///
/// Returns [`ProtocolContractError::MessageTooLarge`] before decoding oversized messages and
/// [`ProtocolContractError::InvalidCanonicalPayload`] for every invalid canonical request.
pub fn validate_canonical_request(input: &[u8]) -> Result<(), ProtocolContractError> {
    if input.len() > MAX_EVALUATION_REQUEST_BYTES {
        return Err(ProtocolContractError::MessageTooLarge);
    }
    decode_evaluation_request(input)
        .map(|_| ())
        .map_err(|_| ProtocolContractError::InvalidCanonicalPayload)
}
