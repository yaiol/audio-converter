//! Persisted user settings, in `settings.json` under the OS app-data folder: language, theme, the
//! skipped update, and everything the converter remembers (presets, saved destinations, which are
//! selected, how many jobs run at once, what to do when an output exists).
//!
//! The window and the command line read and write THE SAME FILE, so a preset made in one is a preset
//! in the other. It lives in the engine for that reason; the product name (which names the folder)
//! is the caller's.
//!
//! The folder mirrors the electron convention: `<app data>/yaiol/<Product Name>`, with a ` (Dev)`
//! suffix on a debug build so a dev run and an installed copy never share state. The file name is
//! plain (`settings.json`) - the folder is already per-app, so there is no prefix to namespace it.

use crate::{
    Destination, Overwrite, Preset, TagCleanup,
    preset::ORIGINAL_ID,
    cleanup::{KEEP_ID, STANDARD_ID},
    destination::SOURCE_ID,
    effects::{EffectsEntry, STANDARD_ID as EFFECTS_STANDARD_ID},
};
use crate::i18n;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The "(None)" choice of the tags and effects selectors: no stored entry, it keeps every tag / applies
/// no effect. Like "(Original)" for the preset, it is offered by the window and never listed in Settings.
pub const NONE_ID: &str = "none";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: String,
    pub theme: String,
    pub presets: Vec<Preset>,
    /// The user's saved destinations. The built-in "same folder as the source" is NOT stored here;
    /// it is always offered first.
    pub destinations: Vec<Destination>,
    /// The user's tag-cleaning entries. The built-in "keep everything" is NOT stored here; it is
    /// always offered first.
    pub cleanups: Vec<TagCleanup>,
    /// The effects chains, chosen apart from the preset like the tag cleaning. Never zero: a fresh
    /// install holds one ordinary entry that changes nothing.
    pub effects: Vec<EffectsEntry>,
    pub preset_id: String,
    pub destination_id: String,
    pub cleanup_id: String,
    pub effects_id: String,
    /// Conversions running at once.
    pub parallel: u32,
    pub overwrite: Overwrite,
    /// Show an operating-system notice when a run of conversions ends.
    pub notify: bool,
}

/// The most conversions that can be asked for, and the default: the PHYSICAL cores. Encoding is
/// CPU-bound, so the second hardware thread of a core adds little and the processes compete for the
/// same core, memory and disk - measured on an 8-core / 16-thread machine, 16 at once took 45 s
/// against 39 s for 8, so nothing above the physical count is offered.
pub fn max_parallel() -> u32 {
    (num_cpus::get_physical() as u32).max(1)
}

impl Default for Settings {
    fn default() -> Self {
        let presets = Preset::defaults();
        let preset_id = presets[0].id.clone();
        Self {
            lang: "en".to_owned(),
            theme: "light".to_owned(),
           
            presets,
            destinations: Vec::new(),
            cleanups: vec![TagCleanup::standard(&i18n::t("en", "lblMainCleanupKeep"))],
            effects: vec![EffectsEntry::standard(&i18n::t("en", "lblMainEffectsNone"))],
            preset_id,
            destination_id: SOURCE_ID.to_owned(),
            cleanup_id: STANDARD_ID.to_owned(),
            effects_id: EFFECTS_STANDARD_ID.to_owned(),
            parallel: max_parallel(),
            overwrite: Overwrite::Skip,
            notify: true,
        }
    }
}

impl Settings {
    /// Repair what a hand-edited or older file can get wrong: never zero presets, a selection that
    /// points at something that exists, presets inside what their format accepts.
    pub fn repaired(mut self) -> Self {
        if self.presets.is_empty() {
            self.presets = Preset::defaults();
        }
        self.presets = self.presets.into_iter().map(Preset::normalized).collect();
        if self.preset_id != ORIGINAL_ID && !self.presets.iter().any(|p| p.id == self.preset_id) {
            self.preset_id = self.presets[0].id.clone();
        }
        if self.destination_id != SOURCE_ID && !self.destinations.iter().any(|d| d.id == self.destination_id) {
            self.destination_id = SOURCE_ID.to_owned();
        }
        self.cleanups = self.cleanups.into_iter().map(TagCleanup::migrated).collect();
        // Never zero entries, as with presets. The old fixed "keep everything" row is an ordinary
        // entry now: a file that still selects it gets one, placed first as that row was.
        let legacy = self.cleanup_id == KEEP_ID;
        if (legacy || self.cleanups.is_empty()) && !self.cleanups.iter().any(|c| c.id == STANDARD_ID) {
            let name = i18n::t(&self.lang, "lblMainCleanupKeep");
            self.cleanups.insert(0, TagCleanup::standard(&name));
        }
        if legacy {
            self.cleanup_id = STANDARD_ID.to_owned();
        }
        if self.cleanup_id != NONE_ID && !self.cleanups.iter().any(|c| c.id == self.cleanup_id) {
            self.cleanup_id = self.cleanups[0].id.clone();
        }
        self.effects = self.effects.into_iter().map(EffectsEntry::normalized).collect();
        if self.effects.is_empty() {
            let name = i18n::t(&self.lang, "lblMainEffectsNone");
            self.effects.push(EffectsEntry::standard(&name));
        }
        if self.effects_id != NONE_ID && !self.effects.iter().any(|e| e.id == self.effects_id) {
            self.effects_id = self.effects[0].id.clone();
        }
        self.parallel = self.parallel.clamp(1, max_parallel());
        self
    }

