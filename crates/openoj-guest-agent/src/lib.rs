//! In-guest command agent for the P0 execution plane.
//!
//! The agent listens on the guest vsock port, parses bounded
//! [`openoj_guest_protocol::Message`] frames, and executes only the restricted
//! commands the host has validated. It provides no generic shell, no host path
//! access, and no network. Guest output is always re-treated as untrusted input
//! by the host evaluator.
//!
//! This crate is kept dependency-light so it can be bundled into a minimal guest
//! root filesystem.

use std::io::{Error as IoError, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use openoj_guest_protocol::{GuestDiagnostic, GuestUsage, Message, Stage};
use sha2::{Digest, Sha256};

/// Capabilities this agent declares during negotiation.
pub const SUPPORTED_CAPABILITIES: [&str; 1] = ["algorithm.batch"];

/// Directory (relative to the task work dir) where uploaded inputs are stored.
pub const INPUT_DIR: &str = "inputs";

/// Default working directory inside the guest when none is provided.
pub const DEFAULT_WORK_DIR: &str = "/work";

/// Exit code reported when a guest stage exceeds its wall-clock budget.
pub const TIMEOUT_EXIT_CODE: i32 = 124;

/// A bounded error produced by the guest agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentError {
    /// An I/O operation failed inside the guest.
    Io { message: String },
    /// The command argument vector was empty.
    EmptyArgv,
    /// The uploaded input name refers outside the task input directory.
    UnsafeName { name: String },
    /// An unexpected message type was received from the host.
    UnexpectedMessage { message_type: &'static str },
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { message } => write!(formatter, "guest agent I/O error: {message}"),
            Self::EmptyArgv => write!(formatter, "command argument vector is empty"),
            Self::UnsafeName { name } => write!(formatter, "unsafe input name: {name}"),
            Self::UnexpectedMessage { message_type } => {
                write!(formatter, "unexpected message type: {message_type}")
            }
        }
    }
}

impl std::error::Error for AgentError {}

