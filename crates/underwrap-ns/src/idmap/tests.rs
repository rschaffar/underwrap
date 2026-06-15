use super::*;

const HOME: &str = "/home/alice";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

#[test]
fn tilde_resolves_to_home() {
    assert_eq!(resolve_spec("~", &home()).unwrap(), home());
}

#[test]
fn tilde_rel_resolves_under_home() {
    assert_eq!(
        resolve_spec("~/.config/gh", &home()).unwrap(),
        home().join(".config/gh")
    );
}

#[test]
fn absolute_path_passes_through() {
    assert_eq!(
        resolve_spec("/srv/data", &home()).unwrap(),
        PathBuf::from("/srv/data")
    );
}

#[test]
fn relative_non_tilde_is_rejected() {
    assert!(resolve_spec(".config/gh", &home()).is_err());
}

#[test]
fn tilde_user_form_is_rejected() {
    assert!(resolve_spec("~bob/x", &home()).is_err());
}

#[test]
fn empty_is_rejected() {
    assert!(resolve_spec("", &home()).is_err());
}

#[test]
fn parent_dir_traversal_is_rejected() {
    assert!(resolve_spec("~/../etc", &home()).is_err());
    assert!(resolve_spec("/srv/../etc/shadow", &home()).is_err());
}

#[test]
fn unescape_mountinfo_decodes_octal_escapes() {
    assert_eq!(unescape_mountinfo("/home/alice"), "/home/alice");
    // \040 space, \011 tab, \012 newline, \134 backslash
    assert_eq!(unescape_mountinfo(r"/mnt/my\040data"), "/mnt/my data");
    assert_eq!(unescape_mountinfo(r"/a\134b"), r"/a\b");
    // A trailing lone backslash (not a full \ooo escape) is copied through.
    assert_eq!(unescape_mountinfo(r"/a\b"), r"/a\b");
}

/// One mountinfo line with the given mount point in field 5 (the fields before
/// it are placeholder ids; only field 5 is parsed).
fn line(mountpoint: &str) -> String {
    format!("36 35 0:30 / {mountpoint} rw,relatime shared:1 - ext4 /dev/x rw")
}

#[test]
fn submounts_under_finds_strict_descendants_relative_to_base() {
    let mi = [
        line("/home/alice"),             // the base itself — excluded
        line("/home/alice/data"),        // a child — included as "data"
        line("/home/alice/data/nested"), // a grandchild — included as "data/nested"
        line("/home/bob"),               // a sibling — excluded
        line("/home/alicia"),            // prefix-but-not-child — excluded
    ]
    .join("\n");
    let rels = submounts_under(&mi, Path::new("/home/alice"));
    assert_eq!(
        rels,
        vec![PathBuf::from("data"), PathBuf::from("data/nested")],
    );
}

#[test]
fn submounts_under_handles_escaped_mountpoints() {
    let mi = line(r"/home/alice/my\040data");
    let rels = submounts_under(&mi, Path::new("/home/alice"));
    assert_eq!(rels, vec![PathBuf::from("my data")]);
}

#[test]
fn submounts_under_empty_when_no_children() {
    let mi = [line("/home/alice"), line("/srv/other")].join("\n");
    assert!(submounts_under(&mi, Path::new("/home/alice")).is_empty());
}
