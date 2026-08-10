use super::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::{
    Collection, Item, LibsecretSection, MAX_CANONICAL_RESOURCE_BYTES,
};

/// Minimal in-memory backend, just for these helpers.
struct MemStore {
    data: Mutex<HashMap<String, Vec<u8>>>,
    writes: AtomicUsize,
}

impl MemStore {
    fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            writes: AtomicUsize::new(0),
        }
    }
}

impl StorageBackend for MemStore {
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .get(secret_id)
            .cloned()
            .map(SecretBytes::from))
    }
    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.data
            .lock()
            .unwrap()
            .insert(secret_id.into(), material.to_vec());
        Ok(())
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
    fn delete(&self, secret_id: &str) -> Result<bool> {
        Ok(self.data.lock().unwrap().remove(secret_id).is_some())
    }
}

#[test]
fn load_returns_empty_blob_when_missing() {
    let store = MemStore::new();
    let blob = load_resource_blob(&store, "github/token").unwrap();
    assert_eq!(blob, ResourceBlob::empty());
}

#[test]
fn save_then_load_round_trips() {
    let store = MemStore::new();
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
    save_resource_blob(&store, "github/token", &blob).unwrap();
    let loaded = load_resource_blob(&store, "github/token").unwrap();
    assert_eq!(loaded, blob);
}

#[test]
fn save_uses_resources_key_shape() {
    let store = MemStore::new();
    save_resource_blob(&store, "github/token", &ResourceBlob::empty()).unwrap();
    assert!(
        store
            .data
            .lock()
            .unwrap()
            .contains_key("resources/github/token"),
        "expected key 'resources/github/token', got {:?}",
        store.data.lock().unwrap().keys().collect::<Vec<_>>()
    );
}

#[test]
fn delete_returns_existed_flag() {
    let store = MemStore::new();
    save_resource_blob(&store, "r", &ResourceBlob::empty()).unwrap();
    assert!(delete_resource_blob(&store, "r").unwrap());
    assert!(!delete_resource_blob(&store, "r").unwrap());
}

fn sample_blob(env_value: &str) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env.insert("TOK".into(), SecretBytes::from(env_value));
    blob
}

fn blob_with_canonical_len(len: usize) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.files
        .insert("padding".into(), SecretBytes::from(Vec::<u8>::new()));
    let base = blob.to_canonical_bytes().unwrap().len();
    blob.files = BTreeMap::from([(
        format!("padding{}", "x".repeat(len - base)),
        SecretBytes::from(Vec::<u8>::new()),
    )]);
    if len <= MAX_CANONICAL_RESOURCE_BYTES {
        assert_eq!(blob.to_canonical_bytes().unwrap().len(), len);
    }
    blob
}

fn canonical_one_byte_over_limit() -> Vec<u8> {
    let exact = blob_with_canonical_len(MAX_CANONICAL_RESOURCE_BYTES)
        .to_canonical_bytes()
        .unwrap();
    let mut oversized = exact.clone();
    let marker = br#""files":{"padding"#;
    let position = oversized
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    // The sole files key contains only unescaped ASCII. Extending that JSON
    // string by one ASCII byte preserves both JSON validity and JCS ordering.
    oversized.insert(position, b'x');
    assert_eq!(oversized.len(), MAX_CANONICAL_RESOURCE_BYTES + 1);
    assert!(std::str::from_utf8(&oversized).is_ok());
    let mut restored = oversized.clone();
    assert_eq!(restored.remove(position), b'x');
    assert_eq!(restored, exact);
    oversized
}

