//! Bounded, versioned guest↔host vsock message codec for the P0 execution plane.
//!
//! This crate defines the canonical wire messages exchanged between a judge-node
//! host adapter (`openoj-firecracker`) and the in-guest command agent
//! (`openoj-guest-agent`) over a Firecracker vsock. Every frame is bounded and
//! versioned; guest output is always re-treated as untrusted input by the host
//! (see `docs/protocol/guest-vsock-v0alpha1.md`).

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// Exact protocol version spoken by this crate.
pub const PROTOCOL_VERSION: &str = "v0alpha1";

/// Maximum encoded frame size, including the 4-byte length prefix.
pub const MAX_FRAME_BYTES: usize = 1_048_576;

/// Maximum length of a message `type` discriminator.
pub const MAX_TYPE_LEN: usize = 32;

/// Maximum number of capability strings in a negotiate message.
pub const MAX_CAPABILITIES: usize = 32;

/// Maximum length of a single capability string.
pub const MAX_CAPABILITY_LEN: usize = 32;

/// Maximum length of a content digest string (a 64-hex SHA-256).
pub const MAX_DIGEST_LEN: usize = 64;

/// Maximum inline input/evidence payload carried by a frame.
pub const MAX_INLINE_BYTES: usize = 262_144;

/// Maximum length of an uploaded input or evidence name.
pub const MAX_NAME_LEN: usize = 128;

/// Maximum number of command arguments in a build/run message.
pub const MAX_ARGUMENTS: usize = 32;

/// Maximum length of a single command argument.
pub const MAX_ARGUMENT_LEN: usize = 256;

/// Maximum number of diagnostics in a stage output.
pub const MAX_DIAGNOSTICS: usize = 32;

/// Maximum length of a single diagnostic message.
pub const MAX_DIAGNOSTIC_MESSAGE: usize = 4096;

/// A bounded error produced while encoding or decoding a guest message frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    /// The encoded frame exceeded `MAX_FRAME_BYTES`.
    FrameTooLarge { actual: usize, maximum: usize },
    /// The declared length prefix exceeded `MAX_FRAME_BYTES`.
    BoundedLengthExceeded { actual: usize, maximum: usize },
    /// The frame is not a 4-byte length prefix followed by at least one byte.
    Truncated,
    /// The frame body is not valid JSON.
    MalformedJson,
    /// The message has no `type` field, an unknown `type`, or a non-string type.
    UnknownType,
    /// The message `type` exceeds `MAX_TYPE_LEN`.
    TypeTooLong,
    /// The message is missing or has a mismatched `version`.
    UnsupportedVersion,
    /// A typed field exceeded its declared bound.
    BoundsExceeded {
        field: String,
        actual: usize,
        maximum: usize,
    },
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "encoded frame is {actual} bytes, maximum {maximum}"
                )
            }
            Self::BoundedLengthExceeded { actual, maximum } => write!(
                formatter,
                "declared frame length {actual} exceeds maximum {maximum}"
            ),
            Self::Truncated => write!(formatter, "frame is truncated"),
            Self::MalformedJson => write!(formatter, "frame body is not valid JSON"),
            Self::UnknownType => write!(formatter, "message type is missing or unknown"),
            Self::TypeTooLong => write!(formatter, "message type is too long"),
            Self::UnsupportedVersion => {
                write!(formatter, "message version is missing or unsupported")
            }
            Self::BoundsExceeded {
                field,
                actual,
                maximum,
            } => write!(formatter, "field {field} is {actual}, maximum {maximum}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// A step of the guest execution pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// Compile the uploaded submission into an executable.
    Build,
    /// Execute the built program against a test input.
    Run,
}

impl Stage {
    /// Returns the canonical wire name of this stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
        }
    }

    fn parse(value: &str) -> Result<Self, CodecError> {
        match value {
            "build" => Ok(Self::Build),
            "run" => Ok(Self::Run),
            _ => Err(CodecError::UnknownType),
        }
    }
}

