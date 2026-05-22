use super::*;

fn test_backend() -> (tempfile::TempDir, PlainBackend) {
    let dir = tempfile::TempDir::new().unwrap();
    let backend = PlainBackend::open(dir.path()).unwrap();
    (dir, backend)
}

#[test]
fn read_nonexistent_returns_none() {
    let (_dir, b) = test_backend();
    assert!(b.read("nope").unwrap().is_none());
}

#[test]
fn write_then_read() {
    let (_dir, b) = test_backend();
    b.write("my/secret", b"hello").unwrap();
    assert_eq!(b.read("my/secret").unwrap().unwrap(), b"hello");
}

#[test]
fn overwrite() {
    let (_dir, b) = test_backend();
    b.write("tok", b"v1").unwrap();
    b.write("tok", b"v2").unwrap();
    assert_eq!(b.read("tok").unwrap().unwrap(), b"v2");
}

#[test]
fn deep_hierarchy() {
    let (_dir, b) = test_backend();
    b.write("a/b/c/d/e", b"deep").unwrap();
    assert_eq!(b.read("a/b/c/d/e").unwrap().unwrap(), b"deep");
}

#[test]
fn rejects_dotdot() {
    let (_dir, b) = test_backend();
    assert!(b.write("../etc/passwd", b"pwned").is_err());
    assert!(b.read("../etc/passwd").is_err());
}

#[test]
fn rejects_absolute_path() {
    let (_dir, b) = test_backend();
    assert!(b.write("/etc/shadow", b"pwned").is_err());
}

#[test]
fn rejects_dot_component() {
    let (_dir, b) = test_backend();
    assert!(b.write("./secret", b"data").is_err());
}

#[test]
fn rejects_empty_segments() {
    let (_dir, b) = test_backend();
    assert!(b.write("a//b", b"data").is_err());
}

#[test]
fn rejects_empty_id() {
    let (_dir, b) = test_backend();
    assert!(b.write("", b"data").is_err());
}

#[test]
fn rejects_null_bytes() {
    let (_dir, b) = test_backend();
    assert!(b.write("secret\0id", b"data").is_err());
}

#[test]
fn empty_material() {
    let (_dir, b) = test_backend();
    b.write("empty", b"").unwrap();
    assert_eq!(b.read("empty").unwrap().unwrap(), b"");
}

#[test]
fn atomic_write_leaves_no_temp_files() {
    let (dir, b) = test_backend();
    b.write("tok", b"val").unwrap();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.starts_with(".tmp."), "temp file left behind: {name}");
    }
}

#[test]
fn list_empty() {
    let (_dir, b) = test_backend();
    let ids = b.list("").unwrap();
    assert!(ids.is_empty());
}

#[test]
fn list_with_prefix() {
    let (_dir, b) = test_backend();
    b.write("a/b/item1", b"v1").unwrap();
    b.write("a/b/item2", b"v2").unwrap();
    b.write("a/c/item3", b"v3").unwrap();
    b.write("d/item4", b"v4").unwrap();

    let mut ids = b.list("a/b").unwrap();
    ids.sort();
    assert_eq!(ids, vec!["a/b/item1", "a/b/item2"]);

    let mut ids = b.list("a").unwrap();
    ids.sort();
    assert_eq!(ids, vec!["a/b/item1", "a/b/item2", "a/c/item3"]);

    let mut ids = b.list("").unwrap();
    ids.sort();
    assert_eq!(ids, vec!["a/b/item1", "a/b/item2", "a/c/item3", "d/item4"]);
}

#[test]
fn list_nonexistent_prefix() {
    let (_dir, b) = test_backend();
    let ids = b.list("nonexistent").unwrap();
    assert!(ids.is_empty());
}

#[test]
fn delete_existing() {
    let (_dir, b) = test_backend();
    b.write("tok", b"val").unwrap();
    assert!(b.delete("tok").unwrap());
    assert!(b.read("tok").unwrap().is_none());
}

#[test]
fn delete_nonexistent() {
    let (_dir, b) = test_backend();
    assert!(!b.delete("nope").unwrap());
}

#[test]
fn delete_cleans_empty_parents() {
    let (dir, b) = test_backend();
    b.write("a/b/c/tok", b"val").unwrap();
    assert!(b.delete("a/b/c/tok").unwrap());
    // Empty parent dirs should be cleaned up.
    assert!(!dir.path().join("a/b/c").exists());
    assert!(!dir.path().join("a/b").exists());
    assert!(!dir.path().join("a").exists());
}

#[test]
fn delete_preserves_sibling_entries() {
    let (_dir, b) = test_backend();
    b.write("a/b/item1", b"v1").unwrap();
    b.write("a/b/item2", b"v2").unwrap();
    assert!(b.delete("a/b/item1").unwrap());
    // item2 should still be there.
    assert_eq!(b.read("a/b/item2").unwrap().unwrap(), b"v2");
    // And listable.
    let ids = b.list("a/b").unwrap();
    assert_eq!(ids, vec!["a/b/item2"]);
}
