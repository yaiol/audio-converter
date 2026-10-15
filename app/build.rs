//! Build script. Two jobs, both turning files into compile-time facts so the source holds no
//! literal identity and no language list:
//!
//! 1. Identity — `[package.metadata.yaiol]` in this crate's Cargo.toml becomes `YAIOL_PRODUCT_NAME`,
//!    `YAIOL_BUILD_NUMBER` (when a build counter is set) and `YAIOL_BUILD_ID` (`<version>.<counter>`).
//!    Name and version come from Cargo itself (`CARGO_PKG_NAME`, `CARGO_PKG_VERSION`).
//! 2. Locales — every `../locales/<lang>.json` is embedded. The wildcard is the point: adding a
//!    language is dropping a file in, with no source edit.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    identity(&manifest);
    locales(&manifest);
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

fn locales(manifest: &Path) {
    let dir = manifest.join("../locales");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut codes: Vec<String> = fs::read_dir(&dir)
        .expect("locales/ next to the app crate")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
        .collect();
    codes.sort();

    let mut out = String::from("pub const BUNDLES: &[(&str, &str)] = &[\n");
    for code in &codes {
        let path = dir.join(format!("{code}.json")).canonicalize().unwrap();
        println!("cargo:rerun-if-changed={}", path.display());
        out.push_str(&format!(
            "    ({code:?}, include_str!({:?})),\n",
            path.display().to_string().trim_start_matches(r"\\?\")
        ));
    }
    out.push_str("];\n");

    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("locales.rs"), out).unwrap();
}