/// Bounded resource usage reported by the guest for a stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GuestUsage {
    cpu_time_ms: u64,
    wall_time_ms: u64,
    memory_peak_bytes: u64,
    output_bytes: u64,
}

impl GuestUsage {
    /// Creates a bounded guest usage summary.
    #[must_use]
    pub const fn new(
        cpu_time_ms: u64,
        wall_time_ms: u64,
        memory_peak_bytes: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            cpu_time_ms,
            wall_time_ms,
            memory_peak_bytes,
            output_bytes,
        }
    }

    /// CPU time consumed in milliseconds.
    #[must_use]
    pub const fn cpu_time_ms(self) -> u64 {
        self.cpu_time_ms
    }

    /// Wall-clock time consumed in milliseconds.
    #[must_use]
    pub const fn wall_time_ms(self) -> u64 {
        self.wall_time_ms
    }

    /// Peak memory resident set size in bytes.
    #[must_use]
    pub const fn memory_peak_bytes(self) -> u64 {
        self.memory_peak_bytes
    }

    /// Captured output bytes.
    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }
}

/// A bounded diagnostic produced by a guest stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestDiagnostic {
    code: String,
    message: String,
    truncated: bool,
}

impl GuestDiagnostic {
    /// Creates a diagnostic whose message is truncated to the protocol bound.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let message = message.into();
        let truncated = message.len() > MAX_DIAGNOSTIC_MESSAGE;
        let message = truncate(&message, MAX_DIAGNOSTIC_MESSAGE);
        Self {
            code,
            message,
            truncated,
        }
    }

    /// Stable diagnostic code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Diagnostic text, always within `MAX_DIAGNOSTIC_MESSAGE` bytes.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the diagnostic message was truncated at the protocol bound.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// A bounded, versioned guest↔host message.
///
/// The message set intentionally excludes generic shell access, host path
/// access, and raw network access. `*` names the sender.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// Host→guest: offer the frame protocol version and a sorted capability set.
    ///
    /// The frame-level `version` field is the offered protocol version; the guest
    /// accepts or rejects it in [`Message::Negotiated`].
    Negotiate { capabilities: Vec<String> },
    /// Guest→host: accept or reject the offered protocol version/capabilities.
    Negotiated { supported: bool },
    /// Host→guest: upload one bounded input or artifact referenced by name+digest.
    UploadInput {
        name: String,
        digest: String,
        bytes: Vec<u8>,
    },
    /// Guest→host: acknowledge an uploaded input.
    UploadAck { name: String, accepted: bool },
    /// Host→guest: run the compile stage with a bounded argument vector.
    Build {
        argv: Vec<String>,
        wall_time_ms: u64,
    },
    /// Host→guest: run the execute stage with a bounded argument vector.
    Run {
        argv: Vec<String>,
        wall_time_ms: u64,
    },
    /// Guest→host: report a stage's output, usage, and bounded diagnostics.
    StageOutput {
        stage: Stage,
        exit_code: i32,
        output_digest: String,
        output_bytes: u64,
        usage: GuestUsage,
        diagnostics: Vec<GuestDiagnostic>,
    },
    /// Host→guest: request a structured evidence artifact from the guest.
    StageEvidence { kind: String },
    /// Guest→host: acknowledge an evidence request.
    EvidenceAck { kind: String, accepted: bool },
    /// Host→guest: cancel the current stage and converge to a terminal state.
    Cancel,
    /// Guest→host: acknowledge cancellation.
    Cancelled,
    /// Host→guest: liveness probe.
    Heartbeat,
    /// Guest→host: acknowledge a heartbeat.
    Ack,
}

