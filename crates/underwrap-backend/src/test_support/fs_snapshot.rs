use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct TreeEntry {
    pub(crate) path: PathBuf,
    pub(crate) kind: &'static str,
    pub(crate) mode: u32,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn tree_snapshot(root: &Path) -> Vec<TreeEntry> {
    fn visit(root: &Path, path: &Path, out: &mut Vec<TreeEntry>) {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        };
        #[cfg(not(unix))]
        let mode = 0;
        let kind = if metadata.file_type().is_symlink() {
            "symlink"
        } else if metadata.is_dir() {
            "directory"
        } else {
            "file"
        };
        let bytes = if metadata.file_type().is_symlink() {
            std::fs::read_link(path)
                .unwrap()
                .as_os_str()
                .to_string_lossy()
                .as_bytes()
                .to_vec()
        } else if metadata.is_file() {
            std::fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        out.push(TreeEntry {
            path: path.strip_prefix(root).unwrap().to_path_buf(),
            kind,
            mode,
            bytes,
        });
        if metadata.is_dir() {
            for child in std::fs::read_dir(path).unwrap() {
                visit(root, &child.unwrap().path(), out);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

pub(crate) fn assert_secure_encrypted_tree(root: &Path) {
    #[cfg(unix)]
    for entry in tree_snapshot(root) {
        match entry.kind {
            "directory" => assert_eq!(entry.mode, 0o700, "directory {:?}", entry.path),
            "file" => assert_eq!(entry.mode, 0o600, "file {:?}", entry.path),
            _ => panic!("unexpected object type in encrypted tree: {entry:?}"),
        }
    }
}
