use crate::{ArtifactId, ContentDigest, DomainError};

pub const MAX_ARTIFACT_BYTES: u64 = 1_099_511_627_776;
pub const MAX_MEDIA_TYPE_LENGTH: usize = 129;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactSensitivity {
    Public,
    Private,
    Hidden,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaType(String);

impl MediaType {
    /// Parses a bounded `type/subtype` media type without parameters.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when either token is missing, oversized, or contains unsupported
    /// characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.len() > MAX_MEDIA_TYPE_LENGTH {
            return Err(DomainError::TooLong {
                field: "media_type",
                maximum: MAX_MEDIA_TYPE_LENGTH,
                actual: value.len(),
            });
        }
        let Some((top_level, subtype)) = value.split_once('/') else {
            return Err(DomainError::InvalidFormat {
                field: "media_type",
                reason: "expected type/subtype",
            });
        };
        if top_level.is_empty()
            || subtype.is_empty()
            || top_level.len() > 64
            || subtype.len() > 64
            || !top_level.bytes().all(is_media_type_byte)
            || !subtype.bytes().all(is_media_type_byte)
        {
            return Err(DomainError::InvalidFormat {
                field: "media_type",
                reason: "type and subtype must be bounded ASCII tokens",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_media_type_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRef {
    artifact_id: ArtifactId,
    digest: ContentDigest,
    media_type: MediaType,
    size_bytes: u64,
    sensitivity: ArtifactSensitivity,
}

impl ArtifactRef {
    /// Creates a content-addressed artifact reference.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the declared artifact size exceeds the protocol hard limit.
    pub fn new(
        artifact_id: ArtifactId,
        digest: ContentDigest,
        media_type: MediaType,
        size_bytes: u64,
        sensitivity: ArtifactSensitivity,
    ) -> Result<Self, DomainError> {
        if size_bytes > MAX_ARTIFACT_BYTES {
            return Err(DomainError::OutOfRange {
                field: "artifact_size_bytes",
                minimum: 0,
                maximum: MAX_ARTIFACT_BYTES,
                actual: size_bytes,
            });
        }
        Ok(Self {
            artifact_id,
            digest,
            media_type,
            size_bytes,
            sensitivity,
        })
    }

    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        &self.digest
    }

    #[must_use]
    pub const fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    #[must_use]
    pub const fn sensitivity(&self) -> ArtifactSensitivity {
        self.sensitivity
    }
}
