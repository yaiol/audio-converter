//! Tag cleaning: which tags (and whether the cover) are left out of every output.
//!
//! It is its own object and not a part of a preset on purpose. A preset says how a file is
//! ENCODED; a cleanup says what METADATA is dropped, and the two are chosen independently - "Opus 128
//! with the Suno clean-up" and "Opus 128 keeping everything" are one preset and two cleanups, not
//! two presets. Destinations are separate from presets for the same reason.

use serde::{Deserialize, Serialize};

/// The id the "keep everything" entry had while it was a fixed built-in nobody could edit. It is an
/// ordinary entry now (`STANDARD_ID`); a saved selection of this id is moved onto it when settings load.
pub const KEEP_ID: &str = "keep";

/// The id of the entry a fresh install starts with: "Keep all tags", edited and deleted like any other.
pub const STANDARD_ID: &str = "default-keep";

/// What happens to the tags that are NOT in the exceptions list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanupMode {
    /// Keep every tag; the exceptions are the ones to delete.
    #[default]
    Keep,
    /// Delete every tag; the exceptions are the ones to keep.
    Delete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagCleanup {
    pub id: String,
    pub name: String,
    pub mode: CleanupMode,
    /// The exceptions to `mode`: one name per entry, case-insensitive, `*` is a wildcard. A name
    /// matches whichever way it is spelled (Mp3tag name, ID3 frame id, Vorbis name, custom field).
    /// With `Keep` they are the tags deleted, with `Delete` the tags kept. (Stored as `remove_tags`
    /// before the mode existed, when the list could only mean "delete these".)
    #[serde(alias = "remove_tags")]
    pub exceptions: Vec<String>,
    /// Leave the cover picture out. A switch of its own: the cover is a picture, not a tag name.
    pub remove_cover: bool,
    /// Read only, to carry a setting saved by a build that had a "remove all tags" switch: it becomes
    /// `mode = Delete` in `normalized` and is never written again.
    #[serde(default, skip_serializing)]
    pub(crate) remove_all: bool,
}

impl Default for TagCleanup {
    fn default() -> Self {
        TagCleanup::keep()
    }
}

impl TagCleanup {
    /// The entry a fresh install starts with: keeps everything, an ordinary entry the user may edit.
    pub fn standard(name: &str) -> Self {
        TagCleanup { id: STANDARD_ID.to_owned(), ..TagCleanup::new(name) }
    }

    /// "Nothing is removed" as a value, for code that has no entry to hand (tests, examples).
    pub fn keep() -> Self {
        TagCleanup {
            id: KEEP_ID.to_owned(),
            name: String::new(),
            mode: CleanupMode::Keep,
            exceptions: Vec::new(),
            remove_cover: false,
            remove_all: false,
        }
    }

    pub fn new(name: &str) -> Self {
        TagCleanup {
            id: crate::preset::new_id(),
            name: name.to_owned(),
            mode: CleanupMode::Keep,
            exceptions: Vec::new(),
            remove_cover: false,
            remove_all: false,
        }
    }

    /// True when it would change nothing.
    pub fn removes_nothing(&self) -> bool {
        !self.remove_cover && !self.edits_tags()
    }

    /// True when some tag is dropped: everything (`Delete`) or at least one named exception (`Keep`).
    pub fn edits_tags(&self) -> bool {
        self.mode == CleanupMode::Delete || self.exceptions.iter().any(|t| !t.trim().is_empty())
    }

    /// Is a field known under `names` (every spelling it answers to) dropped? In `Keep` mode when an
    /// exception names it, in `Delete` mode unless one does.
    pub fn drops(&self, names: &[String]) -> bool {
        let named = crate::tags::listed(&self.exceptions, names);
        match self.mode {
            CleanupMode::Keep => named,
            CleanupMode::Delete => !named,
        }
    }

    /// A setting saved by the build that had a "remove all tags" switch, read in today's shape. The
    /// exceptions text is left exactly as the user typed it.
    pub fn migrated(mut self) -> Self {
        if self.remove_all {
            self.remove_all = false;
            self.mode = CleanupMode::Delete;
            self.exceptions.clear();
        }
        self
    }

    /// The list as it is USED: trimmed, blanks and repeats dropped, the user's order and spelling kept.
    /// A `;` separates names the same way a line break does. It is applied when a job runs and never
    /// to what is stored, so the box shows again what was typed, `;` included.
    pub fn normalized(self) -> Self {
        let mut this = self.migrated();
        let mut seen: Vec<String> = Vec::new();
        for t in std::mem::take(&mut this.exceptions).iter().flat_map(|e| e.split(';')) {
            let t = t.trim().to_owned();
            if !t.is_empty() && !seen.iter().any(|s| s.eq_ignore_ascii_case(&t)) {
                seen.push(t);
            }
        }
        this.exceptions = seen;
        this
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_removal_list_is_tidied() {
        let mut c = TagCleanup::new("t");
        c.exceptions = vec!["  COMMENT ".into(), "".into(), "comment".into(), "ENCODER*".into()];
        assert_eq!(c.normalized().exceptions, ["COMMENT", "ENCODER*"]);
    }

    #[test]
    fn a_semicolon_separates_names_like_a_line_break() {
        let mut c = TagCleanup::new("t");
        c.exceptions = vec!["COMMENT; ARTISTS;encoder*".into(), "comment".into()];
        assert_eq!(c.normalized().exceptions, ["COMMENT", "ARTISTS", "encoder*"]);
    }

    #[test]
    fn the_mode_decides_what_an_exception_means() {
        let names = |n: &str| vec![n.to_owned()];
        let mut c = TagCleanup::new("t");
        c.exceptions = vec!["COMMENT".into()];
        // Keep everything, except the listed.
        assert!(c.drops(&names("comment")));
        assert!(!c.drops(&names("title")));
        // Delete everything, except the listed.
        c.mode = CleanupMode::Delete;
        assert!(!c.drops(&names("comment")));
        assert!(c.drops(&names("title")));
    }

    #[test]
    fn delete_with_no_exception_edits_tags_and_keep_with_none_does_not() {
        let mut c = TagCleanup::new("t");
        assert!(!c.edits_tags());
        c.mode = CleanupMode::Delete;
        assert!(c.edits_tags());
        assert!(!c.removes_nothing());
    }

    #[test]
    fn a_saved_remove_all_and_remove_tags_are_read_in_the_new_shape() {
        let old: TagCleanup = serde_json::from_str(r#"{"id":"x","name":"n","remove_tags":["COMMENT"]}"#).unwrap();
        assert_eq!(old.mode, CleanupMode::Keep);
        assert_eq!(old.exceptions, ["COMMENT"]);
        let all: TagCleanup = serde_json::from_str(r#"{"id":"x","name":"n","remove_all":true}"#).unwrap();
        let all = all.normalized();
        assert_eq!(all.mode, CleanupMode::Delete);
        assert!(serde_json::to_string(&all).unwrap().contains("\"mode\":\"delete\""));
    }

    #[test]
    fn the_default_removes_nothing() {
        assert!(TagCleanup::default().removes_nothing());
        assert_eq!(TagCleanup::default().id, KEEP_ID);
        let mut c = TagCleanup::new("t");
        c.remove_cover = true;
        assert!(!c.removes_nothing());
    }
}
