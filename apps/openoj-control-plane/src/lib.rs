//! P0-C control-plane process configuration and UDS boundary checks.

use std::fmt::{self, Display, Formatter};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketPathError {
    RelativePath,
    InvalidParent,
}

impl Display for SocketPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RelativePath => "Judge Control socket path must be absolute",
            Self::InvalidParent => "Judge Control socket parent must be an existing directory",
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
    Ok(())
}
