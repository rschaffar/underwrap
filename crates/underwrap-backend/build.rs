//! Resolve `$UNDERWRAP_WEB_DIST` and assert the SPA bundle is populated.
//!
//! The Nix `underwrap-backend` derivation sets `UNDERWRAP_WEB_DIST` to
//! the absolute path of the sandbox-built SPA. For `cargo build`
//! outside Nix we default to `crates/underwrap-backend/../../web/dist`.
//!
//! Either way, the resolved path *must* contain an `index.html` —
//! otherwise `rust-embed` would embed an empty asset table and the
//! resulting binary would 404 on `GET /`. Detecting that at build
//! time is the whole point of this script: a hand-rolled `cargo
//! build` without `npm --prefix web run build` first now fails with
//! a clear, actionable error instead of producing a broken artifact.
//!
//! The `web.rs` module relies on this invariant — its `serve_spa`
//! fallback assumes `index.html` is present and panics with the same
//! "build.rs guarantees this" message if it is not.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=UNDERWRAP_WEB_DIST");

    let resolved = match std::env::var_os("UNDERWRAP_WEB_DIST") {
        Some(value) => PathBuf::from(value),
        None => {
            let manifest_dir =
                std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
            let default = PathBuf::from(manifest_dir).join("../../web/dist");
            // Tell rust-embed where to look (it reads the env at
            // proc-macro expansion time).
            println!("cargo:rustc-env=UNDERWRAP_WEB_DIST={}", default.display());
            default
        }
    };

    let index = resolved.join("index.html");
    if !index.is_file() {
        let hint = if std::env::var_os("UNDERWRAP_WEB_DIST").is_some() {
            format!(
                "UNDERWRAP_WEB_DIST points at '{}', but '{}' does not exist. \
                 Point the env var at a populated SPA bundle.",
                resolved.display(),
                index.display(),
            )
        } else {
            format!(
                "Expected the admin SPA bundle at '{}'. Run \
                 `npm --prefix web run build` once before building \
                 underwrap-backend, or set UNDERWRAP_WEB_DIST to point \
                 at a populated bundle. (Nix builds set this \
                 automatically — this error only fires on hand-rolled \
                 cargo builds.)",
                resolved.display(),
            )
        };
        panic!("{hint}");
    }

    // Rebuild when the SPA's entry file changes — covers a
    // `npm run build` that overwrites `dist/`. Vite emits
    // content-hashed asset names, so `index.html`'s `<script src=...>`
    // attribute changes on every rebuild, which is enough of a
    // canary.
    println!("cargo:rerun-if-changed={}", index.display());
}
