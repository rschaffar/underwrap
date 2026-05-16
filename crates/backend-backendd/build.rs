//! Default the `UNDERWRAP_WEB_DIST` env var to the workspace-relative
//! `web/dist/` directory so `cargo build` works out of the box.
//!
//! The Nix derivation overrides this with the absolute path to the
//! sandbox-built SPA bundle, which lets `rust_embed`'s
//! `$UNDERWRAP_WEB_DIST` folder-interpolation pick up the right path
//! without the workspace's relative layout having to exist inside the
//! sandbox.

fn main() {
    println!("cargo:rerun-if-env-changed=UNDERWRAP_WEB_DIST");
    if std::env::var_os("UNDERWRAP_WEB_DIST").is_none() {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
        let default = format!("{manifest_dir}/../../web/dist");
        println!("cargo:rustc-env=UNDERWRAP_WEB_DIST={default}");
    }
}
