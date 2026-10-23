//! Where converted files go and what they are called. A destination is a named folder plus an
//! optional file-name pattern; the built-in first entry writes next to the source file.
//! Pure functions only - the queue asks `resolve_output` and does the file-system work itself.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

/// Id of the built-in "same folder as the source" destination. Its display name is translated by
/// the window, so `name` is left empty for it.
pub const SOURCE_ID: &str = "source";

pub const DEFAULT_PATTERN: &str = "{name}";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Destination {
    pub id: String,
    pub name: String,
    /// Empty for the built-in source destination.
    pub folder: String,
    /// Relative path template, `/` separating sub-folders: `{artist}/{album}/{track} - {title}`.
    pub pattern: String,
    /// When a FOLDER was added, keep its structure, its own name included: `Music/Rock/a.flac`
    /// added as `Music` lands in `<folder>/Music/Rock/`, above the pattern's own path. (The built-in "same folder as the
    /// source" needs no setting - it writes beside each file, so the structure is already kept.)
    pub keep_structure: bool,
}

impl Default for Destination {
    fn default() -> Self {
        Destination {
            id: String::new(),
            name: String::new(),
            folder: String::new(),
            pattern: DEFAULT_PATTERN.to_owned(),
            keep_structure: true,
        }
    }
}

impl Destination {
    pub fn source() -> Destination {
        Destination { id: SOURCE_ID.to_owned(), ..Destination::default() }
    }

    pub fn is_source(&self) -> bool {
        self.id == SOURCE_ID
    }

