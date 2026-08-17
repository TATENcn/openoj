use std::error::Error;
use std::fs;

use openoj_control_plane::{SocketPathError, validate_socket_path};

#[test]
fn rejects_a_relative_socket_path() {
    assert_eq!(
        validate_socket_path("judge-control.sock"),
        Err(SocketPathError::RelativePath)
    );
}

#[test]
fn rejects_a_socket_parent_that_is_a_regular_file() -> Result<(), Box<dyn Error>> {
    let temporary = std::env::temp_dir().join("openoj-p0c-socket-parent-file");
    fs::write(&temporary, b"not a directory")?;
    let path = temporary.join("judge-control.sock");

    assert_eq!(
        validate_socket_path(&path),
        Err(SocketPathError::InvalidParent)
    );
    fs::remove_file(temporary)?;
    Ok(())
}