    /// A preset by id; "(Original)" is the rule preset (`Preset::is_original`).
    pub fn preset_by(&self, id: &str) -> Preset {
        if id == ORIGINAL_ID {
            return Preset::original();
        }
        self.presets.iter().find(|p| p.id == id).unwrap_or(&self.presets[0]).clone()
    }

    pub fn cleanup_by(&self, id: &str) -> TagCleanup {
        if id == NONE_ID {
            return TagCleanup::keep();
        }
        self.cleanups.iter().find(|c| c.id == id).or(self.cleanups.first()).cloned().unwrap_or_else(TagCleanup::keep)
    }

    pub fn effects_by(&self, id: &str) -> EffectsEntry {
        if id == NONE_ID {
            return EffectsEntry::default();
        }
        self.effects.iter().find(|e| e.id == id).or(self.effects.first()).cloned().unwrap_or_default()
    }

    pub fn destination_by(&self, id: &str) -> Destination {
        self.destinations.iter().find(|d| d.id == id).cloned().unwrap_or_else(Destination::source)
    }

    pub fn selected_preset(&self) -> Preset {
        self.preset_by(&self.preset_id)
    }

    pub fn selected_cleanup(&self) -> TagCleanup {
        self.cleanup_by(&self.cleanup_id)
    }

    pub fn selected_effects(&self) -> EffectsEntry {
        self.effects_by(&self.effects_id)
    }

    pub fn selected_destination(&self) -> Destination {
        self.destination_by(&self.destination_id)
    }

    /// Everything a run is made of, as the panel shows it now.
    pub fn choice(&self) -> RunChoice {
        RunChoice {
            preset_id: self.preset_id.clone(),
            cleanup_id: self.cleanup_id.clone(),
            effects_id: self.effects_id.clone(),
            destination_id: self.destination_id.clone(),
            overwrite: self.overwrite,
        }
    }
}

/// The complete run settings of ONE file: the panel's choices when they were applied to it. Entries are
/// kept by id, so editing a preset in Settings changes every file that uses it.
#[derive(Clone, PartialEq, Debug)]
pub struct RunChoice {
    pub preset_id: String,
    pub cleanup_id: String,
    pub effects_id: String,
    pub destination_id: String,
    pub overwrite: Overwrite,
}

/// The settings file of the product called `product` (its display name, e.g. "Audio Converter").
fn path(product: &str) -> Option<PathBuf> {
    let folder = if cfg!(debug_assertions) { format!("{product} (Dev)") } else { product.to_owned() };
    Some(dirs::data_dir()?.join("yaiol").join(folder).join("settings.json"))
}

/// Where the settings of `product` are kept, for a message that has to name the file.
pub fn location(product: &str) -> Option<PathBuf> {
    path(product)
}

pub fn load(product: &str) -> Settings {
    path(product).map(|p| load_from(&p)).unwrap_or_default()
}

pub fn save(product: &str, settings: &Settings) {
    if let Some(p) = path(product) {
        save_to(&p, settings);
    }
}

pub fn load_from(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
        .repaired()
}

