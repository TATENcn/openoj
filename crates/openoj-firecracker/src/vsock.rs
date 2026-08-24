//! Host-side vsock channel to the guest agent.
//!
//! With Firecracker 1.16 the host↔guest vsock device is bridged through a
//! host unix socket (`uds_path`). To reach the guest agent the host connects to
//! that unix socket, issues `CONNECT <port>\n`, and then exchanges bounded,
//! versioned [`openoj_guest_protocol::Message`] frames over the bridged
//! connection. The guest agent itself listens on `AF_VSOCK` at that port.
//!
//! The judge node runs a single worker, so I/O here is blocking; the executor
//! invokes it from a blocking context. Reads carry a timeout so an unresponsive
//! guest converges to a deterministic failure instead of hanging the worker.

use std::io::{Error as IoError, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use openoj_guest_protocol::{CodecError, MAX_FRAME_BYTES, Message};

use crate::config::FirecrackerError;

/// Default guest read timeout; an unresponsive guest fails closed.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Per-attempt timeout while Firecracker waits for the guest vsock listener.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Total readiness budget after a microVM has started.
pub const DEFAULT_CONNECT_WAIT: Duration = Duration::from_secs(10);

/// Poll interval between guest readiness attempts.
pub const DEFAULT_CONNECT_POLL: Duration = Duration::from_millis(50);

/// Firecracker's success reply prefix to a `CONNECT` request.
const CONNECT_OK_PREFIX: &str = "OK ";

/// A connected, bounded host↔guest vsock channel bridged over a firecracker UDS.
pub struct GuestChannel {
    stream: UnixStream,
    read_timeout: Duration,
}

impl GuestChannel {
    /// Connects to the guest agent at `uds_path:port`.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError::Io`] when the unix connection, the `CONNECT`
    /// handshake, or the read-timeout configuration fails.
    pub fn connect(
        uds_path: &Path,
        port: u32,
        read_timeout: Duration,
    ) -> Result<Self, FirecrackerError> {
        let mut stream = UnixStream::connect(uds_path).map_err(|error| FirecrackerError::Io {
            message: format!("vsock unix connect failed: {error}"),
        })?;
        stream
            .set_read_timeout(Some(DEFAULT_CONNECT_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(DEFAULT_CONNECT_TIMEOUT)))
            .map_err(|error| FirecrackerError::Io {
                message: format!("set vsock CONNECT timeout failed: {error}"),
            })?;
        let handshake = format!("CONNECT {port}\n");
        stream
            .write_all(handshake.as_bytes())
            .map_err(|error| FirecrackerError::Io {
                message: format!("vsock CONNECT write failed: {error}"),
            })?;
        let mut line = String::new();
        read_line(&mut stream, &mut line).map_err(|error| FirecrackerError::Io {
            message: format!("vsock CONNECT reply failed: {error}"),
        })?;
        if !line.starts_with(CONNECT_OK_PREFIX) {
            return Err(FirecrackerError::Io {
                message: format!("vsock CONNECT rejected: {line}"),
            });
        }
        stream
            .set_read_timeout(Some(read_timeout))
            .and_then(|()| stream.set_write_timeout(Some(read_timeout)))
            .map_err(|error| FirecrackerError::Io {
                message: format!("set guest channel timeout failed: {error}"),
            })?;
        Ok(Self {
            stream,
            read_timeout,
        })
    }

    /// Connects within a bounded readiness window while the guest boots.
    pub(crate) fn connect_until_ready(
        uds_path: &Path,
        port: u32,
        read_timeout: Duration,
    ) -> Result<Self, FirecrackerError> {
        retry_until(DEFAULT_CONNECT_WAIT, DEFAULT_CONNECT_POLL, || {
            Self::connect(uds_path, port, read_timeout)
        })
    }

    /// Sends one bounded message frame to the guest.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError`] when encoding or the write fails.
    pub fn send(&mut self, message: &Message) -> Result<(), FirecrackerError> {
        let frame = message.encode()?;
        self.stream
            .write_all(&frame)
            .map_err(|error| FirecrackerError::Io {
                message: format!("vsock write failed: {error}"),
            })
    }

    /// Receives one bounded message frame from the guest.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError`] when the frame exceeds the bound, the read
    /// times out, or decoding fails.
    pub fn recv(&mut self) -> Result<Message, FirecrackerError> {
        let mut length_buf = [0u8; 4];
        self.stream
            .read_exact(&mut length_buf)
            .map_err(|error| FirecrackerError::Io {
                message: format!("vsock read failed: {error}"),
            })?;
        let length = read_length(length_buf)?;
        let mut frame = Vec::with_capacity(length + 4);
        frame.extend_from_slice(&length_buf);
        frame.resize(4 + length, 0);
        self.stream
            .read_exact(&mut frame[4..])
            .map_err(|error| FirecrackerError::Io {
                message: format!("vsock read body failed: {error}"),
            })?;
        Message::decode(&frame).map_err(|e| map_codec(&e))
    }

    /// Returns the configured read timeout.
    #[must_use]
    pub const fn read_timeout(&self) -> Duration {
        self.read_timeout
    }
}

fn retry_until<T>(
    wait: Duration,
    poll: Duration,
    mut operation: impl FnMut() -> Result<T, FirecrackerError>,
) -> Result<T, FirecrackerError> {
    let deadline = Instant::now() + wait;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(FirecrackerError::Io {
                        message: format!("guest channel readiness timed out: {error}"),
                    });
                }
                thread::sleep(poll.min(remaining));
            }
        }
    }
}

