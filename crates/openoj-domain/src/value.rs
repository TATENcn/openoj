use std::fmt::{self, Display, Formatter};

use crate::DomainError;

pub const MAX_OPAQUE_ID_LENGTH: usize = 64;
pub const MAX_IDEMPOTENCY_KEY_LENGTH: usize = 128;
pub const MAX_CAPABILITY_LENGTH: usize = 64;
pub const MAX_DIAGNOSTIC_CODE_LENGTH: usize = 64;
pub const MAX_EVIDENCE_KIND_LENGTH: usize = 64;

fn validate_opaque(value: &str, field: &'static str, maximum: usize) -> Result<(), DomainError> {
    if value.is_empty() {
        return Err(DomainError::Empty { field });
    }
    if value.len() > maximum {
        return Err(DomainError::TooLong {
            field,
            maximum,
            actual: value.len(),
        });
    }

    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err(DomainError::Empty { field });
    };
    if !first.is_ascii_alphanumeric()
        || !bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(DomainError::InvalidFormat {
            field,
            reason: "use ASCII letters, digits, dot, underscore, colon, or hyphen",
        });
    }

    Ok(())
}

macro_rules! opaque_id {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Parses a bounded opaque identifier without assigning semantics from another ID type.
            ///
            /// # Errors
            ///
            /// Returns [`DomainError`] when the identifier is empty, oversized, or contains
            /// characters outside the canonical opaque-ID alphabet.
            pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                validate_opaque(&value, $field, MAX_OPAQUE_ID_LENGTH)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

opaque_id!(RequestId, "request_id");
opaque_id!(ProblemId, "problem_id");
opaque_id!(ProblemVersionId, "problem_version_id");
opaque_id!(SubmissionId, "submission_id");
opaque_id!(EvaluationId, "evaluation_id");
opaque_id!(AttemptId, "attempt_id");
opaque_id!(RuntimeId, "runtime_id");
opaque_id!(ArtifactId, "artifact_id");
opaque_id!(EvidenceId, "evidence_id");
opaque_id!(NodeId, "node_id");
opaque_id!(LeaseToken, "lease_token");
opaque_id!(ClaimOperationId, "claim_operation_id");
opaque_id!(ResultOperationId, "result_operation_id");

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Parses a caller-stable idempotency key.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the key is empty, oversized, or malformed.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        validate_opaque(&value, "idempotency_key", MAX_IDEMPOTENCY_KEY_LENGTH)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentDigest(String);

impl ContentDigest {
    /// Parses a lowercase SHA-256 content digest.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] unless the value is `sha256:` followed by exactly 64 lowercase
    /// hexadecimal characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(DomainError::InvalidFormat {
                field: "content_digest",
                reason: "only lowercase sha256 digests are accepted",
            });
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DomainError::InvalidFormat {
                field: "content_digest",
                reason: "expected sha256 followed by 64 lowercase hexadecimal characters",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ContentDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn validate_lower_token(
    value: &str,
    field: &'static str,
    maximum: usize,
) -> Result<(), DomainError> {
    if value.is_empty() {
        return Err(DomainError::Empty { field });
    }
    if value.len() > maximum {
        return Err(DomainError::TooLong {
            field,
            maximum,
            actual: value.len(),
        });
    }

    let mut segment_start = true;
    for byte in value.bytes() {
        if matches!(byte, b'.' | b'_' | b':' | b'-') {
            if segment_start {
                return Err(DomainError::InvalidFormat {
                    field,
                    reason: "separators must occur between lowercase alphanumeric segments",
                });
            }
            segment_start = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            if segment_start && byte.is_ascii_digit() && value.as_bytes().first() == Some(&byte) {
                return Err(DomainError::InvalidFormat {
                    field,
                    reason: "the first segment must start with a lowercase letter",
                });
            }
            segment_start = false;
        } else {
            return Err(DomainError::InvalidFormat {
                field,
                reason: "use lowercase ASCII letters, digits, and bounded separators",
            });
        }
    }

    if segment_start {
        return Err(DomainError::InvalidFormat {
            field,
            reason: "the value must not end with a separator",
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Capability(String);

impl Capability {
    /// Parses a bounded, lowercase capability token.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the token is empty, oversized, or uses an invalid segment.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        validate_lower_token(&value, "capability", MAX_CAPABILITY_LENGTH)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvidenceKind(String);

impl EvidenceKind {
    /// Parses a bounded evidence-kind token.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the token is empty, oversized, or malformed.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        validate_lower_token(&value, "evidence_kind", MAX_EVIDENCE_KIND_LENGTH)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(String);

impl DiagnosticCode {
    /// Parses an uppercase, machine-readable diagnostic code.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the code is empty, oversized, or malformed.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DomainError::Empty {
                field: "diagnostic_code",
            });
        }
        if value.len() > MAX_DIAGNOSTIC_CODE_LENGTH {
            return Err(DomainError::TooLong {
                field: "diagnostic_code",
                maximum: MAX_DIAGNOSTIC_CODE_LENGTH,
                actual: value.len(),
            });
        }
        if !value.as_bytes()[0].is_ascii_uppercase()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(DomainError::InvalidFormat {
                field: "diagnostic_code",
                reason: "use uppercase ASCII letters, digits, and underscore",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, ClaimOperationId, ContentDigest, DiagnosticCode, ProblemId};

    #[test]
    fn identifiers_are_distinct_and_bounded() {
        assert!(ProblemId::parse("problem_01").is_ok());
        assert!(ProblemId::parse("-bad").is_err());
        assert!(ProblemId::parse("x".repeat(65)).is_err());
    }

    #[test]
    fn digest_requires_lowercase_sha256() {
        let valid = format!("sha256:{}", "a".repeat(64));
        assert!(ContentDigest::parse(valid).is_ok());
        assert!(ContentDigest::parse(format!("sha256:{}", "A".repeat(64))).is_err());
    }

    #[test]
    fn semantic_tokens_reject_ambiguous_forms() {
        assert!(Capability::parse("algorithm.batch").is_ok());
        assert!(Capability::parse("algorithm..batch").is_err());
        assert!(DiagnosticCode::parse("STAGE_FAILED").is_ok());
        assert!(DiagnosticCode::parse("stage_failed").is_err());
    }

    #[test]
    fn claim_operation_id_uses_the_opaque_identifier_bound() {
        assert!(ClaimOperationId::parse("claim_01").is_ok());
        assert!(ClaimOperationId::parse("a".repeat(65)).is_err());
    }
}
