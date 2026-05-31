use super::*;

fn test_backend() -> (tempfile::TempDir, EncryptedBackend) {
    let dir = tempfile::TempDir::new().unwrap();
    let key: [u8; 32] = rand::random();
    let backend = EncryptedBackend::open_with_key(&dir.path().join("secrets"), key).unwrap();
    (dir, backend)
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
    let b2 = EncryptedBackend::open_with_key(&dir.path().join("secrets"), other_key).unwrap();
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