const TYPE_NEGOTIATE: &str = "negotiate";
const TYPE_NEGOTIATED: &str = "negotiated";
const TYPE_UPLOAD_INPUT: &str = "upload_input";
const TYPE_UPLOAD_ACK: &str = "upload_ack";
const TYPE_BUILD: &str = "build";
const TYPE_RUN: &str = "run";
const TYPE_STAGE_OUTPUT: &str = "stage_output";
const TYPE_STAGE_EVIDENCE: &str = "stage_evidence";
const TYPE_EVIDENCE_ACK: &str = "evidence_ack";
const TYPE_CANCEL: &str = "cancel";
const TYPE_CANCELLED: &str = "cancelled";
const TYPE_HEARTBEAT: &str = "heartbeat";
const TYPE_ACK: &str = "ack";

impl Message {
    /// Canonical wire `type` of this message.
    #[must_use]
    pub const fn message_type(&self) -> &'static str {
        match self {
            Self::Negotiate { .. } => TYPE_NEGOTIATE,
            Self::Negotiated { .. } => TYPE_NEGOTIATED,
            Self::UploadInput { .. } => TYPE_UPLOAD_INPUT,
            Self::UploadAck { .. } => TYPE_UPLOAD_ACK,
            Self::Build { .. } => TYPE_BUILD,
            Self::Run { .. } => TYPE_RUN,
            Self::StageOutput { .. } => TYPE_STAGE_OUTPUT,
            Self::StageEvidence { .. } => TYPE_STAGE_EVIDENCE,
            Self::EvidenceAck { .. } => TYPE_EVIDENCE_ACK,
            Self::Cancel => TYPE_CANCEL,
            Self::Cancelled => TYPE_CANCELLED,
            Self::Heartbeat => TYPE_HEARTBEAT,
            Self::Ack => TYPE_ACK,
        }
    }

    /// Encodes this message into a bounded, length-prefixed frame.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] when a field exceeds its declared bound or the
    /// encoded frame exceeds `MAX_FRAME_BYTES`.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut object = Map::new();
        object.insert(
            "type".to_owned(),
            Value::String(self.message_type().to_owned()),
        );
        object.insert(
            "version".to_owned(),
            Value::String(PROTOCOL_VERSION.to_owned()),
        );
        self.write_fields(&mut object)?;
        let body =
            serde_json::to_vec(&Value::Object(object)).map_err(|_| CodecError::MalformedJson)?;
        let total = body.len().checked_add(4).ok_or(CodecError::FrameTooLarge {
            actual: usize::MAX,
            maximum: MAX_FRAME_BYTES,
        })?;
        if total > MAX_FRAME_BYTES {
            return Err(CodecError::FrameTooLarge {
                actual: total,
                maximum: MAX_FRAME_BYTES,
            });
        }
        let body_length = u32::try_from(body.len()).map_err(|_| CodecError::FrameTooLarge {
            actual: body.len(),
            maximum: MAX_FRAME_BYTES,
        })?;
        let mut frame = Vec::with_capacity(total);
        frame.extend_from_slice(&body_length.to_be_bytes());
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    fn write_fields(&self, object: &mut Map<String, Value>) -> Result<(), CodecError> {
        match self {
            Self::Negotiate { capabilities } => {
                validate_capabilities(capabilities)?;
                object.insert("capabilities".to_owned(), json_array(capabilities));
                Ok(())
            }
            Self::Negotiated { supported } => {
                object.insert("supported".to_owned(), Value::Bool(*supported));
                Ok(())
            }
            Self::UploadInput {
                name,
                digest,
                bytes,
            } => {
                check_name(name)?;
                if digest.len() > MAX_DIGEST_LEN {
                    return Err(bounds("digest", digest.len(), MAX_DIGEST_LEN));
                }
                if bytes.len() > MAX_INLINE_BYTES {
                    return Err(bounds("bytes", bytes.len(), MAX_INLINE_BYTES));
                }
                object.insert("name".to_owned(), Value::String(name.clone()));
                object.insert("digest".to_owned(), Value::String(digest.clone()));
                object.insert("bytes".to_owned(), json_bytes(bytes));
                Ok(())
            }
            Self::UploadAck { name, accepted } => {
                check_name(name)?;
                object.insert("name".to_owned(), Value::String(name.clone()));
                object.insert("accepted".to_owned(), Value::Bool(*accepted));
                Ok(())
            }
            Self::Build { argv, wall_time_ms } | Self::Run { argv, wall_time_ms } => {
                write_command(object, argv, *wall_time_ms)?;
                Ok(())
            }
            Self::StageOutput {
                stage,
                exit_code,
                output_digest,
                output_bytes,
                usage,
                diagnostics,
            } => {
                if output_digest.len() > MAX_DIGEST_LEN {
                    return Err(bounds("output_digest", output_digest.len(), MAX_DIGEST_LEN));
                }
                if diagnostics.len() > MAX_DIAGNOSTICS {
                    return Err(bounds("diagnostics", diagnostics.len(), MAX_DIAGNOSTICS));
                }
                object.insert("stage".to_owned(), Value::String(stage.as_str().to_owned()));
                object.insert(
                    "exit_code".to_owned(),
                    Value::Number(serde_json::Number::from(*exit_code)),
                );
                object.insert(
                    "output_digest".to_owned(),
                    Value::String(output_digest.clone()),
                );
                object.insert(
                    "output_bytes".to_owned(),
                    Value::Number(serde_json::Number::from(*output_bytes)),
                );
                object.insert("usage".to_owned(), usage_value(*usage));
                object.insert("diagnostics".to_owned(), diagnostics_value(diagnostics));
                Ok(())
            }
            Self::StageEvidence { kind } => {
                if kind.len() > MAX_TYPE_LEN {
                    return Err(bounds("kind", kind.len(), MAX_TYPE_LEN));
                }
                object.insert("kind".to_owned(), Value::String(kind.clone()));
                Ok(())
            }
            Self::EvidenceAck { kind, accepted } => {
                object.insert("kind".to_owned(), Value::String(kind.clone()));
                object.insert("accepted".to_owned(), Value::Bool(*accepted));
                Ok(())
            }
            Self::Cancel | Self::Cancelled | Self::Heartbeat | Self::Ack => Ok(()),
        }
    }

    /// Decodes a complete length-prefixed frame into a message.
    ///
    /// # Errors
    ///
    /// Returns [`CodecError`] when the frame is truncated, oversized, malformed,
    /// unversioned, unknown-typed, or exceeds a field bound.
    pub fn decode(frame: &[u8]) -> Result<Self, CodecError> {
        if frame.len() < 4 {
            return Err(CodecError::Truncated);
        }
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&frame[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;
        if length > MAX_FRAME_BYTES.saturating_sub(4) {
            return Err(CodecError::BoundedLengthExceeded {
                actual: length,
                maximum: MAX_FRAME_BYTES - 4,
            });
        }
        if frame.len() < 4 + length {
            return Err(CodecError::Truncated);
        }
        let body = &frame[4..4 + length];
        let value: Value = serde_json::from_slice(body).map_err(|_| CodecError::MalformedJson)?;
        Self::from_value(&value)
    }

    fn from_value(value: &Value) -> Result<Self, CodecError> {
        let object = value.as_object().ok_or(CodecError::MalformedJson)?;
        let type_name = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or(CodecError::UnknownType)?;
        if type_name.len() > MAX_TYPE_LEN {
            return Err(CodecError::TypeTooLong);
        }
        let version = object
            .get("version")
            .and_then(Value::as_str)
            .ok_or(CodecError::UnsupportedVersion)?;
        if version != PROTOCOL_VERSION {
            return Err(CodecError::UnsupportedVersion);
        }
        match type_name {
            TYPE_NEGOTIATE => {
                let capabilities = parse_capabilities(object)?;
                Ok(Self::Negotiate { capabilities })
            }
            TYPE_NEGOTIATED => {
                let supported = object
                    .get("supported")
                    .and_then(Value::as_bool)
                    .ok_or(CodecError::MalformedJson)?;
                Ok(Self::Negotiated { supported })
            }
            TYPE_UPLOAD_INPUT => {
                let name = string_field(object, "name", MAX_NAME_LEN)?;
                let digest = string_field(object, "digest", MAX_DIGEST_LEN)?;
                let bytes = byte_field(object, "bytes")?;
                Ok(Self::UploadInput {
                    name,
                    digest,
                    bytes,
                })
            }
            TYPE_UPLOAD_ACK => {
                let name = string_field(object, "name", MAX_NAME_LEN)?;
                let accepted = bool_field(object, "accepted")?;
                Ok(Self::UploadAck { name, accepted })
            }
            TYPE_BUILD => readable_command(object)
                .map(|(argv, wall_time_ms)| Self::Build { argv, wall_time_ms }),
            TYPE_RUN => readable_command(object)
                .map(|(argv, wall_time_ms)| Self::Run { argv, wall_time_ms }),
            TYPE_STAGE_OUTPUT => {
                let stage = Stage::parse(string_field(object, "stage", MAX_TYPE_LEN)?.as_str())?;
                let exit_code = i32_field(object, "exit_code")?;
                let output_digest = string_field(object, "output_digest", MAX_DIGEST_LEN)?;
                let output_bytes = u64_field(object, "output_bytes")?;
                let usage = usage_from(object.get("usage").ok_or(CodecError::MalformedJson)?)?;
                let diagnostics =
                    diagnostics_from(object.get("diagnostics").ok_or(CodecError::MalformedJson)?)?;
                Ok(Self::StageOutput {
                    stage,
                    exit_code,
                    output_digest,
                    output_bytes,
                    usage,
                    diagnostics,
                })
            }
            TYPE_STAGE_EVIDENCE => {
                let kind = string_field(object, "kind", MAX_TYPE_LEN)?;
                Ok(Self::StageEvidence { kind })
            }
            TYPE_EVIDENCE_ACK => {
                let kind = string_field(object, "kind", MAX_TYPE_LEN)?;
                let accepted = bool_field(object, "accepted")?;
                Ok(Self::EvidenceAck { kind, accepted })
            }
            TYPE_CANCEL => Ok(Self::Cancel),
            TYPE_CANCELLED => Ok(Self::Cancelled),
            TYPE_HEARTBEAT => Ok(Self::Heartbeat),
            TYPE_ACK => Ok(Self::Ack),
            _ => Err(CodecError::UnknownType),
        }
    }
}

