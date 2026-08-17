use std::error::Error;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};

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

#[tokio::test]
async fn creates_a_private_socket_in_a_private_existing_directory() -> Result<(), Box<dyn Error>> {
    let parent = std::env::temp_dir().join(format!("openoj-p0c-uds-{}", std::process::id()));
    fs::create_dir(&parent)?;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
    let path = parent.join("judge-control.sock");

    let listener = openoj_control_plane::bind_socket(&path).await?;

    let metadata = fs::metadata(&path)?;
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    drop(listener);
    fs::remove_file(&path)?;
    fs::remove_dir(parent)?;
    Ok(())
}
