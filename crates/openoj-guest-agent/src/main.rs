//! Launch entrypoint for the in-guest command agent.
//!
//! Binds the guest vsock port, accepts the single host control connection, and
//! serves bounded [`openoj_guest_protocol::Message`] frames from the task work
//! directory.

use std::env;
use std::path::PathBuf;

use openoj_guest_agent::{serve_frame, DEFAULT_WORK_DIR};

/// Default vsock port the agent listens on (matches the firecracker crate).
pub const DEFAULT_GUEST_PORT: u32 = 8266;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("OPENOJ_GUEST_PORT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_GUEST_PORT);
    let work = env::var("OPENOJ_WORK_DIR")
        .map_or_else(|_| PathBuf::from(DEFAULT_WORK_DIR), PathBuf::from);
    std::fs::create_dir_all(&work)?;

    let listener = vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, port)?;
    eprintln!("openoj-guest-agent listening on vsock cid=any port={port}");

    // The judge node is single-concurrency; serve exactly one control session.
    let (mut stream, _addr) = listener.accept()?;
    while serve_frame(&mut stream, &work)? {}
    eprintln!("openoj-guest-agent session ended");
    Ok(())
}