pub fn save_to(path: &Path, settings: &Settings) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("yaiol-settings-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn round_trips() {
        let p = temp("roundtrip").join("settings.json");
        let mut s = Settings { lang: "ar".to_owned(), theme: "dark".to_owned(), ..Settings::default() };
        s.destinations.push(Destination::new("Music", "D:/Music"));
        s.destination_id = s.destinations[0].id.clone();
        save_to(&p, &s);
        assert!(load_from(&p) == s);
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_choice_is_the_panel_as_it_is_and_resolves_by_id() {
        let s = Settings { overwrite: Overwrite::Rename, ..Settings::default() };
        let c = s.choice();
        assert_eq!((c.preset_id.as_str(), c.destination_id.as_str(), c.overwrite), (s.preset_id.as_str(), SOURCE_ID, Overwrite::Rename));
        assert_eq!(s.preset_by(&c.preset_id).id, s.selected_preset().id);
        assert!(s.preset_by(ORIGINAL_ID).is_original());
        assert!(s.destination_by("gone").is_source());
        assert!(s.effects_by(NONE_ID).items.is_empty());
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        assert!(load_from(&temp("missing")) == Settings::default());
        let p = temp("broken");
        fs::write(&p, "{ not json").unwrap();
        assert!(load_from(&p) == Settings::default());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn a_partial_file_keeps_the_rest_at_defaults() {
        let p = temp("partial");
        fs::write(&p, r#"{ "theme": "dark" }"#).unwrap();
        let s = load_from(&p);
        assert_eq!((s.lang.as_str(), s.theme.as_str()), ("en", "dark"));
        assert!(!s.presets.is_empty());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn the_old_fixed_keep_row_becomes_an_ordinary_first_entry() {
        let p = temp("legacy-keep");
        fs::write(&p, r#"{ "cleanup_id": "keep", "cleanups": [ { "id": "mine", "name": "Mine", "remove_tags": ["COMMENT"] } ] }"#).unwrap();
        let s = load_from(&p);
        assert_eq!(s.cleanups.len(), 2);
        assert_eq!(s.cleanups[0].id, STANDARD_ID);
        assert_eq!(s.cleanup_id, STANDARD_ID);
        assert_eq!(s.cleanups[1].id, "mine");
        assert_eq!(s.cleanups[1].exceptions, ["COMMENT"]);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn the_three_window_only_choices_are_kept_and_mean_what_they_say() {
        let p = temp("sentinels");
        fs::write(&p, r#"{ "preset_id": "original", "cleanup_id": "none", "effects_id": "none" }"#).unwrap();
        let s = load_from(&p);
        assert_eq!((s.preset_id.as_str(), s.cleanup_id.as_str(), s.effects_id.as_str()), ("original", "none", "none"));
        assert!(s.selected_preset().is_original());
        assert!(!s.selected_cleanup().edits_tags() && !s.selected_cleanup().remove_cover);
        assert!(s.selected_effects().items.is_empty());
        // They are not entries of the lists the Settings editors show.
        assert!(!s.presets.iter().any(|x| x.is_original()));
        assert!(s.cleanups.iter().all(|c| c.id != "none") && s.effects.iter().all(|e| e.id != "none"));
        let _ = fs::remove_file(p);
    }

    #[test]
    fn a_selection_that_no_longer_exists_is_repaired() {
        let p = temp("dangling");
        fs::write(&p, r#"{ "preset_id": "gone", "destination_id": "gone", "cleanup_id": "gone", "presets": [], "parallel": 0 }"#).unwrap();
        let s = load_from(&p);
        assert!(s.presets.iter().any(|x| x.id == s.preset_id));
        assert_eq!(s.destination_id, SOURCE_ID);
        assert!(s.cleanups.iter().any(|c| c.id == s.cleanup_id), "the selection is an entry that exists");
        assert_eq!(s.parallel, 1);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn an_older_file_gets_the_ordinary_effects_entry_and_a_dangling_one_is_repaired() {
        let p = temp("effects");
        // A file written before effects existed: no key at all.
        fs::write(&p, r#"{ "theme": "dark" }"#).unwrap();
        let s = load_from(&p);
        assert_eq!(s.effects.len(), 1);
        assert_eq!(s.effects[0].id, EFFECTS_STANDARD_ID);
        assert!(!s.effects[0].needs_analysis(), "the ordinary entry changes nothing");
        assert_eq!(s.selected_effects().id, EFFECTS_STANDARD_ID);
        // Everything deleted, and a selection that points nowhere: there is still one entry, selected.
        fs::write(&p, r#"{ "effects": [], "effects_id": "gone" }"#).unwrap();
        let s = load_from(&p);
        assert_eq!(s.effects.len(), 1);
        assert_eq!(s.effects_id, s.effects[0].id);
        // Out-of-range numbers are clamped when the file is read.
        fs::write(&p, r#"{ "effects": [ { "id": "x", "name": "X", "items": [ { "silence_end": { "seconds": 9999 } } ] } ], "effects_id": "x" }"#).unwrap();
        let s = load_from(&p);
        assert_eq!(s.effects[0].silence_end().unwrap().seconds, 60.0);
        let _ = fs::remove_file(p);
    }
}
