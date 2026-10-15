//! Build script: turns the identity in `[package.metadata.yaiol]` into compile-time facts, so the
//! source holds no literal name. `YAIOL_PRODUCT_NAME`, `YAIOL_BUILD_NUMBER` (when a build counter is
//! set) and `YAIOL_BUILD_ID` (`<version>.<counter>`); name and version come from Cargo itself
//! (`CARGO_PKG_NAME`, `CARGO_PKG_VERSION`). The language bundles are embedded by the engine.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    identity(&manifest);
}

fn identity(manifest: &Path) {
    let cargo_toml = manifest.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", cargo_toml.display());

    let doc: toml::Table = fs::read_to_string(&cargo_toml)
        .expect("read Cargo.toml")
        .parse()
        .expect("parse Cargo.toml");
    let yaiol = doc
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("yaiol"))
        .and_then(|y| y.as_table())
        .expect("Cargo.toml needs a [package.metadata.yaiol] section");

    let product = yaiol
        .get("product-name")
        .and_then(|v| v.as_str())
        .expect("[package.metadata.yaiol] needs product-name");
    println!("cargo:rustc-env=YAIOL_PRODUCT_NAME={product}");

    // The BUILD COUNTER (the 4th identity segment a buffered commit bumps; 0 on a clean release).
    // Cargo's version is strictly three-part, and `dx bundle` writes its OWN Windows version resource
    // (from the Cargo version, build metadata stripped), so the counter cannot ride in either. It is
    // exported as one value and the app embeds it as a marker string the apps-cockpit reads from the
    // exe bytes - see BUILD_ID in main.rs.
    let build = yaiol.get("build-number").and_then(|v| v.as_integer()).unwrap_or(0);
    if build > 0 {
        println!("cargo:rustc-env=YAIOL_BUILD_NUMBER={build}");
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    println!("cargo:rustc-env=YAIOL_BUILD_ID={version}.{build}");
}
