//! Random-id helpers shared by store, client-instance, resource, and
//! rotation-staging paths. All ids here are non-cryptographic
//! disambiguators; secret-material RNG paths live in `auth.rs` and
//! `admin_api::stores` and use larger byte counts.

use rand::RngCore;

/// `<prefix>_<10 random bytes hex-encoded>` — 20 lowercase hex chars
/// after the underscore. Used by the store, client-instance, and
/// resource id paths via [`fresh_id`] / direct calls.
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

/// `random_id(prefix)` re-rolled until `exists(candidate)` returns
/// false. The retry loop is a paranoia guard against the ~80-bit
/// collision space; callers pass a closure that consults whatever
/// registry the id is about to live in.
pub(crate) fn fresh_id<F: Fn(&str) -> bool>(prefix: &str, exists: F) -> String {
    loop {
        let candidate = random_id(prefix);
        if !exists(&candidate) {
            return candidate;
        }
    }
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