impl From<IoError> for AgentError {
    fn from(error: IoError) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

/// Whether this agent supports a requested capability set.
#[must_use]
pub fn accepts_capabilities(capabilities: &[String]) -> bool {
    !capabilities.is_empty()
        && capabilities
            .iter()
            .all(|capability| SUPPORTED_CAPABILITIES.contains(&capability.as_str()))
}

/// Handles one host message, returning the guest response (or `None` to keep
/// waiting for a valid host→guest message).
///
/// # Errors
///
/// Returns [`AgentError`] when an upload name is unsafe or an I/O operation fails.
pub fn handle(message: Message, work: &Path) -> Result<Option<Message>, AgentError> {
    match message {
        Message::Negotiate { capabilities } => Ok(Some(Message::Negotiated {
            supported: accepts_capabilities(&capabilities),
        })),
        Message::UploadInput { name, bytes, .. } => {
            write_input(work, &name, &bytes)?;
            Ok(Some(Message::UploadAck {
                name,
                accepted: true,
            }))
        }
        Message::Build { argv, wall_time_ms } => {
            Ok(Some(stage_output(Stage::Build, &argv, work, wall_time_ms)?))
        }
        Message::Run { argv, wall_time_ms } => {
            Ok(Some(stage_output(Stage::Run, &argv, work, wall_time_ms)?))
        }
        Message::StageEvidence { kind } => Ok(Some(Message::EvidenceAck {
            kind,
            accepted: true,
        })),
        Message::Cancel => Ok(Some(Message::Cancelled)),
        Message::Heartbeat => Ok(Some(Message::Ack)),
        other => Err(AgentError::UnexpectedMessage {
            message_type: other.message_type(),
        }),
    }
}

fn write_input(work: &Path, name: &str, bytes: &[u8]) -> Result<(), AgentError> {
    let input_dir = work.join(INPUT_DIR);
    if !input_dir.is_dir() {
        std::fs::create_dir_all(&input_dir)?;
    }
    let path = safe_input_path(work, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(())
}

fn safe_input_path(work: &Path, name: &str) -> Result<PathBuf, AgentError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == ".."
        || name.starts_with("..")
    {
        return Err(AgentError::UnsafeName {
            name: name.to_owned(),
        });
    }
    Ok(work.join(INPUT_DIR).join(name))
}

/// Runs a validated command with a wall-clock budget and captures bounded output.
fn run_command(
    argv: &[String],
    work: &Path,
    wall_time_ms: u64,
) -> Result<StageOutputParts, AgentError> {
    if argv.is_empty() {
        return Err(AgentError::EmptyArgv);
    }
    let started = Instant::now();
    let child = spawn(argv, work)?;
    let output = wait_with_timeout(child, Duration::from_millis(wall_time_ms))?;
    let elapsed = started.elapsed();
    let stdout = truncate_bytes(&output.stdout, BOOT_OUTPUT_LIMIT);
    let combined = [&stdout[..], &output.stderr[..]].concat();
    let digest = hex(Sha256::digest(&combined));
    let output_bytes = u64::try_from(combined.len()).map_err(|_| AgentError::Io {
        message: "output length overflow".to_owned(),
    })?;
    let diagnostics = build_diagnostics(&output.stderr, BOOT_DIAGNOSTICS);
    let usage = GuestUsage::new(
        u64::try_from(elapsed.as_millis()).unwrap_or_default(),
        u64::try_from(elapsed.as_millis()).unwrap_or_default(),
        0,
        output_bytes,
    );
    Ok(StageOutputParts {
        exit_code: output.exit_code,
        output_digest: digest,
        output_bytes,
        usage,
        diagnostics,
    })
}

/// Runs a validated guest stage and wraps the result as a `StageOutput` message.
fn stage_output(
    stage: Stage,
    argv: &[String],
    work: &Path,
    wall_time_ms: u64,
) -> Result<Message, AgentError> {
    let parts = run_command(argv, work, wall_time_ms)?;
    Ok(Message::StageOutput {
        stage,
        exit_code: parts.exit_code,
        output_digest: parts.output_digest,
        output_bytes: parts.output_bytes,
        usage: parts.usage,
        diagnostics: parts.diagnostics,
    })
}

/// Lower-case hex encoding of a SHA-256 digest.
fn hex(digest: impl AsRef<[u8]>) -> String {
    let mut out = String::with_capacity(2 * 32);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn spawn(argv: &[String], work: &Path) -> Result<Child, AgentError> {
    Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(AgentError::from)
}

struct CommandOutput {
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn wait_with_timeout(mut child: Child, budget: Duration) -> Result<CommandOutput, AgentError> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = read_pipe(child.stdout.take().as_mut(), BOOT_OUTPUT_LIMIT)?;
            let stderr = read_pipe(child.stderr.take().as_mut(), BOOT_DIAGNOSTICS_BYTES)?;
            let exit_code = status.code().unwrap_or(-1);
            return Ok(CommandOutput {
                exit_code,
                stdout,
                stderr,
            });
        }
        if start.elapsed() >= budget {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(CommandOutput {
                exit_code: TIMEOUT_EXIT_CODE,
                stdout: Vec::new(),
                stderr: b"guest stage timed out".to_vec(),
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

const BOOT_OUTPUT_LIMIT: usize = 1_048_576;
const BOOT_DIAGNOSTICS: usize = 32;
const BOOT_DIAGNOSTICS_BYTES: usize = 8192;

fn read_pipe<R: Read>(reader: Option<&mut R>, limit: usize) -> Result<Vec<u8>, AgentError> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = reader.read(&mut buf).map_err(|error| AgentError::Io {
            message: error.to_string(),
        })?;
        if read == 0 {
            break;
        }
        if out.len() >= limit {
            break;
        }
        let take = (limit - out.len()).min(read);
        out.extend_from_slice(&buf[..take]);
    }
    Ok(out)
}

fn truncate_bytes(bytes: &[u8], limit: usize) -> Vec<u8> {
    bytes.iter().copied().take(limit).collect()
}

fn build_diagnostics(stderr: &[u8], limit: usize) -> Vec<GuestDiagnostic> {
    if stderr.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .take(limit)
        .map(|line| GuestDiagnostic::new("guest_stderr", line))
        .collect()
}

/// The stage-output fields produced by a command run.
struct StageOutputParts {
    exit_code: i32,
    output_digest: String,
    output_bytes: u64,
    usage: GuestUsage,
    diagnostics: Vec<GuestDiagnostic>,
}

/// Reads one message frame and writes one response frame over a vsock stream.
///
/// Returns `Ok(false)` when the connection closes cleanly.
///
/// # Errors
///
/// Returns [`AgentError`] on read/decode or write failures.
pub fn serve_frame<S: Read + Write>(stream: &mut S, work: &Path) -> Result<bool, AgentError> {
    let mut length_buf = [0u8; 4];
    let read = stream.read(&mut length_buf)?;
    if read == 0 {
        return Ok(false);
    }
    if read < 4 {
        return Err(AgentError::Io {
            message: "truncated length prefix".to_owned(),
        });
    }
    let length = u32::from_be_bytes(length_buf) as usize;
    if length > openoj_guest_protocol::MAX_FRAME_BYTES {
        return Err(AgentError::Io {
            message: format!("frame length {length} exceeds bound"),
        });
    }
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&length_buf);
    frame.resize(4 + length, 0);
    stream
        .read_exact(&mut frame[4..])
        .map_err(|error| AgentError::Io {
            message: error.to_string(),
        })?;
    let message =
        openoj_guest_protocol::Message::decode(&frame).map_err(|error| AgentError::Io {
            message: format!("decode {error}"),
        })?;
    let response = handle(message, work)?;
    if let Some(response) = response {
        let response_frame = response.encode().map_err(|error| AgentError::Io {
            message: format!("encode {error}"),
        })?;
        stream.write_all(&response_frame)?;
        stream.flush()?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openoj-guest-agent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn negotiate_supported_capabilities() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        let response = handle(
            Message::Negotiate {
                capabilities: vec!["algorithm.batch".to_owned()],
            },
            &work,
        )?;
        assert_eq!(response, Some(Message::Negotiated { supported: true }));
        Ok(())
    }

    #[test]
    fn negotiate_rejects_unknown_capability() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        let response = handle(
            Message::Negotiate {
                capabilities: vec!["algorithm.interactive".to_owned()],
            },
            &work,
        )?;
        assert_eq!(response, Some(Message::Negotiated { supported: false }));
        Ok(())
    }

    #[test]
    fn upload_persists_into_input_dir() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        let response = handle(
            Message::UploadInput {
                name: "main.txt".to_owned(),
                digest: "sha256:x".to_owned(),
                bytes: b"hello".to_vec(),
            },
            &work,
        )?;
        assert_eq!(
            response,
            Some(Message::UploadAck {
                name: "main.txt".to_owned(),
                accepted: true,
            })
        );
        let persisted = work.join(INPUT_DIR).join("main.txt");
        assert_eq!(std::fs::read(&persisted)?, b"hello");
        Ok(())
    }

