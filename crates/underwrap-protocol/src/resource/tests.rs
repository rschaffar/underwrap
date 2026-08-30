use super::*;
use crate::target_env::FORCED_TARGET_ENV_NAMES;

const RESERVED_ENV_REASON: &str = "environment variable name is reserved by underwrap hardening";

#[test]
fn resource_blob_key_is_stable() {
    assert_eq!(resource_blob_key("github/token"), "resources/github/token");
    assert_eq!(resource_blob_key("gh"), "resources/gh");
}

#[test]
fn parse_secret_id_handles_all_slots() {
    let p = parse_secret_id("github/token/env/GH_TOKEN").unwrap();
    assert_eq!(p.resource_id, "github/token");
    assert_eq!(p.slot, ResourceSlot::Env);
    assert_eq!(p.inner, "GH_TOKEN");

    let p = parse_secret_id("gws/state/files/config.yaml").unwrap();
    assert_eq!(p.resource_id, "gws/state");
    assert_eq!(p.slot, ResourceSlot::Files);
    assert_eq!(p.inner, "config.yaml");

    let p = parse_secret_id("gh/libsecret/oauth").unwrap();
    assert_eq!(p.resource_id, "gh");
    assert_eq!(p.slot, ResourceSlot::Libsecret);
    assert_eq!(p.inner, "oauth");

    // System keys (no slot in the canonical position) → None.
    assert!(parse_secret_id("control/policy").is_none());
    assert!(parse_secret_id("no-slot-here").is_none());
}

#[test]
fn compose_secret_id_round_trips_through_parse() {
    let composed = compose_secret_id("github/token", ResourceSlot::Env, "GH_TOKEN");
    assert_eq!(composed, "github/token/env/GH_TOKEN");
    let parsed = parse_secret_id(&composed).unwrap();
    assert_eq!(parsed.resource_id, "github/token");
    assert_eq!(parsed.slot, ResourceSlot::Env);
    assert_eq!(parsed.inner, "GH_TOKEN");
}

#[test]
fn validate_secret_key_accepts_per_slot_shapes() {
    assert!(validate_secret_key(ResourceSlot::Env, "GH_TOKEN").is_ok());
    assert!(validate_secret_key(ResourceSlot::Files, "config.yaml").is_ok());
    assert!(validate_secret_key(ResourceSlot::Files, "nested/dir/file").is_ok());
    assert!(validate_secret_key(ResourceSlot::Libsecret, "default/gh-cli.secret").is_ok());
}

#[test]
fn validate_secret_key_rejects_invalid_shapes() {
    assert!(validate_secret_key(ResourceSlot::Env, "").is_err());
    assert!(validate_secret_key(ResourceSlot::Env, "bad-name").is_err());
    assert!(validate_secret_key(ResourceSlot::Env, "PATH").is_err());
    assert!(validate_secret_key(ResourceSlot::Files, "").is_err());
    assert!(validate_secret_key(ResourceSlot::Files, "/abs").is_err());
    assert!(validate_secret_key(ResourceSlot::Files, "a/../b").is_err());
    assert!(validate_secret_key(ResourceSlot::Files, "a//b").is_err());
    assert!(validate_secret_key(ResourceSlot::Libsecret, "x\0y").is_err());
}

#[test]
fn env_validation_rejects_dangerous_names() {
    assert!(validate_env_name("GH_TOKEN").is_ok());
    assert!(validate_env_name("_TOKEN1").is_ok());
    assert!(validate_env_name("LD_PRELOAD").is_err());
    assert!(validate_env_name("PATH").is_err());
    assert!(validate_env_name("UNDERWRAP_SECRETSD").is_err());
    assert!(validate_env_name("1TOKEN").is_err());
    assert!(validate_env_name("BAD-NAME").is_err());
}

#[test]
fn every_forced_target_env_name_is_reserved_for_resource_projection() {
    for name in FORCED_TARGET_ENV_NAMES {
        assert_eq!(validate_env_name(name), Err(RESERVED_ENV_REASON), "{name}");
        assert_eq!(
            validate_secret_key(ResourceSlot::Env, name),
            Err(RESERVED_ENV_REASON.to_string()),
            "{name}",
        );

        let near_miss = format!("{name}_APP");
        assert_eq!(validate_env_name(&near_miss), Ok(()), "{near_miss}");
        assert_eq!(
            validate_secret_key(ResourceSlot::Env, &near_miss),
            Ok(()),
            "{near_miss}",
        );
    }
}