fn assert_backend_resource_boundaries(backend: &dyn StorageBackend) {
    for len in [
        MAX_CANONICAL_RESOURCE_BYTES - 1,
        MAX_CANONICAL_RESOURCE_BYTES,
    ] {
        let blob = blob_with_canonical_len(len);
        save_resource_blob(backend, "boundary", &blob).unwrap();
        assert_eq!(load_resource_blob(backend, "boundary").unwrap(), blob);
        assert_eq!(
            backend
                .read("resources/boundary")
                .unwrap()
                .unwrap()
                .as_ref()
                .len(),
            len
        );
    }

    let prior = backend
        .read("resources/boundary")
        .unwrap()
        .expect("prior bytes");
    let oversized = blob_with_canonical_len(MAX_CANONICAL_RESOURCE_BYTES + 1);
    let error = save_resource_blob(backend, "boundary", &oversized).unwrap_err();
    assert!(error.to_string().contains("exceeds maximum"));
    assert_eq!(
        backend.read("resources/boundary").unwrap().unwrap(),
        prior,
        "rejected save changed prior bytes"
    );

    backend
        .write("resources/injected", &canonical_one_byte_over_limit())
        .unwrap();
    let error = load_resource_blob(backend, "injected").unwrap_err();
    assert!(
        format!("{error:#}").contains("exceeds maximum"),
        "unexpected load error: {error:#}"
    );
}

#[test]
fn plain_and_encrypted_backends_enforce_complete_resource_boundaries() {
    let plain_tmp = tempfile::TempDir::new().unwrap();
    let plain = crate::PlainBackend::open(plain_tmp.path()).unwrap();
    assert_backend_resource_boundaries(&plain);

    let encrypted_tmp = tempfile::TempDir::new().unwrap();
    let encrypted =
        crate::EncryptedBackend::create_with_key(&encrypted_tmp.path().join("store"), [7; 32])
            .unwrap();
    assert_backend_resource_boundaries(&encrypted);
}

#[test]
fn oversized_save_is_rejected_before_storage_write() {
    let store = MemStore::new();
    save_resource_blob(&store, "boundary", &sample_blob("prior")).unwrap();
    let prior = store
        .data
        .lock()
        .unwrap()
        .get("resources/boundary")
        .unwrap()
        .clone();
    let writes = store.writes.load(Ordering::Relaxed);

    let oversized = blob_with_canonical_len(MAX_CANONICAL_RESOURCE_BYTES + 1);
    assert!(save_resource_blob(&store, "boundary", &oversized).is_err());
    assert_eq!(store.writes.load(Ordering::Relaxed), writes);
    assert_eq!(store.data.lock().unwrap()["resources/boundary"], prior);
}

#[test]
fn plain_backend_one_file_per_resource() {
    // Each saved resource lands as exactly one file on disk under
    // the `resources/<id>` path — the atomic-blob storage shape
    // for plain stores.
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("secrets");
    let backend = crate::PlainBackend::open(&dir).unwrap();

    save_resource_blob(&backend, "github/token", &sample_blob("a")).unwrap();
    save_resource_blob(&backend, "aws/creds", &sample_blob("b")).unwrap();

    let files: Vec<std::path::PathBuf> = walk_files(&dir.join("resources"))
        .into_iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_path_buf())
        .collect();
    let mut paths: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["resources/aws/creds", "resources/github/token"]);
}

#[test]
fn encrypted_backend_one_file_per_resource() {
    // Same invariant on the encrypted backend: one file per
    // resource keyed under `resources/<id>`. Encryption produces
    // one nonce per write, so this is also the "one nonce per
    // resource per save" assertion in disguise.
    let tmp = tempfile::TempDir::new().unwrap();
    let key: [u8; 32] = rand::random();
    let dir = tmp.path().join("secrets");
    let backend = crate::EncryptedBackend::create_with_key(&dir, key).unwrap();

    save_resource_blob(&backend, "github/token", &sample_blob("a")).unwrap();
    save_resource_blob(&backend, "aws/creds", &sample_blob("b")).unwrap();

    let mut paths: Vec<String> = walk_files(&dir.join("resources"))
        .into_iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["resources/aws/creds", "resources/github/token"]);

    // And the on-disk ciphertext is not the plain serialized blob.
    let raw = std::fs::read(dir.join("resources/github/token")).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("\"TOK\""),
        "plaintext leaked through encrypted backend",
    );
}

