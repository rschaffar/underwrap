use std::collections::BTreeSet;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::ExecDirectoryBinding;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{DirectoryRequest, DirectoryResponse};

pub struct MountedDirectory {
    pub resource_id: String,
    pub access: AccessMode,
    pub channel_fd: i32,
    pub target_mount_path: PathBuf,
    pub staging_path: PathBuf,
    pub created_mount_dirs: Vec<PathBuf>,
    pub initial_files: BTreeSet<String>,
}

pub fn prepare_directory_mounts(
    bindings: &[ExecDirectoryBinding],
    target_home: &Path,
    staging_root: &Path,
) -> Result<Vec<MountedDirectory>, String> {
    let mut mounted = Vec::new();
    for (index, binding) in bindings.iter().enumerate() {
        validate_relative_path(&binding.target_path)?;
        let (target_mount_path, created_mount_dirs) =
            ensure_mount_target(target_home, &binding.target_path)?;

        let staging_path = staging_root.join(format!("dir-{index}"));
        std::fs::create_dir_all(&staging_path)
            .map_err(|e| format!("creating staging directory {}: {e}", staging_path.display()))?;
        std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("setting permissions on {}: {e}", staging_path.display()))?;

        let mut channel = DirectoryChannel::from_fd_dup(binding.channel_fd as i32)?;
        let paths = channel.list_files()?;
        let mut initial_files = BTreeSet::new();
        for relative_path in paths {
            validate_relative_path(&relative_path)?;
            initial_files.insert(relative_path.clone());
            let content = channel.read_file(&relative_path)?.ok_or_else(|| {
                format!(
                    "directory credential '{}' listed file '{}' but read returned no content",
                    binding.resource_id, relative_path
                )
            })?;
            materialize_file(&staging_path, &relative_path, &content)?;
        }

        if binding.access == AccessMode::ReadOnly {
            bind_mount_readonly_to_self(&staging_path)?;
        }
        bind_mount_dir(
            &staging_path,
            &target_mount_path,
            binding.access == AccessMode::ReadOnly,
        )?;

        mounted.push(MountedDirectory {
            resource_id: binding.resource_id.clone(),
            access: binding.access,
            channel_fd: binding.channel_fd as i32,
            target_mount_path,
            staging_path,
            created_mount_dirs,
            initial_files,
        });
    }
    Ok(mounted)
}

pub fn sync_directory_mounts(directories: &[MountedDirectory]) -> Result<(), String> {
    for directory in directories {
        if directory.access == AccessMode::ReadOnly {
            continue;
        }
        sync_directory(directory)?;
    }
    Ok(())
}

pub fn close_directory_channel_fds_for_exec(directories: &[MountedDirectory]) {
    for directory in directories {
        unsafe {
            libc::close(directory.channel_fd);
        }
    }
}

