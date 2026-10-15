//! Localisation: flat JSON bundles embedded at build time (see build.rs), `en` canonical.
//! Lookup chain is the workspace's: current language → English → the key itself.
//!
//! ⚠ Every call site spells the key as a string literal (`t("tipHdrHelp")`): the audit finds
//! references by scanning source, and a key passed through a variable is invisible to it.

use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

#[derive(Clone, PartialEq, Deserialize)]
pub struct Language {
    pub key: String,
    pub label: String,
}

/// The language picker's entries, each in its own language.
pub static LANGUAGES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../languages.json")).expect("languages.json is valid")
});

type Bundle = HashMap<String, String>;

static PARSED: LazyLock<HashMap<&'static str, Bundle>> = LazyLock::new(|| {
    BUNDLES
        .iter()
        .map(|(code, json)| {
            let bundle = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("locales/{code}.json is not a flat string map: {e}"));
            (*code, bundle)
        })
        .collect()
});

pub fn t(lang: &str, key: &str) -> String {
    let lookup = |code: &str| PARSED.get(code).and_then(|b| b.get(key));
    lookup(lang)
        .or_else(|| lookup("en"))
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english_then_to_the_key() {
        assert_eq!(t("en", "ttlStg"), "Settings");
        assert_eq!(t("xx", "ttlStg"), "Settings");
        assert_eq!(t("en", "noSuchKey"), "noSuchKey");
    }

    #[test]
    fn every_language_in_the_picker_has_a_bundle() {
        for l in LANGUAGES.iter() {
            assert!(PARSED.contains_key(l.key.as_str()), "no bundle for {}", l.key);
        }
    }
}
