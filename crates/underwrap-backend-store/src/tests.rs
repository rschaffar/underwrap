use super::*;

#[test]
fn open_dispatches_to_plain_backend() {
    let dir = tempfile::TempDir::new().unwrap();
    let backend = open(&StorageConfig::Plain {
        dir: dir.path().join("plain"),
    })
    .unwrap();
    backend.write("github/token", b"ghp_test").unwrap();
    assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
}

#[test]
fn open_refuses_encrypted_config() {
    let dir = tempfile::TempDir::new().unwrap();
    let result = open(&StorageConfig::Encrypted {
        dir: dir.path().join("encrypted"),
    });
    match result {
        Ok(_) => panic!("encrypted configs must be refused by the generic dispatcher"),
        Err(err) => assert!(
            err.to_string()
                .contains("EncryptedBackend::open_existing_with_key"),
            "unexpected error: {err}",
        ),
    }
}

#[test]
fn parse_hex_key_accepts_canonical_lowercase() {
    let hex: String = (0u8..32).map(|b| format!("{b:02x}")).collect();
    let bytes = parse_hex_key(&hex).unwrap();
    for (i, b) in bytes.iter().enumerate() {
        assert_eq!(*b, i as u8);
    }
}

#[test]
fn parse_hex_key_accepts_uppercase_and_whitespace() {
    let hex: String = (0u8..32).map(|b| format!("{b:02X}")).collect();
    let padded = format!("\n  {hex}  \n");
    assert!(parse_hex_key(&padded).is_ok());
}

#[test]
fn parse_hex_key_rejects_wrong_length() {
    let err = parse_hex_key("deadbeef").unwrap_err();
    assert!(
        err.to_string().contains("exactly 64 hex characters"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_hex_key_rejects_non_hex() {
    let bad = "z".repeat(64);
    assert!(parse_hex_key(&bad).is_err());
}

#[test]
fn strict_key_parser_accepts_only_unpadded_lowercase_hex() {
    let canonical = "ab".repeat(32);
    assert!(parse_hex_key_strict(&canonical).is_ok());
    assert!(parse_hex_key_strict(&canonical.to_ascii_uppercase()).is_err());
    assert!(parse_hex_key_strict(&format!(" {canonical}")).is_err());
    assert!(parse_hex_key_strict(&format!("{canonical}\n")).is_err());
}
