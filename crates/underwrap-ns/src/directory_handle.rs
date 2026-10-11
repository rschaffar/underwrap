//! Owned directory authority for files projection. No operation reconstructs
//! an absolute staging pathname; every child lookup stays relative to a held fd.

use std::ffi::{CStr, CString, OsString};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

#[derive(Debug)]
pub(crate) struct Directory(File);

impl Directory {
    pub(crate) fn from_mount(mount: OwnedFd, attachment: &Self) -> Result<Self, String> {
        // Retain the actual fsmount fd through attachment: its final close
        // dissolves an anonymous mount even if normal directory fds still pin
        // its vfsmount. Once attached, closing it no longer dissolves the tree.
        move_mount(mount.as_raw_fd(), attachment.0.as_raw_fd())?;
        Ok(Self(open_at(
            mount.as_raw_fd(),
            c".",
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?))
    }

    #[cfg(test)]
    pub(crate) fn open_path(path: &Path) -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| format!("opening test directory: {e}"))?;
        Ok(Self(file))
    }

    pub(crate) fn open_target_home(path: &Path) -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        // O_PATH permits a handle even when the named mountpoint is mode 0000.
        // Descendants are resolved independently with O_NOFOLLOW below.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| format!("opening target home: {e}"))?;
        Ok(Self(file))
    }

    pub(crate) fn require_identity_mode(
        &self,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<(), String> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = self
            .0
            .metadata()
            .map_err(|e| format!("checking staging attachment authority: {e}"))?;
        if metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.permissions().mode() & 0o7777 != mode
        {
            return Err("staging attachment has unexpected ownership or permissions".into());
        }
        Ok(())
    }

    pub(crate) fn create_child(&self, name: &str, mode: u32) -> Result<Self, String> {
        let name = component(name)?;
        // SAFETY: self owns a live directory fd and `name` is one validated,
        // NUL-terminated component. The return is checked before opening it.
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), mode) } != 0 {
            return Err(format!(
                "creating projection directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        // This method is used only on the private, not-yet-exposed staging
        // filesystem. No caller/target can mutate this parent during setup.
        // Restore mode before opening, so even umask 0777 remains supported.
        self.set_child_mode(&name, mode)?;
        Ok(Self(open_at(
            self.0.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?))
    }

    pub(crate) fn target_child(&self, name: &str, mode: u32) -> Result<Self, String> {
        let name = component(name)?;
        let mut created = false;
        // SAFETY: valid owned directory and single-component CString. EEXIST
        // does not assert type; the O_PATH/O_DIRECTORY/O_NOFOLLOW open does.
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), mode) } == 0 {
            created = true;
        } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(format!(
                "creating target mountpoint: {}",
                std::io::Error::last_os_error()
            ));
        }
        if created {
            // Preserve exact modes despite umask. Unlike private staging, the
            // destination parent can be caller-owned: refuse symlinks and let
            // target-fsid DAC reject replaced objects we do not own. No secret
            // is placed in these bare attachment directories.
            self.set_child_mode(&name, mode)?;
        }
        let flags = libc::O_PATH | libc::O_DIRECTORY;
        Ok(Self(open_at(self.0.as_raw_fd(), &name, flags, 0)?))
    }

    pub(crate) fn open_child(&self, name: &str) -> Result<Self, String> {
        let name = component(name)?;
        Ok(Self(open_at(
            self.0.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?))
    }

    pub(crate) fn create_file(&self, name: &str, content: &[u8]) -> Result<(), String> {
        let name = component(name)?;
        let mut file = open_at(
            self.0.as_raw_fd(),
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        file.write_all(content)
            .map_err(|e| format!("writing projection file: {e}"))?;
        set_file_mode(&file, 0o600)
    }

    pub(crate) fn open_entry(&self, name: &str) -> Result<File, String> {
        let name = component(name)?;
        // NONBLOCK avoids blocking on a planted FIFO before metadata rejection.
        open_at(
            self.0.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )
    }

    pub(crate) fn from_file(file: File) -> Self {
        Self(file)
    }

    pub(crate) fn entries(&self) -> Result<Vec<OsString>, String> {
        // A fresh open description avoids shared readdir offsets across fork or
        // repeated collections; dup alone would share the directory position.
        let file = open_at(
            self.0.as_raw_fd(),
            c".",
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        let fd = file.into_raw_fd();
        // SAFETY: fd is a newly owned readable directory descriptor. fdopendir
        // owns it only on success, in which case DirectoryStream closes it.
        let raw = unsafe { libc::fdopendir(fd) };
        let Some(raw) = std::ptr::NonNull::new(raw) else {
            let error = std::io::Error::last_os_error();
            // SAFETY: failed fdopendir did not take ownership of fd.
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
            return Err(format!("opening projection directory stream: {error}"));
        };
        let stream = DirectoryStream(raw);
        let mut entries = Vec::new();
        loop {
            // SAFETY: errno is thread-local; clearing it distinguishes EOF from
            // error. stream remains live and this function alone uses readdir.
            unsafe { *libc::__errno_location() = 0 };
            let entry = unsafe { libc::readdir(stream.0.as_ptr()) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(format!("reading projection directory: {error}"));
                }
                break;
            }
            // SAFETY: readdir returned a live dirent with a NUL-terminated name;
            // copy it before the next call can overwrite the internal storage.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                entries.push(OsString::from_vec(name.to_vec()));
            }
        }
        Ok(entries)
    }

    pub(crate) fn bind_to(&self, target: &Self) -> Result<(), String> {
        // Clone precisely the directory object held by this handle, not a path
        // which could have been renamed/replaced since materialization.
        let flags = libc::OPEN_TREE_CLONE | libc::O_CLOEXEC as u32 | libc::AT_EMPTY_PATH as u32;
        // SAFETY: the source fd is live; the empty C string and trusted flags
        // select this exact object. A successful syscall returns one owned fd.
        let fd =
            unsafe { libc::syscall(libc::SYS_open_tree, self.0.as_raw_fd(), c"".as_ptr(), flags) };
        if fd < 0 {
            return Err(format!(
                "cloning projection bind mount: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: successful open_tree returned a fresh uniquely owned fd.
        let tree = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        move_mount(tree.as_raw_fd(), target.0.as_raw_fd())
    }

    fn set_child_mode(&self, name: &CStr, mode: u32) -> Result<(), String> {
        // fchmodat2 with NOFOLLOW is supported at the Linux 6.5 frontend floor;
        // its newer AT_EMPTY_PATH extension is deliberately not required.
        // SAFETY: live parent fd and single-component C string; NOFOLLOW
        // prevents a planted symlink from selecting another object. The return
        // is checked before opening the child or performing any bind.
        let result = unsafe {
            libc::syscall(
                libc::SYS_fchmodat2,
                self.0.as_raw_fd(),
                name.as_ptr(),
                mode,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            return Err(format!(
                "setting projection directory permissions: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

fn move_mount(source: i32, target: i32) -> Result<(), String> {
    // MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH, linux/mount.h.
    const EMPTY_PATHS: u32 = 0x4 | 0x40;
    // SAFETY: both descriptors are borrowed from live owners; static empty
    // strings and flags select the held mount root and attachment object.
    let result = unsafe {
        libc::syscall(
            libc::SYS_move_mount,
            source,
            c"".as_ptr(),
            target,
            c"".as_ptr(),
            EMPTY_PATHS,
        )
    };
    if result != 0 {
        return Err(format!(
            "attaching projection mount: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn component(name: &str) -> Result<CString, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err("projection lookup requires one clean path component".into());
    }
    CString::new(name).map_err(|_| "projection component contains NUL".into())
}

fn open_at(fd: i32, name: &CStr, flags: i32, mode: u32) -> Result<File, String> {
    // SAFETY: fd is borrowed from a live directory owner; name is a live CString
    // or static C string. NOFOLLOW rejects symlinks at every traversed component.
    let opened = unsafe {
        libc::openat(
            fd,
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if opened < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) {
            return Err(format!(
                "projection entry is a symlink or not a directory: {error}"
            ));
        }
        return Err(format!("opening projection entry: {error}"));
    }
    // SAFETY: openat returned one fresh fd, adopted exactly once by File.
    Ok(unsafe { File::from_raw_fd(opened) })
}

fn set_file_mode(file: &File, mode: u32) -> Result<(), String> {
    // SAFETY: file owns the live fd; fchmod affects the held object, not a name.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode) } != 0 {
        return Err(format!(
            "setting projection permissions: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

struct DirectoryStream(std::ptr::NonNull<libc::DIR>);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this owner exclusively owns the successful fdopendir result.
        // closedir consumes its descriptor once; failure is not retried.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests;
