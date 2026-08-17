//! P0-C control-plane process configuration and UDS boundary checks.

use std::fmt::{self, Display, Formatter};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketPathError {
    RelativePath,
    InvalidParent,
    InsecureParent,
    ExistingPath,
    CreateFailed,
}

impl Display for SocketPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RelativePath => "Judge Control socket path must be absolute",
            Self::InvalidParent => "Judge Control socket parent must be an existing directory",
            Self::InsecureParent => {
                "Judge Control socket parent must not be accessible by group or other users"
            }
            Self::ExistingPath => "Judge Control socket path must not already exist",
            Self::CreateFailed => "Judge Control socket could not be created",
        })
    }
}

impl std::error::Error for SocketPathError {}

/// Checks the non-destructive UDS parent preconditions before a listener is created.
///
/// # Errors
///
/// Returns [`SocketPathError`] when the path is relative or its parent is missing, a symlink, or
/// not a directory.
pub fn validate_socket_path(path: impl AsRef<Path>) -> Result<(), SocketPathError> {
    let path = path.as_ref();
    if !path.is_absolute() {
        return Err(SocketPathError::RelativePath);
    }
    let parent = path.parent().ok_or(SocketPathError::InvalidParent)?;
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| SocketPathError::InvalidParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SocketPathError::InvalidParent);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(SocketPathError::InsecureParent);
    }
    Ok(())
}

/// Creates the P0-C Unix Domain Socket listener with private filesystem permissions.
///
/// # Errors
///
/// Returns [`SocketPathError`] when preconditions fail, an existing object would be replaced, or
/// the socket cannot be created with mode `0600`.
pub async fn bind_socket(
    path: impl AsRef<Path>,
) -> Result<tokio::net::UnixListener, SocketPathError> {
    let path = path.as_ref();
    validate_socket_path(path)?;
    if path.exists() {
        return Err(SocketPathError::ExistingPath);
    }
    let listener =
        tokio::net::UnixListener::bind(path).map_err(|_| SocketPathError::CreateFailed)?;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|_| SocketPathError::CreateFailed)?;
    Ok(listener)
}