    #[test]
    fn malicious_upload_name_is_rejected() {
        let work = work_dir();
        let result = handle(
            Message::UploadInput {
                name: "../../etc/passwd".to_owned(),
                digest: "sha256:x".to_owned(),
                bytes: Vec::new(),
            },
            &work,
        );
        assert!(matches!(result, Err(AgentError::UnsafeName { .. })));
    }

    #[test]
    fn heartbeat_is_acknowledged() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        assert_eq!(handle(Message::Heartbeat, &work)?, Some(Message::Ack));
        Ok(())
    }

    #[test]
    fn empty_argv_is_rejected() {
        let result = run_command(&[], Path::new("/tmp"), 1_000);
        assert!(matches!(result, Err(AgentError::EmptyArgv)));
    }

    #[test]
    fn run_command_captures_output() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        let parts = run_command(
            &[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "printf ok; printf warn >&2".to_owned(),
            ],
            &work,
            2_000,
        )?;
        assert_eq!(parts.exit_code, 0);
        assert_eq!(parts.output_digest, hex(Sha256::digest(b"okwarn")));
        assert_eq!(
            parts.diagnostics.first().map(GuestDiagnostic::message),
            Some("warn")
        );
        Ok(())
    }

    #[test]
    fn timeout_produces_deterministic_exit() -> Result<(), Box<dyn std::error::Error>> {
        let work = work_dir();
        let parts = run_command(&["/bin/sleep".to_owned(), "5".to_owned()], &work, 50)?;
        assert_eq!(parts.exit_code, TIMEOUT_EXIT_CODE);
        Ok(())
    }
}