pub fn cleanup_directory_mounts(directories: &[MountedDirectory]) -> Result<(), String> {
    let mut first_error = None;
    for directory in directories.iter().rev() {
        if let Err(e) = unmount_path(&directory.target_mount_path) {
            first_error.get_or_insert(e);
            continue;
        }
        for dir in directory.created_mount_dirs.iter().rev() {
            if let Err(e) = std::fs::remove_dir(dir)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                first_error.get_or_insert(format!("removing mountpoint {}: {e}", dir.display()));
            }
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

struct DirectoryChannel {
    stream: std::os::unix::net::UnixStream,
}

impl DirectoryChannel {
    fn from_fd_dup(fd: i32) -> Result<Self, String> {
        let dup_fd = unsafe { libc::dup(fd) };
        if dup_fd < 0 {
            return Err(format!(
                "dup({fd}) for directory channel failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(dup_fd) };
        Ok(Self { stream })
    }

    fn list_files(&mut self) -> Result<Vec<String>, String> {
        write_frame_sync(&mut self.stream, &DirectoryRequest::ListFiles)
            .map_err(|e| format!("sending directory ListFiles request: {e}"))?;
        let response: DirectoryResponse = read_frame_sync(&mut self.stream)
            .map_err(|e| format!("reading directory ListFiles response: {e}"))?;
        match response {
            DirectoryResponse::ListFilesOk { paths } => Ok(paths),
            DirectoryResponse::Error { code, message } => {
                Err(format!("directory list error ({code:?}): {message}"))
            }
            other => Err(format!(
                "unexpected directory ListFiles response: {other:?}"
            )),
        }
    }

    fn read_file(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        write_frame_sync(
            &mut self.stream,
            &DirectoryRequest::ReadFile {
                path: path.to_string(),
            },
        )
        .map_err(|e| format!("sending directory ReadFile request for '{path}': {e}"))?;
        let response: DirectoryResponse = read_frame_sync(&mut self.stream)
            .map_err(|e| format!("reading directory ReadFile response for '{path}': {e}"))?;
        match response {
            DirectoryResponse::ReadFileOk { content } => Ok(content.map(Into::into)),
            DirectoryResponse::Error { code, message } => Err(format!(
                "directory read error ({code:?}) for '{path}': {message}"
            )),
            other => Err(format!("unexpected directory ReadFile response: {other:?}")),
        }
    }

    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), String> {
        write_frame_sync(
            &mut self.stream,
            &DirectoryRequest::WriteFile {
                path: path.to_string(),
                content: content.into(),
            },
        )
        .map_err(|e| format!("sending directory WriteFile request for '{path}': {e}"))?;
        let response: DirectoryResponse = read_frame_sync(&mut self.stream)
            .map_err(|e| format!("reading directory WriteFile response for '{path}': {e}"))?;
        match response {
            DirectoryResponse::WriteFileOk => Ok(()),
            DirectoryResponse::Error { code, message } => Err(format!(
                "directory write error ({code:?}) for '{path}': {message}"
            )),
            other => Err(format!(
                "unexpected directory WriteFile response: {other:?}"
            )),
        }
    }

    fn delete_file(&mut self, path: &str) -> Result<bool, String> {
        write_frame_sync(
            &mut self.stream,
            &DirectoryRequest::DeleteFile {
                path: path.to_string(),
            },
        )
        .map_err(|e| format!("sending directory DeleteFile request for '{path}': {e}"))?;
        let response: DirectoryResponse = read_frame_sync(&mut self.stream)
            .map_err(|e| format!("reading directory DeleteFile response for '{path}': {e}"))?;
        match response {
            DirectoryResponse::DeleteFileOk { existed } => Ok(existed),
            DirectoryResponse::Error { code, message } => Err(format!(
                "directory delete error ({code:?}) for '{path}': {message}"
            )),
            other => Err(format!(
                "unexpected directory DeleteFile response: {other:?}"
            )),
        }
    }
}

fn sync_directory(directory: &MountedDirectory) -> Result<(), String> {
    let mut channel = DirectoryChannel::from_fd_dup(directory.channel_fd)?;
    let final_files = collect_managed_files(&directory.staging_path)?;

    for relative_path in &final_files {
        let bytes = std::fs::read(directory.staging_path.join(relative_path)).map_err(|e| {
            format!(
                "reading staged file {} for sync of '{}': {e}",
                directory.staging_path.join(relative_path).display(),
                directory.resource_id,
            )
        })?;
        channel.write_file(relative_path, &bytes)?;
    }

    for deleted in directory.initial_files.difference(&final_files) {
        channel.delete_file(deleted)?;
    }

    Ok(())
}

fn collect_managed_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    collect_files_recursive(root, root, &mut out)?;
    Ok(out)
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
    out: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current)
        .map_err(|e| format!("listing directory {}: {e}", current.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading directory entry: {e}"))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(format!(
                "managed directory '{}' contains a symlink at {}",
                root.display(),
                path.display()
            ));
        }
        if file_type.is_dir() {
            collect_files_recursive(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("stripping prefix from {}: {e}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("path {} is not valid UTF-8", path.display()))?
                .to_string();
            validate_relative_path(&relative)?;
            out.insert(relative);
        } else {
            return Err(format!(
                "managed directory '{}' contains unsupported entry type at {}",
                root.display(),
                path.display()
            ));
        }
    }
    Ok(())
}

fn materialize_file(
    staging_root: &Path,
    relative_path: &str,
    content: &[u8],
) -> Result<(), String> {
    let target = staging_root.join(relative_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating staging directory {}: {e}", parent.display()))?;
        set_mode(parent, 0o700)?;
    }
    std::fs::write(&target, content)
        .map_err(|e| format!("writing staged file {}: {e}", target.display()))?;
    set_mode(&target, 0o600)?;
    Ok(())
}

fn ensure_mount_target(
    target_home: &Path,
    relative_path: &str,
) -> Result<(PathBuf, Vec<PathBuf>), String> {
    validate_relative_path(relative_path)?;
    let mut current = target_home.to_path_buf();
    let mut created = Vec::new();
    let components: Vec<&str> = relative_path.split('/').collect();

    for (index, component) in components.iter().enumerate() {
        let next = current.join(component);
        match std::fs::symlink_metadata(&next) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "target path {} contains a symlink component",
                        next.display()
                    ));
                }
                if !metadata.is_dir() {
                    return Err(format!(
                        "target path {} exists but is not a directory",
                        next.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&next)
                    .map_err(|err| format!("creating mountpoint {}: {err}", next.display()))?;
                let is_final = index + 1 == components.len();
                set_mode(&next, if is_final { 0o000 } else { 0o700 })?;
                created.push(next.clone());
            }
            Err(e) => {
                return Err(format!("stat {}: {e}", next.display()));
            }
        }
        current = next;
    }

    Ok((current, created))
}

