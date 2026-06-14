use super::*;
use std::io::Write;

fn write_table(contents: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn missing_table_fails_closed() {
    let r = resolve_isolation_from("/nonexistent/underwrap/target-uids", "/usr/bin/target");
    assert!(r.is_err(), "missing table must fail closed, got {r:?}");
}

#[test]
fn path_not_in_table_fails_closed() {
    let t = write_table("/usr/bin/other\tunderwrap-target-x\tunderwrap-target-x\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(r.is_err(), "absent path must fail closed, got {r:?}");
}

#[test]
fn uid_zero_is_rejected() {
    // "root" resolves to uid 0 on every system => must be refused (no escalation,
    // even if the root-owned table were somehow made to name it).
    let t = write_table("/usr/bin/target\troot\troot\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(r.is_err(), "root/uid0 must be rejected, got {r:?}");
}

#[test]
fn unknown_user_is_hard_error() {
    // A path present in the table but unresolvable must fail closed — never run
    // the target as the caller.
    let t =
        write_table("/usr/bin/target\tunderwrap-no-such-user-zzz\tunderwrap-no-such-group-zzz\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(r.is_err(), "unknown user must fail closed, got {r:?}");
}

#[test]
fn extra_fields_is_hard_error() {
    let t = write_table("/usr/bin/target\ta\tb\tc\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(
        r.is_err(),
        "extra tab-separated fields must error, got {r:?}"
    );
}

#[test]
fn missing_group_field_is_hard_error() {
    let t = write_table("/usr/bin/target\tonlyuser\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(r.is_err(), "missing group field must error, got {r:?}");
}

#[test]
fn blank_lines_are_skipped() {
    let t = write_table("\n/usr/bin/other\tu\tg\n\n");
    let r = resolve_isolation_from(t.path().to_str().unwrap(), "/usr/bin/target");
    assert!(
        r.is_err(),
        "blank lines skipped, absent path must fail closed, got {r:?}"
    );
}

#[test]
fn with_target_fsuid_runs_closure_for_own_ids() {
    // Switching the filesystem ids to our own uid/gid is always permitted (no
    // capability needed), so the closure runs and its value is returned.
    let me = unsafe { libc::getuid() };
    let mygid = unsafe { libc::getgid() };
    let iso = Isolation {
        uid: me,
        gid: mygid,
    };
    let out = with_target_fsuid(iso, || 41 + 1);
    assert!(matches!(out, Ok(42)), "expected Ok(42), got {out:?}");
}

#[test]
fn with_target_fsuid_fails_closed_without_cap() {
    // As an unprivileged test process (no CAP_SETUID), setfsuid to a *foreign*
    // uid is silently ignored by the kernel, so the verify step must catch it
    // and the window must NOT run the closure. (If the suite somehow runs as
    // root the switch would succeed; skip the assertion in that case.)
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipping: running as root, the switch would succeed");
        return;
    }
    // Pick a uid/gid that is not our own. 1 (daemon/bin) is present on every
    // system and is not the test user.
    let foreign = if unsafe { libc::getuid() } == 1 { 2 } else { 1 };
    let iso = Isolation {
        uid: foreign,
        gid: foreign,
    };
    let mut ran = false;
    let out = with_target_fsuid(iso, || {
        ran = true;
    });
    assert!(out.is_err(), "expected fail-closed Err, got {out:?}");
    assert!(
        !ran,
        "closure must not run when the fsuid window cannot be entered"
    );
}
