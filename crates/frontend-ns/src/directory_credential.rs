use std::collections::BTreeSet;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::exec::ExecDirectoryBinding;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{DirectoryRequest, DirectoryResponse};

pub struct DirectoryMount {
    pub target_mount_path: PathBuf,
    pub created_mount_dirs: Vec<PathBuf>,
}

pub struct MountedDirectory {
    pub resource_id: String,
    pub channel_fd: i32,
    /// All bind-mount targets backed by `staging_path`. A single backend
    /// resource can be exposed at multiple namespace-local paths; they
    /// all share the same staging directory and channel, so writes are
    /// serialized through one writeback path.
    pub mounts: Vec<DirectoryMount>,
    pub staging_path: PathBuf,
    pub initial_files: BTreeSet<String>,
}

pub fn prepare_directory_mounts(
    bindings: &[ExecDirectoryBinding],
    target_home: &Path,
    staging_root: &Path,
) -> Result<Vec<MountedDirectory>, String> {
    let mut mounted = Vec::new();
    for (index, binding) in bindings.iter().enumerate() {
        if binding.target_paths.is_empty() {
            return Err(format!(
                "directory credential '{}' has no target paths",
                binding.resource_id
            ));
        }
        for target_path in &binding.target_paths {
            validate_relative_path(target_path)?;
        }

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

        // The staging tmpfs and every bind mount are always writable
        // for the target. The backend is the single source of truth
        // for write authorization: any modifications attempted on a
        // read-only grant are silently dropped at sync time (with a
        // journal + stderr warning, suppressible via
        // UNDERWRAP_SUPPRESS_READONLY_WARNING). Mounting the staging
        // dir read-only would surface EROFS to the target and break
        // tools that test-write before falling back.
        let mut mounts = Vec::with_capacity(binding.target_paths.len());
        for target_path in &binding.target_paths {
            let (target_mount_path, created_mount_dirs) =
                ensure_mount_target(target_home, target_path)?;
            bind_mount_dir(&staging_path, &target_mount_path, false)?;
            mounts.push(DirectoryMount {
                target_mount_path,
                created_mount_dirs,
            });
        }

        mounted.push(MountedDirectory {
            resource_id: binding.resource_id.clone(),
            channel_fd: binding.channel_fd as i32,
            mounts,
            staging_path,
            initial_files,
        });
    }
    Ok(mounted)
}

/// Outcome of one resource's end-of-run sync. Carried up so the
/// caller can emit a single user-facing warning per affected
/// resource (one journal event + one stderr line, suppressible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// All changes (if any) were accepted by the backend.
    Accepted,
    /// The target modified files but the backend rejected the
    /// writeback because the grant is not writable. Changes have
    /// been dropped.
    Rejected { resource_id: String },
}

pub fn sync_directory_mounts(directories: &[MountedDirectory]) -> Result<Vec<SyncOutcome>, String> {
    let mut outcomes = Vec::with_capacity(directories.len());
    for directory in directories {
        outcomes.push(sync_directory(directory)?);
    }
    Ok(outcomes)
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
        for mount in directory.mounts.iter().rev() {
            if let Err(e) = unmount_path(&mount.target_mount_path) {
                first_error.get_or_insert(e);
                continue;
            }
            for dir in mount.created_mount_dirs.iter().rev() {
                if let Err(e) = std::fs::remove_dir(dir)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    first_error
                        .get_or_insert(format!("removing mountpoint {}: {e}", dir.display()));
                }
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

    /// Returns `Ok(true)` if accepted, `Ok(false)` if the backend
    /// rejected the write because the grant is not writable. Other
    /// errors propagate.
    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<bool, String> {
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
            DirectoryResponse::WriteFileOk => Ok(true),
            DirectoryResponse::Error {
                code: underwrap_protocol::message::DirectoryErrorCode::Unauthorized,
                ..
            } => Ok(false),
            DirectoryResponse::Error { code, message } => Err(format!(
                "directory write error ({code:?}) for '{path}': {message}"
            )),
            other => Err(format!(
                "unexpected directory WriteFile response: {other:?}"
            )),
        }
    }

    /// Returns `Ok(Some(existed))` if accepted, `Ok(None)` if the
    /// backend rejected the delete because the grant is not writable.
    fn delete_file(&mut self, path: &str) -> Result<Option<bool>, String> {
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
            DirectoryResponse::DeleteFileOk { existed } => Ok(Some(existed)),
            DirectoryResponse::Error {
                code: underwrap_protocol::message::DirectoryErrorCode::Unauthorized,
                ..
            } => Ok(None),
            DirectoryResponse::Error { code, message } => Err(format!(
                "directory delete error ({code:?}) for '{path}': {message}"
            )),
            other => Err(format!(
                "unexpected directory DeleteFile response: {other:?}"
            )),
        }
    }
}

fn sync_directory(directory: &MountedDirectory) -> Result<SyncOutcome, String> {
    let mut channel = DirectoryChannel::from_fd_dup(directory.channel_fd)?;
    let final_files = collect_managed_files(&directory.staging_path)?;
    let mut rejected = false;

    for relative_path in &final_files {
        // Send every file. The staging dir was materialized from the
        // backend at namespace start, so unchanged files re-send the
        // same content (cheap; channel is local). Skipping unchanged
        // entries would require a content hash per entry, which is
        // story 410 follow-up work.
        let bytes = std::fs::read(directory.staging_path.join(relative_path)).map_err(|e| {
            format!(
                "reading staged file {} for sync of '{}': {e}",
                directory.staging_path.join(relative_path).display(),
                directory.resource_id,
            )
        })?;
        if !channel.write_file(relative_path, &bytes)? {
            rejected = true;
        }
    }

    for deleted in directory.initial_files.difference(&final_files) {
        if channel.delete_file(deleted)?.is_none() {
            rejected = true;
        }
    }

    // When the grant denies writes, every write/delete is rejected
    // and we treat the run as "modifications dropped". When the
    // grant allows writes, no individual call should report
    // Unauthorized (no per-file gating today).
    if rejected {
        Ok(SyncOutcome::Rejected {
            resource_id: directory.resource_id.clone(),
        })
    } else {
        Ok(SyncOutcome::Accepted)
    }
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