fn bounds(field: &str, actual: usize, maximum: usize) -> CodecError {
    CodecError::BoundsExceeded {
        field: field.to_owned(),
        actual,
        maximum,
    }
}

fn truncate(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_owned();
    }
    let mut end = maximum;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn is_sorted_unique(values: &[String]) -> bool {
    let set: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    set.len() == values.len() && set.iter().copied().eq(values.iter().map(String::as_str))
}

fn validate_capabilities(capabilities: &[String]) -> Result<(), CodecError> {
    if capabilities.len() > MAX_CAPABILITIES {
        return Err(bounds("capabilities", capabilities.len(), MAX_CAPABILITIES));
    }
    if !is_sorted_unique(capabilities) {
        return Err(CodecError::BoundsExceeded {
            field: "capabilities".to_owned(),
            actual: capabilities.len(),
            maximum: MAX_CAPABILITIES,
        });
    }
    for capability in capabilities {
        if capability.len() > MAX_CAPABILITY_LEN {
            return Err(bounds("capability", capability.len(), MAX_CAPABILITY_LEN));
        }
    }
    Ok(())
}

fn parse_capabilities(object: &Map<String, Value>) -> Result<Vec<String>, CodecError> {
    let capabilities = string_vec(object, "capabilities", MAX_CAPABILITIES)?;
    validate_capabilities(&capabilities)?;
    Ok(capabilities)
}