#[test]
fn list_resources_prefix_returns_expected_set() {
    let tmp = tempfile::TempDir::new().unwrap();
    let backend = crate::PlainBackend::open(tmp.path()).unwrap();
    save_resource_blob(&backend, "a/one", &sample_blob("a")).unwrap();
    save_resource_blob(&backend, "a/two", &sample_blob("b")).unwrap();
    save_resource_blob(&backend, "b/three", &sample_blob("c")).unwrap();

    let mut ids = backend.list("resources").unwrap();
    ids.sort();
    assert_eq!(
        ids,
        vec!["resources/a/one", "resources/a/two", "resources/b/three"]
    );
}

#[test]
fn read_prefixed_resources_returns_full_blobs() {
    // Reading each `resources/<id>` entry yields the same bytes
    // `to_canonical_bytes` produces — the storage layer doesn't
    // re-serialize, just round-trips the canonical JCS bytes.
    let tmp = tempfile::TempDir::new().unwrap();
    let backend = crate::PlainBackend::open(tmp.path()).unwrap();
    let blob_a = sample_blob("a");
    let blob_b = sample_blob("b");
    save_resource_blob(&backend, "a/one", &blob_a).unwrap();
    save_resource_blob(&backend, "b/two", &blob_b).unwrap();

    let raw_a = backend.read("resources/a/one").unwrap().expect("present");
    assert_eq!(Vec::<u8>::from(raw_a), blob_a.to_canonical_bytes().unwrap());
    let raw_b = backend.read("resources/b/two").unwrap().expect("present");
    assert_eq!(Vec::<u8>::from(raw_b), blob_b.to_canonical_bytes().unwrap());
}

fn walk_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for entry in std::fs::read_dir(&p).unwrap() {
                stack.push(entry.unwrap().path());
            }
        } else {
            out.push(p);
        }
    }
    out
}

/// Build a blob with env + files content plus a libsecret section
/// that exists only in the structured form. The libsecret content
/// must not leak into the flat-KV view; the admin tree endpoints are
/// the only way to address it.
fn populated_blob() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
    blob.files
        .insert("config/app.toml".into(), SecretBytes::from("x=1\n"));

    let mut items = BTreeMap::new();
    let mut attrs = BTreeMap::new();
    attrs.insert("server".into(), "github.com".into());
    items.insert(
        "gh-cli".into(),
        Item {
            label: "gh CLI".into(),
            content_type: "text/plain".into(),
            created: 100,
            modified: 200,
            attributes: attrs,
            secret: SecretBytes::from("ghp_secret"),
        },
    );

    let mut collections = BTreeMap::new();
    collections.insert(
        "default".into(),
        Collection {
            label: "Default".into(),
            created: 10,
            modified: 20,
            items,
        },
    );

    let mut aliases = BTreeMap::new();
    aliases.insert("default".into(), "default".into());

    blob.libsecret = LibsecretSection {
        aliases,
        collections,
    };
    blob
}

#[test]
fn expand_emits_only_env_and_files_entries() {
    let blob = populated_blob();
    let entries = expand_blob_to_entries("github/token", &blob);

    assert_eq!(
        entries.get("github/token/env/GH_TOKEN").unwrap(),
        &SecretBytes::from("ghp_test")
    );
    assert_eq!(
        entries.get("github/token/files/config/app.toml").unwrap(),
        &SecretBytes::from("x=1\n")
    );

    // Libsecret content is intentionally absent from the flat-KV projection;
    // structured /libsecret/* admin endpoints own that slot.
    let libsecret_keys: Vec<&String> = entries
        .keys()
        .filter(|k| k.contains("/libsecret/"))
        .collect();
    assert!(
        libsecret_keys.is_empty(),
        "libsecret content leaked into flat-KV projection: {libsecret_keys:?}",
    );
}
