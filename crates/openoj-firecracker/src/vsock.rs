//! Host-side `AF_VSOCK` channel to the guest agent.
//!
//! The host connects to the guest context identifier on the configured port and
//! exchanges bounded, versioned [`openoj_guest_protocol::Message`] frames. I/O
//! is blocking because the judge node runs a single worker; the executor calls
//! this from a blocking context. Reads carry a timeout so an unresponsive guest
//! converges to a deterministic failure instead of hanging the worker.

use std::io::{Read, Write};
use std::time::Duration;

use openoj_guest_protocol::{CodecError, MAX_FRAME_BYTES, Message};

use crate::config::FirecrackerError;

/// Default guest read timeout; an unresponsive guest fails closed.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// A connected, bounded host↔guest vsock channel.
pub struct GuestChannel {
    stream: vsock::VsockStream,
    read_timeout: Duration,
}

impl GuestChannel {
    /// Connects to the guest agent at `cid:port`.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError::Io`] when the `AF_VSOCK` connection or timeout
    /// configuration fails.
    pub fn connect(cid: u32, port: u32, read_timeout: Duration) -> Result<Self, FirecrackerError> {
        let stream = vsock::VsockStream::connect_with_cid_port(cid, port)
            .map_err(|error| FirecrackerError::Io {
                message: format!("vsock connect to {cid}:{port} failed: {error}"),
            })?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(|error| FirecrackerError::Io {
                message: format!("set read timeout failed: {error}"),
            })?;
        Ok(Self {
            stream,
            read_timeout,
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

impl Drop for GuestChannel {
    fn drop(&mut self) {
        let _ = self.stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openoj_guest_protocol::Stage;

    #[test]
    fn oversized_frame_length_is_rejected() {
        let frame = [0xffu8, 0xff, 0xff, 0xff];
        let err = read_length(frame);
        assert!(matches!(err, Err(FirecrackerError::Io { .. })));
    }

    #[test]
    fn guest_channel_roundtrip_over_loopback_io() -> Result<(), Box<dyn std::error::Error>> {
        // Drive send/recv through an in-memory pipe using the frame helpers plus
        // the ordinary message codec to prove framing on both sides.
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
            output_digest: "sha256:o".to_owned(),
            output_bytes: 3,
            usage: openoj_guest_protocol::GuestUsage::default(),
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
}
