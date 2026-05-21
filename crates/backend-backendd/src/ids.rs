//! Random-id helpers shared by store, client-instance, resource, and
//! rotation-staging paths. All ids here are non-cryptographic
//! disambiguators; secret-material RNG paths live in `auth.rs` and
//! `admin_api::stores` and use larger byte counts.

use rand::RngCore;

/// `<prefix>_<10 random bytes hex-encoded>` — 20 lowercase hex chars
/// after the underscore. Used by `generate_store_id`,
/// `generate_client_instance_id`, and `generate_resource_id`.
pub(crate) fn random_id(prefix: &str) -> String {
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(prefix.len() + 1 + bytes.len() * 2);
    out.push_str(prefix);
    out.push('_');
    for b in bytes {
        use std::fmt::Write;
        write!(&mut out, "{b:02x}").unwrap();
    }
    out
}

/// 8 random bytes hex-encoded as 16 lowercase characters. Used to make
/// transient on-disk paths unique without claiming key-grade entropy.
pub(crate) fn random_hex_8() -> String {
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut out, "{b:02x}").unwrap();
    }
    out
}
