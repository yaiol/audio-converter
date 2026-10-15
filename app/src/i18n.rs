//! Localisation: flat JSON bundles embedded at build time (see build.rs), `en` canonical.
//! Lookup chain is the workspace's: current language → English → the key itself.
//!
//! ⚠ Every call site spells the key as a string literal (`t("tipHdrHelp")`): the audit finds
//! references by scanning source, and a key passed through a variable is invisible to it.

use serde::Deserialize;
use std::sync::LazyLock;

pub use engine::i18n::t;

#[derive(Clone, PartialEq, Deserialize)]
pub struct Language {
    pub key: String,
    pub label: String,
}

/// The language picker's entries, each in its own language.
pub static LANGUAGES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../languages.json")).expect("languages.json is valid")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_in_the_picker_has_a_bundle() {
        for l in LANGUAGES.iter() {
            assert!(engine::i18n::has_bundle(l.key.as_str()), "no bundle for {}", l.key);
        }
    }
}
