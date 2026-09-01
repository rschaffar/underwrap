use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

#[cfg(unix)]
use std::process::Command;

use super::*;
use crate::test_support::{TreeEntry, assert_secure_encrypted_tree, tree_snapshot};

struct TestHooks {
    ids: Mutex<VecDeque<String>>,
    failures: HashSet<RotationPhase>,
    key: [u8; 32],
}

impl TestHooks {
    fn new(ids: impl IntoIterator<Item = &'static str>, failures: &[RotationPhase]) -> Self {
        Self::with_key(ids, failures, [9; 32])
    }

    fn with_key(
        ids: impl IntoIterator<Item = &'static str>,
        failures: &[RotationPhase],
        key: [u8; 32],
    ) -> Self {
        Self {
            ids: Mutex::new(ids.into_iter().map(str::to_owned).collect()),
            failures: failures.iter().copied().collect(),
            key,
        }
    }
}

impl RotationHooks for TestHooks {
    fn rotation_id(&self) -> String {
        self.ids.lock().unwrap().pop_front().unwrap()
    }

    fn replacement_key(&self) -> [u8; 32] {
        self.key
    }

    fn checkpoint(&self, phase: RotationPhase) -> io::Result<()> {
        if self.failures.contains(&phase) {
            Err(io::Error::other("injected rotation failure"))
        } else {
            Ok(())
        }
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, [u8; 32], EncryptedBackend) {
    let tmp = tempfile::TempDir::new().unwrap();
    let canonical = tmp.path().join("store-data");
    let key = [3; 32];
    let backend = EncryptedBackend::create_with_key(&canonical, key).unwrap();
    backend
        .write(
            underwrap_backend_store::ENCRYPTED_CANARY_ID,
            underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
        )
        .unwrap();
    backend.write("resource/item", b"seeded-value").unwrap();
    backend.write("control/policy", b"policy-document").unwrap();
    (tmp, canonical, key, backend)
}

fn workspace_paths(parent: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(parent)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("underwrap-rotation")
        })
        .collect();
    paths.sort();
    paths
}

fn assert_generation(path: &Path, key: [u8; 32]) {
    let backend = EncryptedBackend::open_existing_with_key(path, key).unwrap();
    assert_eq!(
        backend.read("resource/item").unwrap().unwrap().as_ref(),
        b"seeded-value"
    );
    assert_eq!(
        backend.read("control/policy").unwrap().unwrap().as_ref(),
        b"policy-document"
    );
    assert_eq!(
        backend
            .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
        underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
    );
}