    pub fn new(name: &str, folder: &str) -> Destination {
        Destination {
            id: crate::preset::new_id(),
            name: name.to_owned(),
            folder: folder.to_owned(),
            pattern: DEFAULT_PATTERN.to_owned(),
            keep_structure: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overwrite {
    /// An output that already exists is left alone and the job is reported as skipped.
    Skip,
    Overwrite,
    /// `name (2).mp3`, `name (3).mp3`, ...
    Rename,
}

/// The tag tokens a pattern may use, in the order the editor lists them.
pub const TOKENS: [&str; 9] =
    ["name", "title", "artist", "albumartist", "album", "track", "disc", "year", "genre"];

/// Make one path component safe on every platform: no separators or reserved characters, no
/// trailing dot or space, no Windows device names, bounded length.
pub fn sanitize_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_owned();
    if out.chars().count() > 120 {
        out = out.chars().take(120).collect::<String>().trim_end().to_owned();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.push('_');
    }
    out
}

fn token_value(token: &str, tags: &HashMap<String, String>, stem: &str) -> String {
    let get = |k: &str| tags.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
    match token {
        "name" => stem.to_owned(),
        "title" => get("title").unwrap_or(stem).to_owned(),
        "artist" => get("artist").or_else(|| get("album_artist")).unwrap_or("Unknown Artist").to_owned(),
        "albumartist" => get("album_artist").or_else(|| get("artist")).unwrap_or("Unknown Artist").to_owned(),
        "album" => get("album").unwrap_or("Unknown Album").to_owned(),
        "track" => match get("track").map(|t| t.split('/').next().unwrap_or("").trim()) {
            Some(n) if n.parse::<u32>().is_ok() => format!("{:02}", n.parse::<u32>().unwrap()),
            Some(n) => n.to_owned(),
            None => String::new(),
        },
        "disc" => get("disc").map(|d| d.split('/').next().unwrap_or("").trim().to_owned()).unwrap_or_default(),
        "year" => get("date").or_else(|| get("year")).map(|d| d.chars().take(4).collect()).unwrap_or_default(),
        "genre" => get("genre").unwrap_or("").to_owned(),
        _ => String::new(),
    }
}

/// Expand a pattern into a relative path (components joined with `/`). A component that comes out
/// empty (a missing tag) is dropped; if nothing is left the source file's name is used.
pub fn render_pattern(pattern: &str, tags: &HashMap<String, String>, stem: &str) -> String {
    let pattern = if pattern.trim().is_empty() { DEFAULT_PATTERN } else { pattern };
    let mut parts: Vec<String> = Vec::new();
    for raw in pattern.split(['/', '\\']) {
        let mut out = String::new();
        let mut rest = raw;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            match rest[open..].find('}') {
                Some(close) => {
                    let token = &rest[open + 1..open + close];
                    out.push_str(&sanitize_component(&token_value(token, tags, stem)));
                    rest = &rest[open + close + 1..];
                }
                None => {
                    out.push_str(&rest[open..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        let part = sanitize_component(&out);
        if !part.is_empty() {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        parts.push(sanitize_component(stem));
    }
    parts.join("/")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    Write(PathBuf),
    /// Already there and the policy says leave it.
    Skip(PathBuf),
}

fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

fn numbered(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for n in 2..10_000 {
        let name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// The folders `input` sits in, starting WITH the folder it was added through: adding `Beatles`
/// that holds `Abbey Road/a.flac` gives `Beatles/Abbey Road`. Keeping the added folder's own name
/// is what stops two added folders from merging into one. Empty when the file was added on its own.
fn structure_below(input: &Path, source_root: Option<&Path>) -> Vec<String> {
    let Some(root) = source_root else { return Vec::new() };
    let Some(rel) = input.parent().and_then(|p| p.strip_prefix(root).ok()) else { return Vec::new() };
    // A drive root (`D:\`) has no name of its own; there is nothing to prepend.
    let top = root.file_name().map(std::path::PathBuf::from).unwrap_or_default();
    top.components()
        .chain(rel.components())
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(sanitize_component(&s.to_string_lossy())),
            _ => None,
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Whether a destination folder is taken from each source file's own folder: anything that is not
/// absolute - `convert`, `.\convert`, `..\convert`. An absolute path (`D:\Out`, a share) is one place
/// for every file; so is a path with a root but no drive (`\Out`), which the system resolves itself.
pub fn is_relative_folder(folder: &Path) -> bool {
    !folder.as_os_str().is_empty() && !folder.is_absolute() && !folder.has_root()
}

/// The folder a destination names, for one source file. Empty: the source's own folder. Absolute: itself.
/// Relative: counted from the source file's folder - `.\convert` is a `convert` folder beside the file,
/// `..\convert` one beside its folder - so a single entry serves files in many folders. `.` and `..` are
/// folded away rather than left in the path, so what is shown and created is the plain folder.
pub fn base_folder(folder: &Path, input: &Path) -> PathBuf {
    let own = input.parent().map(Path::to_path_buf).unwrap_or_default();
    if folder.as_os_str().is_empty() {
        return own;
    }
    if !is_relative_folder(folder) {
        return folder.to_path_buf();
    }
    // `C:convert` (a drive but no root) is relative to that drive's current folder: not ours to fold.
    if matches!(folder.components().next(), Some(Component::Prefix(_))) {
        return folder.to_path_buf();
    }
    let mut out = own;
    for part in folder.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            _ => {}
        }
    }
    out
}

/// Decide the output path for `input`. Never returns the input itself: converting a FLAC to FLAC
/// beside the source would otherwise overwrite the original while ffmpeg is reading it.
///
/// `source_root` is the folder `input` was added through (None for a file added on its own).
pub fn resolve_output(
    dest: &Destination,
    input: &Path,
    source_root: Option<&Path>,
    tags: &HashMap<String, String>,
    extension: &str,
    overwrite: Overwrite,
) -> Resolved {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".to_owned());
    // A relative folder (`convert`, `.\convert`, `..\convert`) is counted from each file's own folder, so
    // it keeps no structure either: the folders the files sit in already are the structure.
    let folder = if dest.is_source() { Path::new("") } else { Path::new(dest.folder.trim()) };
    let base = base_folder(folder, input);
    let relative = is_relative_folder(folder);
    let pattern = if dest.is_source() { DEFAULT_PATTERN } else { dest.pattern.as_str() };
    let rel = render_pattern(pattern, tags, &stem);

    let mut path = base;
    if dest.keep_structure && !dest.is_source() && !relative {
        for part in structure_below(input, source_root) {
            path.push(part);
        }
    }
    let mut segments = rel.split('/').peekable();
    while let Some(seg) = segments.next() {
        if segments.peek().is_some() {
            path.push(seg);
        } else {
            // The extension is appended, never swapped in: a title may legitimately contain a dot.
            path.push(format!("{seg}.{extension}"));
        }
    }

    if same_file(&path, input) {
        return Resolved::Write(numbered(&path));
    }
    if !path.exists() {
        return Resolved::Write(path);
    }
    match overwrite {
        Overwrite::Skip => Resolved::Skip(path),
        Overwrite::Overwrite => Resolved::Write(path),
        Overwrite::Rename => Resolved::Write(numbered(&path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn a_relative_folder_is_counted_from_each_files_own_folder() {
        let dir = std::env::temp_dir().join(format!("yaiol-dest-rel-{}", std::process::id()));
        let a = dir.join("a").join("song.flac");
        let b = dir.join("b").join("deep").join("song.flac");
        let dest = |f: &str| Destination { id: "x".into(), folder: f.into(), keep_structure: true, ..Destination::default() };
        let out = |d: &Destination, i: &Path, root: Option<&Path>| match resolve_output(d, i, root, &tags(&[]), "mp3", Overwrite::Skip) {
            Resolved::Write(p) | Resolved::Skip(p) => p,
        };
        assert_eq!(out(&dest(".\\convert"), &a, None), dir.join("a").join("convert").join("song.mp3"));
        assert_eq!(out(&dest(".\\convert"), &b, None), dir.join("b").join("deep").join("convert").join("song.mp3"));
        assert_eq!(out(&dest("..\\convert"), &b, None), dir.join("b").join("convert").join("song.mp3"));
        assert_eq!(out(&dest("convert"), &a, None), dir.join("a").join("convert").join("song.mp3"));
        // An added folder's structure is not repeated under a relative folder.
        let root = dir.join("Music");
        let deep = root.join("Rock").join("song.flac");
        assert_eq!(out(&dest("convert"), &deep, Some(&root)), root.join("Rock").join("convert").join("song.mp3"));
    }

    #[test]
    fn parent_references_are_folded_and_an_absolute_folder_is_left_alone() {
        let base = std::env::temp_dir();
        let file = base.join("a").join("b").join("f.flac");
        assert_eq!(base_folder(Path::new("../../x"), &file), base.join("x"));
        assert_eq!(base_folder(&base.join("Out"), &file), base.join("Out"));
        assert!(!is_relative_folder(Path::new("")));
    }

    #[test]
    fn sanitizing_removes_what_a_file_system_refuses() {
        assert_eq!(sanitize_component("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(sanitize_component("name. "), "name");
        assert_eq!(sanitize_component("CON"), "CON_");
        assert_eq!(sanitize_component("com1.txt"), "com1.txt_");
        assert_eq!(sanitize_component(&"x".repeat(300)).len(), 120);
    }

    #[test]
    fn a_pattern_expands_tags_and_pads_the_track() {
        let t = tags(&[("artist", "Björk"), ("album", "Homogenic"), ("title", "Jóga"), ("track", "3/10")]);
        assert_eq!(
            render_pattern("{artist}/{album}/{track} - {title}", &t, "src"),
            "Björk/Homogenic/03 - Jóga"
        );
    }

    #[test]
    fn missing_tags_fall_back_instead_of_leaving_holes() {
        let t = tags(&[]);
        assert_eq!(render_pattern("{artist}/{album}/{title}", &t, "song"), "Unknown Artist/Unknown Album/song");
        assert_eq!(render_pattern("{year}/{name}", &t, "song"), "song");
        assert_eq!(render_pattern("", &t, "song"), "song");
    }

    #[test]
    fn a_tag_cannot_smuggle_a_folder_into_the_path() {
        let t = tags(&[("artist", "../../etc"), ("title", "x")]);
        let r = render_pattern("{artist}/{title}", &t, "s");
        // No component may BE a parent reference; a name that merely contains dots is harmless.
        assert!(r.split('/').all(|c| c != ".." && c != "."), "{r}");
        assert_eq!(r.split('/').count(), 2);
    }

    #[test]
    fn output_never_lands_on_the_input() {
        let dir = std::env::temp_dir().join(format!("yaiol-dest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("a.flac");
        std::fs::write(&input, b"x").unwrap();
        let r = resolve_output(&Destination::source(), &input, None, &tags(&[]), "flac", Overwrite::Overwrite);
        assert_eq!(r, Resolved::Write(dir.join("a (2).flac")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn overwrite_policies() {
        let dir = std::env::temp_dir().join(format!("yaiol-dest2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("a.wav");
        std::fs::write(&input, b"x").unwrap();
        std::fs::write(dir.join("a.mp3"), b"old").unwrap();
        let d = Destination::source();
        assert_eq!(resolve_output(&d, &input, None, &tags(&[]), "mp3", Overwrite::Skip), Resolved::Skip(dir.join("a.mp3")));
        assert_eq!(resolve_output(&d, &input, None, &tags(&[]), "mp3", Overwrite::Overwrite), Resolved::Write(dir.join("a.mp3")));
        assert_eq!(resolve_output(&d, &input, None, &tags(&[]), "mp3", Overwrite::Rename), Resolved::Write(dir.join("a (2).mp3")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folders_sub_folders_are_kept_under_the_destination() {
        let d = Destination::new("m", "D:/Out");
        let root = Path::new("/in/Music");
        let nested = Path::new("/in/Music/Rock/CD 1/a.flac");
        let flat = Path::new("/in/Music/b.flac");
        let t = tags(&[]);
        let w = |p: &Path, d: &Destination| match resolve_output(d, p, Some(root), &t, "mp3", Overwrite::Overwrite) {
            Resolved::Write(p) => p,
            other => panic!("{other:?}"),
        };
        assert_eq!(w(nested, &d), PathBuf::from("D:/Out").join("Music").join("Rock").join("CD 1").join("a.mp3"));
        assert_eq!(w(flat, &d), PathBuf::from("D:/Out").join("Music").join("b.mp3"));
        // Off: everything lands flat.
        let off = Destination { keep_structure: false, ..d.clone() };
        assert_eq!(w(nested, &off), PathBuf::from("D:/Out").join("a.mp3"));
        // A file added on its own has no root, so there is nothing to keep.
        let alone = resolve_output(&d, nested, None, &t, "mp3", Overwrite::Overwrite);
        assert_eq!(alone, Resolved::Write(PathBuf::from("D:/Out").join("a.mp3")));
    }

    #[test]
    fn the_kept_structure_sits_above_the_pattern_s_own_folders() {
        let d = Destination { pattern: "{artist}/{title}".into(), ..Destination::new("m", "D:/Out") };
        let t = tags(&[("artist", "Me"), ("title", "Song")]);
        let r = resolve_output(&d, Path::new("/in/Lib/Live/x.wav"), Some(Path::new("/in/Lib")), &t, "mp3", Overwrite::Overwrite);
        assert_eq!(r, Resolved::Write(PathBuf::from("D:/Out").join("Lib").join("Live").join("Me").join("Song.mp3")));
    }

    #[test]
    fn two_added_folders_stay_two_folders() {
        let d = Destination::new("m", "D:/Out");
        let t = tags(&[]);
        let w = |input: &str, root: &str| match resolve_output(&d, Path::new(input), Some(Path::new(root)), &t, "mp3", Overwrite::Overwrite) {
            Resolved::Write(p) => p,
            other => panic!("{other:?}"),
        };
        // The same file name in two albums, each added as its own folder.
        let a = w("/lib/Album A/01.flac", "/lib/Album A");
        let b = w("/lib/Album B/01.flac", "/lib/Album B");
        assert_ne!(a, b);
        assert_eq!(a, PathBuf::from("D:/Out").join("Album A").join("01.mp3"));
    }

    #[test]
    fn a_destination_saved_before_the_setting_existed_keeps_the_structure() {
        // serde fills a missing field from Default, which is true.
        let d: Destination = serde_json::from_str(r#"{ "id": "x", "name": "n", "folder": "f" }"#).unwrap();
        assert!(d.keep_structure);
    }

    #[test]
    fn a_custom_folder_and_pattern() {
        let d = Destination { folder: "D:/Music".into(), pattern: "{artist}/{title}".into(), ..Destination::new("m", "D:/Music") };
        let t = tags(&[("artist", "Me"), ("title", "Song")]);
        let r = resolve_output(&d, Path::new("/in/x.wav"), None, &t, "mp3", Overwrite::Overwrite);
        assert_eq!(r, Resolved::Write(PathBuf::from("D:/Music").join("Me").join("Song.mp3")));
    }
}