fn json_array(values: &[String]) -> Value {
    Value::Array(values.iter().map(|v| Value::String(v.clone())).collect())
}

fn json_bytes(bytes: &[u8]) -> Value {
    Value::Array(
        bytes
            .iter()
            .map(|b| Value::Number(serde_json::Number::from(*b)))
            .collect(),
    )
}

fn check_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(bounds("name", name.len(), MAX_NAME_LEN));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(bounds("name", name.len(), MAX_NAME_LEN));
    }
    Ok(())
}

fn string_field(
    object: &Map<String, Value>,
    key: &str,
    maximum: usize,
) -> Result<String, CodecError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(CodecError::MalformedJson)?;
    if value.len() > maximum {
        return Err(bounds(key, value.len(), maximum));
    }
    Ok(value.to_owned())
}

fn string_vec(
    object: &Map<String, Value>,
    key: &str,
    maximum: usize,
) -> Result<Vec<String>, CodecError> {
    let Some(array) = object.get(key).and_then(Value::as_array) else {
        return Err(CodecError::MalformedJson);
    };
    if array.len() > maximum {
        return Err(bounds(key, array.len(), maximum));
    }
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let Some(s) = item.as_str() else {
            return Err(CodecError::MalformedJson);
        };
        out.push(s.to_owned());
    }
    Ok(out)
}