#[test]
fn success_retains_complete_secure_old_generation() {
    let (tmp, canonical, old_key, backend) = fixture();
    let hooks = TestHooks::new(["1111111111111111"], &[]);
    let result = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap();
    let new_key = underwrap_backend_store::parse_hex_key(&result.key_hex).unwrap();
    assert_generation(&canonical, new_key);
    let workspaces = workspace_paths(tmp.path());
    assert_eq!(workspaces.len(), 1);
    assert_generation(&workspaces[0].join("backup"), old_key);
    let workspace_name = workspaces[0].file_name().unwrap().to_string_lossy();
    assert!(!workspace_name.contains(&result.key_hex));
    assert!(!workspace_name.contains("resource/item"));
    for generation in [&canonical, &workspaces[0].join("backup")] {
        for id in [
            "resource/item",
            "control/policy",
            underwrap_backend_store::ENCRYPTED_CANARY_ID,
        ] {
            let raw = std::fs::read(generation.join(id)).unwrap();
            assert!(
                !raw.windows(b"seeded-value".len())
                    .any(|window| window == b"seeded-value")
            );
            assert!(
                !raw.windows(result.key_hex.len())
                    .any(|window| window == result.key_hex.as_bytes())
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&workspaces[0])
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(workspaces[0].join("backup"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[cfg(unix)]
struct RestrictiveUmaskHooks {
    parent: PathBuf,
}

#[cfg(unix)]
struct OwnerReadableExitUmask;

#[cfg(unix)]
impl Drop for OwnerReadableExitUmask {
    fn drop(&mut self) {
        // SAFETY: this runs only in the exact, single-test child after the
        // restrictive-umask assertions finish or unwind. The process exits next.
        unsafe {
            libc::umask(0o077);
        }
    }
}

#[cfg(unix)]
impl RotationHooks for RestrictiveUmaskHooks {
    fn rotation_id(&self) -> String {
        "1111111111111111".into()
    }

    fn checkpoint(&self, phase: RotationPhase) -> io::Result<()> {
        if phase == RotationPhase::OpenStage {
            use std::os::unix::fs::PermissionsExt;
            let workspaces = workspace_paths(&self.parent);
            assert_eq!(workspaces.len(), 1);
            assert_eq!(
                std::fs::symlink_metadata(&workspaces[0])
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            return Err(io::Error::other("stop after workspace mode check"));
        }
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn workspace_mode_is_exact_and_failure_cleans_up_under_restrictive_umask() {
    const CHILD_ENV: &str = "UNDERWRAP_ROTATION_UMASK_CHILD";
    const TEST_NAME: &str = "admin_api::stores::lifecycle::rotation::tests::workspace_mode_is_exact_and_failure_cleans_up_under_restrictive_umask";

    if let Ok(parent) = std::env::var(CHILD_ENV) {
        let _exit_umask = OwnerReadableExitUmask;
        let parent = PathBuf::from(parent);
        let canonical = parent.join("store-data");
        let backend = EncryptedBackend::create_with_key(&canonical, [3; 32]).unwrap();
        backend
            .write(
                underwrap_backend_store::ENCRYPTED_CANARY_ID,
                underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
            )
            .unwrap();
        let hooks = RestrictiveUmaskHooks {
            parent: parent.clone(),
        };
        let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
        assert_eq!(failure.disposition, FailureDisposition::KeepUnlocked);
        assert!(workspace_paths(&parent).is_empty());
        std::fs::remove_dir_all(parent).unwrap();
        return;
    }

    let script = r#"
set -eu
parent=$(mktemp -d)
chmod 0700 "$parent"
export UNDERWRAP_ROTATION_UMASK_CHILD="$parent"
umask 0700
exec "$1" --exact "$2" --nocapture
"#;
    let status = Command::new("/bin/sh")
        .args(["-c", script, "underwrap-umask-child"])
        .arg(std::env::current_exe().unwrap())
        .arg(TEST_NAME)
        .status()
        .unwrap();
    assert!(status.success(), "restrictive-umask child failed: {status}");
}

#[test]
fn generated_key_hex_is_wiped_on_failure_and_discard() {
    let before_failure = key_hex_wipe_count();
    let (_tmp, canonical, _, backend) = fixture();
    let hooks = TestHooks::new(["1111111111111111"], &[RotationPhase::ListSource]);
    rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(key_hex_wipe_count(), before_failure + 1);

    let (_tmp, canonical, _, backend) = fixture();
    let before_success = key_hex_wipe_count();
    let result = rotate_generation(
        "store-a",
        &canonical,
        &backend,
        &TestHooks::new(["2222222222222222"], &[]),
    )
    .unwrap();
    assert_eq!(key_hex_wipe_count(), before_success);
    result.discard();
    assert_eq!(key_hex_wipe_count(), before_success + 1);
}

#[test]
fn collisions_are_never_reused_or_overwritten() {
    let (tmp, canonical, _, backend) = fixture();
    let file_collision = tmp
        .path()
        .join(".store-a.underwrap-rotation-1111111111111111");
    let dir_collision = tmp
        .path()
        .join(".store-a.underwrap-rotation-2222222222222222");
    let symlink_collision = tmp
        .path()
        .join(".store-a.underwrap-rotation-3333333333333333");
    std::fs::write(&file_collision, b"untouched").unwrap();
    std::fs::create_dir(&dir_collision).unwrap();
    std::fs::write(dir_collision.join("marker"), b"directory-untouched").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&canonical, &symlink_collision).unwrap();
    let hooks = TestHooks::new(
        [
            "1111111111111111",
            "2222222222222222",
            "3333333333333333",
            "4444444444444444",
        ],
        &[],
    );
    rotate_generation("store-a", &canonical, &backend, &hooks).unwrap();
    assert_eq!(std::fs::read(&file_collision).unwrap(), b"untouched");
    assert_eq!(
        std::fs::read(dir_collision.join("marker")).unwrap(),
        b"directory-untouched"
    );
    #[cfg(unix)]
    assert!(
        std::fs::symlink_metadata(&symlink_collision)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        tmp.path()
            .join(".store-a.underwrap-rotation-4444444444444444/backup")
            .is_dir()
    );
}

#[test]
fn every_pre_displacement_failure_leaves_old_generation_usable() {
    for (index, phase) in [
        RotationPhase::ReserveWorkspace,
        RotationPhase::OpenStage,
        RotationPhase::ListSource,
        RotationPhase::WriteCanary,
        RotationPhase::ReadSource,
        RotationPhase::WriteDestination,
        RotationPhase::VerifyStage,
        RotationPhase::DisplaceLive,
    ]
    .into_iter()
    .enumerate()
    {
        let (tmp, canonical, old_key, backend) = fixture();
        let id = format!("{index:016x}");
        let hooks = TestHooks {
            ids: Mutex::new([id].into_iter().collect()),
            failures: [phase].into_iter().collect(),
            key: [9; 32],
        };
        let before = tree_snapshot(&canonical);
        let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
        assert_eq!(failure.disposition, FailureDisposition::KeepUnlocked);
        assert_generation(&canonical, old_key);
        assert_eq!(tree_snapshot(&canonical), before);
        assert!(workspace_paths(tmp.path()).is_empty());
    }
}

#[test]
fn promotion_failure_rolls_back_without_deleting_complete_stage() {
    let (tmp, canonical, old_key, backend) = fixture();
    let hooks = TestHooks::new(["1111111111111111"], &[RotationPhase::PromoteStage]);
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::KeepUnlocked);
    assert_generation(&canonical, old_key);
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert!(workspace.join("stage").is_dir());
    assert!(!workspace.join("backup").exists());
}

#[test]
fn failed_rollback_retains_old_backup_and_requires_locked_recovery() {
    let (tmp, canonical, old_key, backend) = fixture();
    let hooks = TestHooks::new(
        ["1111111111111111"],
        &[RotationPhase::PromoteStage, RotationPhase::RestoreBackup],
    );
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::Lock);
    assert!(!canonical.exists());
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert_generation(&workspace.join("backup"), old_key);
    assert!(workspace.join("stage").is_dir());
}

#[test]
fn promoted_open_failure_rolls_back_and_preserves_new_generation() {
    let (tmp, canonical, old_key, backend) = fixture();
    let hooks = TestHooks::new(["1111111111111111"], &[RotationPhase::OpenPromoted]);
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::KeepUnlocked);
    assert_generation(&canonical, old_key);
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert!(workspace.join("stage").is_dir());
    assert!(!workspace.join("backup").exists());
}

#[test]
fn failed_post_promotion_rollback_preserves_every_generation() {
    let (tmp, canonical, old_key, backend) = fixture();
    let old_snapshot = tree_snapshot(&canonical);
    let hooks = TestHooks::new(
        ["1111111111111111"],
        &[
            RotationPhase::OpenPromoted,
            RotationPhase::MovePromotedAside,
        ],
    );
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::Lock);
    assert_eq!(failure.audit_reason, "rollback-failed");
    assert!(canonical.is_dir());
    assert_generation(&canonical, [9; 32]);
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert_generation(&workspace.join("backup"), old_key);
    assert_eq!(tree_snapshot(&workspace.join("backup")), old_snapshot);
    assert!(!workspace.join("stage").exists());
    assert_secure_encrypted_tree(&canonical);
    assert_secure_encrypted_tree(&workspace);
}

#[test]
fn promoted_verification_failure_restores_old_and_retains_complete_new_generation() {
    let (tmp, canonical, old_key, backend) = fixture();
    let old_snapshot = tree_snapshot(&canonical);
    let hooks = TestHooks::new(["1111111111111111"], &[RotationPhase::VerifyPromoted]);
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::KeepUnlocked);
    assert_eq!(failure.audit_reason, "reopen-failed");
    assert_generation(&canonical, old_key);
    assert_eq!(tree_snapshot(&canonical), old_snapshot);
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert_generation(&workspace.join("stage"), [9; 32]);
    assert!(!workspace.join("backup").exists());
    assert_secure_encrypted_tree(&workspace);
}

#[test]
fn post_promotion_restore_failure_keeps_old_backup_and_new_stage() {
    let (tmp, canonical, old_key, backend) = fixture();
    let old_snapshot = tree_snapshot(&canonical);
    let hooks = TestHooks::new(
        ["1111111111111111"],
        &[RotationPhase::OpenPromoted, RotationPhase::RestoreBackup],
    );
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::Lock);
    assert_eq!(failure.audit_reason, "rollback-failed");
    assert!(!canonical.exists());
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert_generation(&workspace.join("backup"), old_key);
    assert_eq!(tree_snapshot(&workspace.join("backup")), old_snapshot);
    assert_generation(&workspace.join("stage"), [9; 32]);
    assert_secure_encrypted_tree(&workspace);
}

struct MissingCanonicalOnDisplace {
    displaced: PathBuf,
}

impl RotationHooks for MissingCanonicalOnDisplace {
    fn rotation_id(&self) -> String {
        "1111111111111111".into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        [9; 32]
    }

    fn rename(&self, phase: RotationPhase, from: &Path, to: &Path) -> io::Result<()> {
        if phase == RotationPhase::DisplaceLive {
            std::fs::rename(from, &self.displaced)?;
            Err(io::Error::other("canonical disappeared during rename"))
        } else {
            std::fs::rename(from, to)
        }
    }
}

#[test]
fn first_rename_failure_locks_when_canonical_is_no_longer_usable() {
    let (tmp, canonical, old_key, backend) = fixture();
    let displaced = tmp.path().join("externally-displaced");
    let hooks = MissingCanonicalOnDisplace {
        displaced: displaced.clone(),
    };
    let failure = rotate_generation("store-a", &canonical, &backend, &hooks).unwrap_err();
    assert_eq!(failure.disposition, FailureDisposition::Lock);
    assert!(format!("{:?}", failure.error).contains("rotation_recovery_required"));
    assert!(!canonical.exists());
    assert_generation(&displaced, old_key);
    let workspace = workspace_paths(tmp.path()).pop().unwrap();
    assert_generation(&workspace.join("stage"), [9; 32]);
}

#[test]
fn three_rotations_retain_each_prior_generation_unchanged() {
    let (tmp, canonical, old_key, backend) = fixture();
    let mut keys = vec![old_key];
    let mut active: Arc<dyn StorageBackend> = Arc::new(backend);
    let mut prior_snapshots: Vec<(PathBuf, Vec<TreeEntry>)> = Vec::new();
    for (id, key) in [
        ("1111111111111111", [10; 32]),
        ("2222222222222222", [11; 32]),
        ("3333333333333333", [12; 32]),
    ] {
        let hooks = TestHooks::with_key([id], &[], key);
        let result = rotate_generation("store-a", &canonical, active.as_ref(), &hooks).unwrap();
        keys.push(key);
        active = result.backend;
        for (path, before) in &prior_snapshots {
            assert_eq!(
                &tree_snapshot(path),
                before,
                "changed retained tree {path:?}"
            );
        }
        let newest = workspace_paths(tmp.path()).pop().unwrap();
        assert_secure_encrypted_tree(&newest);
        prior_snapshots.push((newest.clone(), tree_snapshot(&newest)));
    }
    let workspaces = workspace_paths(tmp.path());
    assert_eq!(workspaces.len(), 3);
    for (workspace, key) in workspaces.iter().zip(keys.iter()) {
        assert_generation(&workspace.join("backup"), *key);
    }
    assert_generation(&canonical, *keys.last().unwrap());
    assert_secure_encrypted_tree(&canonical);
}
