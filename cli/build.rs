//! Build script: the command line has no identity of its own. The product name (which names the
//! settings folder it shares with the window) and the version are the app's, read from the app
//! crate's Cargo.toml, so there is one place to change either.

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let app = manifest.join("../app/Cargo.toml");
    println!("cargo:rerun-if-changed={}", app.display());

    let doc: toml::Table = fs::read_to_string(&app).expect("read ../app/Cargo.toml").parse().expect("parse ../app/Cargo.toml");
    let package = doc.get("package").and_then(|p| p.as_table()).expect("[package] in the app's Cargo.toml");
    let version = package.get("version").and_then(|v| v.as_str()).expect("the app's version");
    let product = package
        .get("metadata")
        .and_then(|m| m.get("yaiol"))
        .and_then(|y| y.get("product-name"))
        .and_then(|v| v.as_str())
        .expect("the app's [package.metadata.yaiol] product-name");

    println!("cargo:rustc-env=YAIOL_PRODUCT_NAME={product}");
    println!("cargo:rustc-env=YAIOL_APP_VERSION={version}");
}
