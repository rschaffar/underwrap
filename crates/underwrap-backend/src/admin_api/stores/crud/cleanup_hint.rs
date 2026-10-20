//! Literal pathname transport for operator-controlled cleanup, never execution.

use std::path::Path;

use underwrap_protocol::service::AdminStoreDeleteCleanupHint;

use crate::admin_api::AdminError;

pub(super) fn for_directory(dir: &Path) -> Result<AdminStoreDeleteCleanupHint, AdminError> {
    let invalid_path = || {
        AdminError::internal_with_code(
            "cleanup_path_unrepresentable",
            "could not prepare exact cleanup guidance; store registry record was not removed",
        )
    };
    if dir.as_os_str().is_empty() {
        return Err(invalid_path());
    }
    // Anchor relative records without normalizing their suffix. Canonicalizing
    // resolves symlinks and requires existence; even path::absolute removes `.`
    // components, changing e.g. `file/.` into a removable `file` operand.
    let absolute = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| invalid_path())?
            .join(dir)
    };
    let data_dir = absolute.to_str().ok_or_else(invalid_path)?;
    if data_dir.contains('\0') {
        return Err(invalid_path());
    }
    // POSIX single quotes inhibit every expansion. A literal quote ends the
    // quoted span, is emitted by an escaped quote, then reopens that span.
    let operand = data_dir.replace('\'', "'\\''");
    Ok(AdminStoreDeleteCleanupHint {
        data_dir: data_dir.to_owned(),
        command: format!("rm -rf -- '{operand}'"),
    })
}

#[cfg(test)]
mod tests;