fn read_line<R: Read>(reader: &mut R, out: &mut String) -> Result<(), IoError> {
    let mut buf = [0u8; 1];
    let mut previous = b'\n';
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            return Ok(());
        }
        let byte = buf[0];
        if previous == b'\r' && byte == b'\n' {
            // Strip a trailing CRLF from the prior CR.
            out.pop();
        }
        previous = byte;
        if byte == b'\n' {
            return Ok(());
        }
        out.push(byte as char);
    }
}

fn read_length(length_buf: [u8; 4]) -> Result<usize, FirecrackerError> {
    let length = u32::from_be_bytes(length_buf) as usize;
    if length > MAX_FRAME_BYTES.saturating_sub(4) {
        return Err(FirecrackerError::Io {
            message: format!("guest frame length {length} exceeds bound {MAX_FRAME_BYTES}"),
        });
    }
    Ok(length)
}

fn map_codec(error: &CodecError) -> FirecrackerError {
    FirecrackerError::Io {
        message: format!("guest message decode failed: {error}"),
    }
}

/// Reads a bounded frame from any `Read` source; used by unit tests.
#[cfg(test)]
fn read_frame<R: Read>(reader: &mut R) -> Result<Message, FirecrackerError> {
    let mut length_buf = [0u8; 4];
    reader
        .read_exact(&mut length_buf)
        .map_err(|error| FirecrackerError::Io {
            message: error.to_string(),
        })?;
    let length = read_length(length_buf)?;
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&length_buf);
    frame.resize(4 + length, 0);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|error| FirecrackerError::Io {
            message: error.to_string(),
        })?;
    Message::decode(&frame).map_err(|e| map_codec(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openoj_guest_protocol::{GuestUsage, Stage};

    #[test]
    fn oversized_frame_length_is_rejected() {
        let frame = [0xffu8, 0xff, 0xff, 0xff];
        let err = read_length(frame);
        assert!(matches!(err, Err(FirecrackerError::Io { .. })));
    }

    #[test]
    fn guest_channel_roundtrip_over_loopback_io() -> Result<(), Box<dyn std::error::Error>> {
        let message = Message::Negotiate {
            capabilities: vec!["algorithm.batch".to_owned()],
        };
        let encoded = message.encode()?;
        let mut sink: &[u8] = &encoded;
        let decoded = read_frame(&mut sink)?;
        assert_eq!(decoded, message);
        Ok(())
    }

    #[test]
    fn stage_output_roundtrip_through_codec() -> Result<(), Box<dyn std::error::Error>> {
        let message = Message::StageOutput {
            stage: Stage::Build,
            exit_code: 0,
            output_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            output_bytes: 3,
            usage: GuestUsage::default(),
            diagnostics: Vec::new(),
        };
        let encoded = message.encode()?;
        let mut sink: &[u8] = &encoded;
        let decoded = read_frame(&mut sink)?;
        assert_eq!(decoded, message);
        Ok(())
    }

    #[test]
    fn truncated_frame_is_an_io_error() {
        let mut sink: &[u8] = &[0, 0, 0, 8, 1, 2];
        let result = read_frame(&mut sink);
        assert!(matches!(result, Err(FirecrackerError::Io { .. })));
    }

    #[test]
    fn connect_reply_line_is_parsed() -> Result<(), Box<dyn std::error::Error>> {
        // Simulate Firecracker's reply over an in-memory reader.
        let mut reply: &[u8] = b"OK 1073741824\n";
        let mut line = String::new();
        read_line(&mut reply, &mut line)?;
        assert!(line.starts_with(CONNECT_OK_PREFIX));
        Ok(())
    }

    #[test]
    fn guest_readiness_retries_transient_failures() -> Result<(), Box<dyn std::error::Error>> {
        let mut attempts = 0;
        let value = retry_until(Duration::from_secs(1), Duration::ZERO, || {
            attempts += 1;
            if attempts < 3 {
                return Err(FirecrackerError::Io {
                    message: "guest not ready".to_owned(),
                });
            }
            Ok(42)
        })?;
        assert_eq!(value, 42);
        assert_eq!(attempts, 3);
        Ok(())
    }

    #[test]
    fn guest_readiness_timeout_is_bounded() {
        let result = retry_until(Duration::ZERO, Duration::ZERO, || {
            Err::<(), _>(FirecrackerError::Io {
                message: "guest not ready".to_owned(),
            })
        });
        assert!(matches!(
            result,
            Err(FirecrackerError::Io { message }) if message.contains("readiness timed out")
        ));
    }
}
