use super::*;

fn test_backend() -> (tempfile::TempDir, EncryptedBackend) {
    let dir = tempfile::TempDir::new().unwrap();
    let key: [u8; 32] = rand::random();
    let backend = EncryptedBackend::create_with_key(&dir.path().join("secrets"), key).unwrap();
    (dir, backend)
}

#[test]
fn create_is_exclusive_and_open_existing_never_creates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("store");
    let backend = EncryptedBackend::create_with_key(&path, [7; 32]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert!(EncryptedBackend::create_with_key(&path, [8; 32]).is_err());
    drop(backend);
    assert!(EncryptedBackend::open_existing_with_key(&path, [7; 32]).is_ok());

    let missing = tmp.path().join("missing");
    assert!(EncryptedBackend::open_existing_with_key(&missing, [7; 32]).is_err());
    assert!(!missing.exists());

    let file = tmp.path().join("file");
    std::fs::write(&file, b"untouched").unwrap();
    assert!(EncryptedBackend::create_with_key(&file, [7; 32]).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"untouched");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&path, tmp.path().join("link")).unwrap();
        assert!(
            EncryptedBackend::open_existing_with_key(&tmp.path().join("link"), [7; 32]).is_err()
        );
    }
}

#[test]
fn nested_directories_and_ciphertext_files_are_private() {
    let (dir, backend) = test_backend();
    backend.write("resource/nested/value", b"private").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            dir.path().join("secrets"),
            dir.path().join("secrets/resource"),
            dir.path().join("secrets/resource/nested"),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(
            std::fs::metadata(dir.path().join("secrets/resource/nested/value"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn write_then_read() {
    let (_dir, b) = test_backend();
    b.write("secret/tok", b"encrypted_value").unwrap();
    assert_eq!(b.read("secret/tok").unwrap().unwrap(), b"encrypted_value");
}

#[test]
fn file_is_not_plaintext() {
    let (dir, b) = test_backend();
    b.write("tok", b"my_secret_data").unwrap();
    let raw = std::fs::read(dir.path().join("secrets/tok")).unwrap();
    let raw_str = String::from_utf8_lossy(&raw);
    assert!(
        !raw_str.contains("my_secret_data"),
        "plaintext found in encrypted file!"
    );
    assert!(raw.len() > "my_secret_data".len());
}

#[test]
fn overwrite() {
    let (_dir, b) = test_backend();
    b.write("tok", b"v1").unwrap();
    b.write("tok", b"v2").unwrap();
    assert_eq!(b.read("tok").unwrap().unwrap(), b"v2");
}

#[test]
fn empty_material() {
    let (_dir, b) = test_backend();
    b.write("empty", b"").unwrap();
    assert_eq!(b.read("empty").unwrap().unwrap(), b"");
}

#[test]
fn wrong_key_fails() {
    let (dir, b) = test_backend();
    b.write("tok", b"secret").unwrap();

    let other_key: [u8; 32] = rand::random();
    let b2 =
        EncryptedBackend::open_existing_with_key(&dir.path().join("secrets"), other_key).unwrap();
    assert!(b2.read("tok").is_err());
}

#[test]
fn large_secret() {
    let (_dir, b) = test_backend();
    let big = "x".repeat(32_000);
    b.write("big", big.as_bytes()).unwrap();
    assert_eq!(b.read("big").unwrap().unwrap(), big);
}

#[test]
fn list_and_delete() {
    let (_dir, b) = test_backend();
    b.write("a/item1", b"v1").unwrap();
    b.write("a/item2", b"v2").unwrap();
    b.write("b/item3", b"v3").unwrap();

    let mut ids = b.list("a").unwrap();
    ids.sort();
    assert_eq!(ids, vec!["a/item1", "a/item2"]);

    assert!(b.delete("a/item1").unwrap());
    assert!(!b.delete("nonexistent").unwrap());

    let ids = b.list("a").unwrap();
    assert_eq!(ids, vec!["a/item2"]);

    // Deleted item can't be read.
    assert!(b.read("a/item1").unwrap().is_none());
}
