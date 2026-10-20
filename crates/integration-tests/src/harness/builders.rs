//! Builder helpers for common wrapper test configs.

use std::path::PathBuf;

use underwrap_protocol::config::{FilesUse, WrapperDef};

/// Select the effective (not real or supplementary) group used by the daemon
/// gate. An unresolved name is a fixture error, never another identity.
pub(crate) fn current_group_name() -> anyhow::Result<String> {
    // SAFETY: getegid takes no pointers and does not change process identity.
    let gid = unsafe { libc::getegid() };
    group_name_with(gid, |record, buffer, result| {
        // SAFETY: each lookup has its own writable record, initialized buffer
        // and result slot. getgrgid_r borrows them for this call only. They stay
        // alive and unmodified through validation and the owned name copy below;
        // no shared NSS record or borrowed name escapes, including on ERANGE.
        unsafe {
            libc::getgrgid_r(
                gid,
                record,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                result,
            )
        }
    })
}

const GROUP_BUFFER_START: usize = 16 * 1024;
const GROUP_BUFFER_MAX: usize = 1024 * 1024;

fn group_name_with(
    gid: libc::gid_t,
    mut lookup: impl FnMut(&mut libc::group, &mut [u8], &mut *mut libc::group) -> libc::c_int,
) -> anyhow::Result<String> {
    let mut size = GROUP_BUFFER_START;
    loop {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(size)
            .map_err(|_| anyhow::anyhow!("allocating private group lookup buffer"))?;
        buffer.resize(size, 0u8);
        // SAFETY: all-zero is a valid initial libc::group (integers and raw
        // nullable pointers). No fields are dereferenced before result checks.
        let mut record: libc::group = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        let code = lookup(&mut record, &mut buffer, &mut result);
        if code == libc::ERANGE {
            size = size
                .checked_mul(2)
                .filter(|next| *next <= GROUP_BUFFER_MAX)
                .ok_or_else(|| anyhow::anyhow!("group lookup exceeded private buffer limit"))?;
            continue;
        }
        anyhow::ensure!(
            code == 0,
            "effective group lookup failed with NSS code {code}"
        );
        anyhow::ensure!(!result.is_null(), "effective group has no NSS name");
        anyhow::ensure!(
            std::ptr::eq(result, &record),
            "unexpected group result pointer"
        );
        anyhow::ensure!(
            record.gr_gid == gid,
            "NSS returned a different effective group"
        );
        anyhow::ensure!(!record.gr_name.is_null(), "NSS returned a null group name");
        // Validate provenance/range without dereferencing the returned pointer.
        // Slice access and the NUL search stay inside the private initialized
        // buffer, unlike an unbounded CStr::from_ptr read. Copy before either
        // record or buffer can be dropped/reused by this or another lookup.
        let offset = (record.gr_name as usize)
            .checked_sub(buffer.as_ptr() as usize)
            .filter(|offset| *offset < buffer.len())
            .ok_or_else(|| anyhow::anyhow!("group name is outside private lookup buffer"))?;
        let name = &buffer[offset..];
        let end = name.iter().position(|byte| *byte == 0).ok_or_else(|| {
            anyhow::anyhow!("group name is not terminated in private lookup buffer")
        })?;
        anyhow::ensure!(end != 0, "NSS returned an empty group name");
        return String::from_utf8(name[..end].to_vec())
            .map_err(|_| anyhow::anyhow!("effective group name is not UTF-8"));
    }
}

/// Simple env-projecting wrapper def for testing.
pub fn wrapper(target: &str, resource_id: &str, _env_var: &str) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        resource: resource_id.to_string(),
        env: true,
        files: None,
        libsecret: false,
        env_passthrough: Vec::new(),
        env_set: std::collections::HashMap::new(),
        access_paths: Vec::new(),
    }
}

/// Wrapper def projecting a single resource into its files slot at
/// `path`.
pub fn files_wrapper(target: &str, resource_id: &str, path: &str) -> WrapperDef {
    files_wrapper_paths(target, resource_id, &[path])
}

/// Wrapper def projecting a single resource into its files slot at
/// multiple `paths`.
pub fn files_wrapper_paths(target: &str, resource_id: &str, paths: &[&str]) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        resource: resource_id.to_string(),
        env: false,
        files: Some(FilesUse {
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        }),
        libsecret: false,
        env_passthrough: Vec::new(),
        env_set: std::collections::HashMap::new(),
        access_paths: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
