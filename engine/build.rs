//! Build script: embeds every `../locales/<lang>.json`. The wildcard is the point - adding a language
//! is dropping a file in, with no source edit. The bundles live here, in the engine, because the
//! window and the command line both speak the user's language.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    locales(&manifest);
}

fn locales(manifest: &Path) {
    let dir = manifest.join("../locales");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut codes: Vec<String> = fs::read_dir(&dir)
        .expect("locales/ next to the engine crate")
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
