//! Minimal bounded HTTP/1.1 client over a Unix socket for the Firecracker API.
//!
//! Firecracker exposes its control plane as JSON-over-HTTP on a Unix domain
//! socket. This module implements exactly the small request surface the P0
//! slice needs, with bounded bodies and read timeouts. It deliberately does not
//! depend on a general HTTP client.

use std::io::{Error as IoError, ErrorKind};
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};

use crate::config::FirecrackerError;

/// Maximum accepted control API response body, in bytes.
pub const MAX_API_BODY: usize = 65_536;

/// Default control API request/response timeout.
pub const API_TIMEOUT: Duration = Duration::from_secs(10);

/// A parsed HTTP response from the Firecracker control API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiResponse {
    status: u16,
    body: Vec<u8>,
}

impl ApiResponse {
    /// The HTTP status line code (successful requests usually return 204).
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// The response body (bounded to `MAX_API_BODY`).
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

/// Sends one HTTP request over a Unix socket and parses the bounded response.
///
/// # Errors
///
/// Returns [`FirecrackerError::Io`] on socket, timeout, or malformed response
/// conditions, and a non-2xx status is surfaced as [`FirecrackerError::ControlApi`].
pub async fn request(
    socket_path: &Path,
    method: &str,
    target: &str,
    content_type: &str,
    body: Option<&[u8]>,
) -> Result<ApiResponse, FirecrackerError> {
    let mut stream = timeout(API_TIMEOUT, UnixStream::connect(socket_path))
        .await
        .map_err(|_| io("connect timed out"))?
        .map_err(|error| io(&format!("connect failed: {error}")))?;

    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: firecracker\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| io(&format!("write request failed: {error}")))?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .await
            .map_err(|error| io(&format!("write body failed: {error}")))?;
    }

    let mut reader = BoundedReader::new(MAX_API_BODY);
    timeout(API_TIMEOUT, read_response(&mut stream, &mut reader))
        .await
        .map_err(|_| io("read response timed out"))?
        .map_err(FirecrackerError::from)?;

    let (status, body) = reader.finish();
    if !(200..300).contains(&status) {
        return Err(FirecrackerError::ControlApi {
            status,
            detail: String::from_utf8_lossy(&body).into_owned(),
        });
    }
    Ok(ApiResponse { status, body })
}

fn io(message: &str) -> FirecrackerError {
    FirecrackerError::Io {
        message: message.to_owned(),
    }
}

/// Collects the raw bytes of an HTTP response line, headers, and body.
struct BoundedReader {
    buffer: Vec<u8>,
    maximum: usize,
}

impl BoundedReader {
    fn new(maximum: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(maximum),
            maximum,
        }
    }

    fn push(&mut self, byte: u8) -> Result<(), IoError> {
        if self.buffer.len() >= self.maximum {
            return Err(IoError::new(ErrorKind::InvalidData, "response exceeds bound"));
        }
        self.buffer.push(byte);
        Ok(())
    }

    fn finish(self) -> (u16, Vec<u8>) {
        let text = self.buffer;
        let index = text
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap_or(text.len());
        let head = String::from_utf8_lossy(&text[..index]);
        let body = text[index.saturating_add(4)..].to_vec();
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or(0);
        (status, body)
    }
}

async fn read_response<R: AsyncReadExt + Unpin>(reader: &mut R, out: &mut BoundedReader) -> Result<(), IoError> {
    let mut header_done = false;
    let mut body_remaining: Option<usize> = None;
    let mut byte = [0u8; 1];

    // Read until headers complete.
    while !header_done {
        let buffer = &mut byte;
        if reader.read_exact(buffer).await? == 0 {
            break;
        }
        out.push(byte[0])?;
        header_done = out.buffer.windows(4).any(|window| window == b"\r\n\r\n");
        if out.buffer.len() > 16 * 1024 {
            break;
        }
    }
    if !header_done {
        return Err(IoError::new(ErrorKind::InvalidData, "headers not terminated"));
    }

    // Determine content length from headers.
    let head = String::from_utf8_lossy(&out.buffer);
    let content_length = head.lines().find_map(|line| {
        let separator = line.find(':')?;
        if !line[..separator].eq_ignore_ascii_case("content-length") {
            return None;
        }
        line[separator + 1..].trim().parse::<usize>().ok()
    });
    if let Some(length) = content_length {
        if length > out.maximum.saturating_sub(out.buffer.len()) {
            return Err(IoError::new(ErrorKind::InvalidData, "content-length exceeds bound"));
        }
        body_remaining = Some(length);
    }

    while let Some(mut remaining) = body_remaining {
        if remaining == 0 {
            break;
        }
        let chunk = &mut byte;
        if reader.read_exact(chunk).await? == 0 {
            break;
        }
        out.push(byte[0])?;
        remaining -= 1;
        body_remaining = Some(remaining);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(reader: &mut BoundedReader, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        for byte in bytes {
            reader.push(*byte)?;
        }
        Ok(())
    }

    #[test]
    fn bounds_reader_parses_status_and_body() -> Result<(), Box<dyn std::error::Error>> {
        let mut reader = BoundedReader::new(MAX_API_BODY);
        feed(
            &mut reader,
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n",
        )?;
        let (status, body) = reader.finish();
        assert_eq!(status, 204);
        assert!(body.is_empty());
        Ok(())
    }

    #[test]
    fn bounds_reader_parses_json_body() -> Result<(), Box<dyn std::error::Error>> {
        let mut reader = BoundedReader::new(MAX_API_BODY);
        feed(
            &mut reader,
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 5\r\n\r\nhello",
        )?;
        let (status, body) = reader.finish();
        assert_eq!(status, 400);
        assert_eq!(body, b"hello");
        Ok(())
    }

    #[test]
    fn oversize_declared_length_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut reader = BoundedReader::new(8);
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n";
        let mut stream = &response[..];
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;
        let result = runtime.block_on(read_response(&mut stream, &mut reader));
        assert!(result.is_err());
        Ok(())
    }
}
