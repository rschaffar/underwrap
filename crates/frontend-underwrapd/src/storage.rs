//! Storage — delegates to underwrap-store backends.

pub use underwrap_store::{StorageBackend, open};

#[cfg(test)]
mod tests {
    use super::*;
    use underwrap_protocol::config::StorageConfig;

    #[test]
    fn reexported_open_creates_working_plain_backend() {
        let dir = tempfile::TempDir::new().unwrap();
        let backend = open(&StorageConfig::Plain {
            dir: dir.path().join("plain"),
        })
        .unwrap();
        backend.write("github/token", b"ghp_test").unwrap();
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
    }
}