fn byte_field(object: &Map<String, Value>, key: &str) -> Result<Vec<u8>, CodecError> {
    let Some(array) = object.get(key).and_then(Value::as_array) else {
        return Err(CodecError::MalformedJson);
    };
    if array.len() > MAX_INLINE_BYTES {
        return Err(bounds("bytes", array.len(), MAX_INLINE_BYTES));
    }
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let Some(number) = item.as_u64() else {
            return Err(CodecError::MalformedJson);
        };
        if number > u64::from(u8::MAX) {
            return Err(CodecError::MalformedJson);
        }
        let byte = u8::try_from(number).map_err(|_| CodecError::MalformedJson)?;
        out.push(byte);
    }
    Ok(out)
}

fn bool_field(object: &Map<String, Value>, key: &str) -> Result<bool, CodecError> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or(CodecError::MalformedJson)
}

fn i32_field(object: &Map<String, Value>, key: &str) -> Result<i32, CodecError> {
    let number = object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or(CodecError::MalformedJson)?;
    i32::try_from(number).map_err(|_| CodecError::MalformedJson)
}

fn u64_field(object: &Map<String, Value>, key: &str) -> Result<u64, CodecError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(CodecError::MalformedJson)
}

fn write_command(
    object: &mut Map<String, Value>,
    argv: &[String],
    wall_time_ms: u64,
) -> Result<(), CodecError> {
    if argv.is_empty() || argv.len() > MAX_ARGUMENTS {
        return Err(bounds("argv", argv.len(), MAX_ARGUMENTS));
    }
    for argument in argv {
        if argument.len() > MAX_ARGUMENT_LEN {
            return Err(bounds("argument", argument.len(), MAX_ARGUMENT_LEN));
        }
    }
    object.insert("argv".to_owned(), json_array(argv));
    object.insert(
        "wall_time_ms".to_owned(),
        Value::Number(serde_json::Number::from(wall_time_ms)),
    );
    Ok(())
}

fn readable_command(object: &Map<String, Value>) -> Result<(Vec<String>, u64), CodecError> {
    let argv = string_vec(object, "argv", MAX_ARGUMENTS)?;
    if argv.is_empty() {
        return Err(CodecError::BoundsExceeded {
            field: "argv".to_owned(),
            actual: 0,
            maximum: MAX_ARGUMENTS,
        });
    }
    for argument in &argv {
        if argument.len() > MAX_ARGUMENT_LEN {
            return Err(bounds("argument", argument.len(), MAX_ARGUMENT_LEN));
        }
    }
    let wall_time_ms = u64_field(object, "wall_time_ms")?;
    Ok((argv, wall_time_ms))
}