fn bind_mount_readonly_to_self(path: &Path) -> Result<(), String> {
    let path = path_to_cstring(path)?;
    let ret = unsafe {
        libc::mount(
            path.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        return Err(format!(
            "self bind-mounting {} failed: {}",
            path.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    let ret = unsafe {
        libc::mount(
            std::ptr::null(),
            path.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        return Err(format!(
            "remounting {} read-only failed: {}",
            path.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn bind_mount_dir(source: &Path, target: &Path, readonly: bool) -> Result<(), String> {
    let source = path_to_cstring(source)?;
    let target = path_to_cstring(target)?;
    let ret = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        return Err(format!(
            "bind-mounting {} -> {} failed: {}",
            source.to_string_lossy(),
            target.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }

    if readonly {
        let ret = unsafe {
            libc::mount(
                std::ptr::null(),
                target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
                std::ptr::null(),
            )
        };
        if ret != 0 {
            return Err(format!(
                "remounting {} read-only failed: {}",
                target.to_string_lossy(),
                std::io::Error::last_os_error()
            ));
        }
    }

    Ok(())
}

fn unmount_path(path: &Path) -> Result<(), String> {
    let path = path_to_cstring(path)?;
    let ret = unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) };
    if ret != 0 {
        return Err(format!(
            "unmounting {} failed: {}",
            path.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("setting permissions on {}: {e}", path.display()))
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if path.starts_with('/') {
        return Err("path must be relative".to_string());
    }
    if path.contains('\0') {
        return Err("path must not contain null bytes".to_string());
    }
    for component in path.split('/') {
        if component.is_empty() {
            return Err(format!("path '{path}' contains an empty segment"));
        }
        if component == "." || component == ".." {
            return Err(format!("path '{path}' must be clean"));
        }
    }
    Ok(())
}

fn path_to_cstring(path: &Path) -> Result<std::ffi::CString, String> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("path '{}' contains a null byte", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_relative_path_rejects_bad_inputs() {
        for path in ["", "/abs", "a//b", "a/./b", "a/../b", "a\0b"] {
            assert!(
                validate_relative_path(path).is_err(),
                "path {path:?} should fail"
            );
        }
        assert!(validate_relative_path(".config/gws").is_ok());
    }
}
