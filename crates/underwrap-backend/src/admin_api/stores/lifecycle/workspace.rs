use std::io;
use std::path::Path;

pub(super) fn create_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        std::fs::DirBuilder::new().mode(0o700).create(path)?;
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)) {
            let _ = std::fs::remove_dir(path);
            return Err(error);
        }
        if let Err(error) = require_directory(path, true) {
            let _ = std::fs::remove_dir(path);
            return Err(error);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)?;
        require_directory(path, false)
    }
}

pub(super) fn require_directory(path: &Path, exact_private_mode: bool) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "workspace path is not a regular directory",
        ));
    }
    #[cfg(unix)]
    if exact_private_mode {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o700 {
            return Err(io::Error::other(
                "workspace path is not an exact mode-0700 directory",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = exact_private_mode;
    Ok(())
}

pub(super) fn require_absent(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "workspace destination is occupied",
        )),
        Err(error) => Err(error),
    }
}

pub(super) fn checked_rename(from: &Path, to: &Path) -> io::Result<()> {
    require_directory(from, false)?;
    require_absent(to)?;
    std::fs::rename(from, to)
}