fn usage_value(usage: GuestUsage) -> Value {
    let mut object = Map::new();
    object.insert("cpu_time_ms".to_owned(), number(usage.cpu_time_ms()));
    object.insert("wall_time_ms".to_owned(), number(usage.wall_time_ms()));
    object.insert(
        "memory_peak_bytes".to_owned(),
        number(usage.memory_peak_bytes()),
    );
    object.insert("output_bytes".to_owned(), number(usage.output_bytes()));
    Value::Object(object)
}

fn usage_from(value: &Value) -> Result<GuestUsage, CodecError> {
    let object = value.as_object().ok_or(CodecError::MalformedJson)?;
    Ok(GuestUsage::new(
        u64_field(object, "cpu_time_ms")?,
        u64_field(object, "wall_time_ms")?,
        u64_field(object, "memory_peak_bytes")?,
        u64_field(object, "output_bytes")?,
    ))
}

fn diagnostics_value(diagnostics: &[GuestDiagnostic]) -> Value {
    Value::Array(
        diagnostics
            .iter()
            .map(|d| {
                let mut object = Map::new();
                object.insert("code".to_owned(), Value::String(d.code().to_owned()));
                object.insert("message".to_owned(), Value::String(d.message().to_owned()));
                object.insert("truncated".to_owned(), Value::Bool(d.truncated()));
                Value::Object(object)
            })
            .collect(),
    )
}

fn diagnostics_from(value: &Value) -> Result<Vec<GuestDiagnostic>, CodecError> {
    let Some(array) = value.as_array() else {
        return Err(CodecError::MalformedJson);
    };
    if array.len() > MAX_DIAGNOSTICS {
        return Err(bounds("diagnostics", array.len(), MAX_DIAGNOSTICS));
    }
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object().ok_or(CodecError::MalformedJson)?;
        let code = string_field(object, "code", MAX_TYPE_LEN)?;
        let message = string_field(object, "message", MAX_DIAGNOSTIC_MESSAGE)?;
        let truncated = bool_field(object, "truncated")?;
        out.push(GuestDiagnostic {
            code,
            message,
            truncated,
        });
    }
    Ok(out)
}

fn number(value: u64) -> Value {
    Value::Number(serde_json::Number::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(message: &Message) -> Result<(), Box<dyn std::error::Error>> {
        let frame = message.encode()?;
        let decoded = Message::decode(&frame)?;
        assert_eq!(decoded, *message);
        Ok(())
    }

    #[test]
    fn negotiate_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        roundtrip(&Message::Negotiate {
            capabilities: vec!["algorithm.batch".to_owned()],
        })
    }

    #[test]
    fn build_and_run_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        roundtrip(&Message::Build {
            argv: vec![
                "gcc".to_owned(),
                "main.c".to_owned(),
                "-o".to_owned(),
                "a.out".to_owned(),
            ],
            wall_time_ms: 5_000,
        })?;
        roundtrip(&Message::Run {
            argv: vec!["./a.out".to_owned()],
            wall_time_ms: 2_000,
        })
    }

    #[test]
    fn stage_output_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        roundtrip(&Message::StageOutput {
            stage: Stage::Run,
            exit_code: 0,
            output_digest: "sha256:abc".to_owned(),
            output_bytes: 12,
            usage: GuestUsage::new(10, 20, 4096, 12),
            diagnostics: vec![GuestDiagnostic::new("warn", "a short diagnostic")],
        })
    }

    #[test]
    fn upload_input_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        roundtrip(&Message::UploadInput {
            name: "main.c".to_owned(),
            digest: "sha256:def".to_owned(),
            bytes: vec![1, 2, 3, 250],
        })
    }

    #[test]
    fn control_messages_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        roundtrip(&Message::Cancel)?;
        roundtrip(&Message::Cancelled)?;
        roundtrip(&Message::Heartbeat)?;
        roundtrip(&Message::Ack)?;
        roundtrip(&Message::StageEvidence {
            kind: "stdout".to_owned(),
        })?;
        roundtrip(&Message::EvidenceAck {
            kind: "stdout".to_owned(),
            accepted: true,
        })?;
        roundtrip(&Message::UploadAck {
            name: "main.c".to_owned(),
            accepted: true,
        })
    }

    #[test]
    fn unknown_type_is_rejected() {
        let body = format!(r#"{{"type":"run_arbitrary","version":"{PROTOCOL_VERSION}"}}"#);
        let frame = frame_from(body.as_bytes());
        assert_eq!(Message::decode(&frame), Err(CodecError::UnknownType));
    }

    #[test]
    fn missing_or_wrong_version_is_rejected() {
        let missing = r#"{"type":"heartbeat"}"#;
        assert_eq!(
            Message::decode(&frame_from(missing.as_bytes())),
            Err(CodecError::UnsupportedVersion)
        );
        let wrong = r#"{"type":"heartbeat","version":"v0beta1"}"#;
        assert_eq!(
            Message::decode(&frame_from(wrong.as_bytes())),
            Err(CodecError::UnsupportedVersion)
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        let frame = frame_from(b"not json");
        assert_eq!(Message::decode(&frame), Err(CodecError::MalformedJson));
    }

    #[test]
    fn oversized_declared_length_is_rejected() {
        let mut frame = vec![0xff, 0xff, 0xff, 0xff];
        frame.extend_from_slice(b"{}");
        assert!(matches!(
            Message::decode(&frame),
            Err(CodecError::BoundedLengthExceeded { .. })
        ));
    }

    #[test]
    fn truncated_frame_is_rejected() {
        assert_eq!(
            Message::decode(&[0, 0, 0, 10, 1]),
            Err(CodecError::Truncated)
        );
        assert_eq!(Message::decode(&[0, 0, 1]), Err(CodecError::Truncated));
    }

    #[test]
    fn duplicate_capabilities_are_rejected() {
        let message = Message::Negotiate {
            capabilities: vec!["a".to_owned(), "a".to_owned()],
        };
        assert!(message.encode().is_err());
    }

    #[test]
    fn unsorted_capabilities_are_rejected() {
        let message = Message::Negotiate {
            capabilities: vec!["b".to_owned(), "a".to_owned()],
        };
        assert!(message.encode().is_err());
    }

    #[test]
    fn oversized_input_is_rejected() {
        let message = Message::UploadInput {
            name: "big".to_owned(),
            digest: "sha256:x".to_owned(),
            bytes: vec![0; MAX_INLINE_BYTES + 1],
        };
        assert!(message.encode().is_err());
    }

    #[test]
    fn name_with_separator_is_rejected() {
        let message = Message::UploadInput {
            name: "../etc/passwd".to_owned(),
            digest: "sha256:x".to_owned(),
            bytes: Vec::new(),
        };
        assert!(message.encode().is_err());
    }

    #[test]
    fn diagnostic_message_is_truncated() {
        let long = "x".repeat(MAX_DIAGNOSTIC_MESSAGE + 100);
        let diagnostic = GuestDiagnostic::new("warn", long);
        assert!(diagnostic.truncated());
        assert_eq!(diagnostic.message().len(), MAX_DIAGNOSTIC_MESSAGE);
        assert_eq!(diagnostic.message(), "x".repeat(MAX_DIAGNOSTIC_MESSAGE));
    }

    fn frame_from(body: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&u32::try_from(body.len()).unwrap_or_default().to_be_bytes());
        frame.extend_from_slice(body);
        frame
    }
}
