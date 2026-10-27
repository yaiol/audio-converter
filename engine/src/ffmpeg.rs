//! Everything that touches the ffmpeg executable: finding it, asking it about a file, building the
//! command line, and reading its progress. ffmpeg is a separate program - never linked, never shipped with the
//! app, never the machine's own: the build yaiol approved, downloaded into the folder every yaiol app shares.
//!
//! ⚠ Nothing here may assume which build it is: the arguments are the same for a GPL and an LGPL one.

use crate::{cleanup::{CleanupMode, TagCleanup}, destination::Destination, format::Format, preset::Preset};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn executable_name() -> &'static str {
    if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }
}

/// The ffmpeg to use: the one the app installed into `dir`, and no other. PATH is never searched, nothing is
/// shipped beside the app, and a program in `dir` that no install recorded (one put there by hand) is not a build
/// we know.
pub fn locate(dir: Option<&Path>) -> Option<PathBuf> {
    let dir = dir?;
    crate::download::installed_version(dir)?;
    Some(dir.join(executable_name()))
}

/// Whether this ffmpeg was built with non-free parts (`--enable-nonfree`); `None` when it does not run.
pub fn is_nonfree(ffmpeg: &Path) -> Option<bool> {
    let out = command(ffmpeg).arg("-version").stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).contains("--enable-nonfree"))
}

/// The ffmpeg the tests that run a real one use: whatever is on PATH, or nothing (the test then skips). The app
/// itself never looks on PATH; a test machine's own ffmpeg is a convenience for the tests only.
#[cfg(test)]
pub(crate) fn ffmpeg_for_tests() -> Option<PathBuf> {
    let name = executable_name();
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

/// A `Command` that does not flash a console window on Windows.
pub fn command(ffmpeg: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// What a conversion needs to know about HOW the source holds its tags, beyond the tags themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceKind {
    /// The tags sit on the audio STREAM and the file itself carries none (an Ogg Vorbis or Opus
    /// file). `-map_metadata 0` copies only the file's own tags, so such a source needs the stream's
    /// copied as well or every tag is lost.
    pub tags_on_stream: bool,
    /// An ID3 source (an MP3, or a WAV or AIFF with an ID3 chunk). In it ffmpeg's key `performer` is
    /// the frame `TPE3` - the conductor - where a Vorbis file's own `PERFORMER` field arrives under
    /// the same key.
    pub id3: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub duration_us: u64,
    /// The first audio stream's codec as ffmpeg names it (`flac`, `mp3`, `pcm_s16le`...); empty
    /// when none could be read.
    pub codec: String,
    /// The file's overall bitrate, when it states one.
    pub bitrate_kbps: Option<u32>,
    /// Lower-cased keys: title, artist, album, album_artist, track, date, genre, ...
    pub tags: HashMap<String, String>,
    /// How the source holds its tags.
    pub source: SourceKind,
}

fn parse_duration(s: &str) -> Option<u64> {
    // 00:03:41.23
    let mut it = s.trim().split(':');
    let h: u64 = it.next()?.parse().ok()?;
    let m: u64 = it.next()?.parse().ok()?;
    let sec: f64 = it.next()?.parse().ok()?;
    Some((h * 3600 + m * 60) * 1_000_000 + (sec * 1_000_000.0) as u64)
}

/// Fields a stream block carries that are not tags. Everything else in it is read as a tag - Ogg
/// Vorbis and Opus keep ALL their comments there, custom ones included.
const STREAM_NOT_TAGS: [&str; 10] = [
    "encoder",
    "handler_name",
    "vendor_id",
    "language",
    "creation_time",
    "major_brand",
    "minor_version",
    "compatible_brands",
    "duration",
    "timecode",
];

/// What the CONTAINER reports about itself and ffmpeg lists beside the tags: not fields of the file.
/// An M4A's brands were written into an M4A output as free-form atoms (`MAJOR_BRAND`...), and a
/// WMA's `creation_time` (1970) reached an ID3 file as a custom frame.
const CONTAINER_NOT_TAGS: [&str; 4] = ["creation_time", "major_brand", "minor_version", "compatible_brands"];

/// Vorbis spellings mapped to the names the rest of the app uses.
fn canonical_tag(key: &str) -> &str {
    match key {
        "tracknumber" => "track",
        "discnumber" => "disc",
        "albumartist" => "album_artist",
        "year" => "date",
        other => other,
    }
}

/// Read the duration and the tags out of the text `ffmpeg -i <file>` prints: the container's own
/// `Metadata:` block first, then - for formats that keep tags on the stream - the first audio
/// stream's, filling only what the container did not give.
pub fn parse_probe(stderr: &str) -> Probe {
    let mut probe = Probe::default();
    let mut in_tags = false;
    let mut in_audio_stream = false;
    let mut audio_seen = false;
    let mut in_stream_tags = false;
    let (mut container_tags, mut stream_tags) = (0usize, 0usize);
    let mut id3_container = false;
    for line in stderr.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if let Some(rest) = text.strip_prefix("Input #") {
            // `Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'x.m4a':` - the demuxer(s) ffmpeg chose.
            id3_container = rest
                .split(',')
                .skip(1)
                .take_while(|p| !p.trim_start().starts_with("from "))
                .any(|c| matches!(c.trim(), "mp3" | "wav" | "aiff"));
        }
        if text.starts_with("Stream #") {
            in_tags = false;
            in_stream_tags = false;
            in_audio_stream = text.contains("Audio:") && !audio_seen;
            if in_audio_stream {
                audio_seen = true;
                probe.codec = text
                    .split("Audio:")
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_owned();
            }
            continue;
        }
        if in_audio_stream && indent == 4 && text == "Metadata:" {
            in_stream_tags = true;
            continue;
        }
        if in_stream_tags {
            if indent > 4 {
                if let Some((k, v)) = text.split_once(':') {
                    let key = k.trim().to_ascii_lowercase();
                    let val = v.trim();
                    // `language` in a stream block is a property ffmpeg adds (an M4A's `und`); an Ogg or
                    // Opus file's own Vorbis field is written LANGUAGE, and is a tag.
                    let own_language = k.trim() == "LANGUAGE";
                    if !val.is_empty() && (own_language || !STREAM_NOT_TAGS.contains(&key.as_str())) {
                        if !probe.tags.contains_key(canonical_tag(&key)) {
                            stream_tags += 1;
                        }
                        probe.tags.entry(canonical_tag(&key).to_owned()).or_insert_with(|| val.to_owned());
                    }
                }
            } else {
                in_stream_tags = false;
            }
            continue;
        }
        if text.starts_with("Duration:") {
            in_tags = false;
            if let Some(d) = text["Duration:".len()..].split(',').next().and_then(parse_duration) {
                probe.duration_us = d;
            }
            probe.bitrate_kbps = text
                .split("bitrate:")
                .nth(1)
                .and_then(|r| r.trim().split_whitespace().next())
                .and_then(|n| n.parse::<u32>().ok());
            continue;
        }
        if indent == 2 && text == "Metadata:" {
            in_tags = true;
            continue;
        }
        if in_tags {
            if indent > 2 {
                if let Some((k, v)) = text.split_once(':') {
                    let key = k.trim().to_ascii_lowercase();
                    let val = v.trim();
                    if !key.is_empty() && !val.is_empty() && !CONTAINER_NOT_TAGS.contains(&key.as_str()) {
                        if !probe.tags.contains_key(&key) {
                            container_tags += 1;
                        }
                        probe.tags.entry(key).or_insert_with(|| val.to_owned());
                    }
                }
            } else {
                in_tags = false;
            }
        }
    }
    probe.source = SourceKind { tags_on_stream: container_tags == 0 && stream_tags > 0, id3: id3_container };
    probe
}

/// Ask ffmpeg about a file (a run with no output exits non-zero by design; only the text matters).
pub fn probe(ffmpeg: &Path, input: &Path) -> Result<Probe, String> {
    let out = command(ffmpeg)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(input)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    let p = parse_probe(&text);
    if p.duration_us == 0 && p.tags.is_empty() {
        let why = text
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| {
                l.contains("Invalid data") || l.contains("No such file") || l.contains("Error opening") || l.contains("does not contain any stream")
            })
            .unwrap_or("not an audio file ffmpeg can read");
        return Err(why.to_owned());
    }
    Ok(p)
}

/// The name ffmpeg keeps a field under, for the spellings a tagger shows: ffmpeg says `album_artist`
/// where Mp3tag says `ALBUMARTIST`. Anything else is passed through (its matching is case-insensitive).
fn ffmpeg_key(name: &str) -> String {
    let up = name.to_ascii_uppercase();
    TAGGER_TO_FFMPEG.iter().find(|(tagger, _)| *tagger == up).map_or_else(|| name.to_owned(), |(_, key)| (*key).to_owned())
}

/// The few fields ffmpeg names differently from the tagger programs: (tagger's name, ffmpeg's key).
const TAGGER_TO_FFMPEG: [(&str, &str); 6] = [
    ("ALBUMARTIST", "album_artist"),
    ("DISCNUMBER", "disc"),
    ("TRACKNUMBER", "track"),
    ("DESCRIPTION", "comment"),
    ("YEAR", "date"),
    ("ENCODEDBY", "encoded_by"),
];

/// ID3 frames that reach ffmpeg as fields of their own because it has no mapping for them (the sort
/// frames of ID3v2.4, which iTunes made common, and `TEXT`, the lyricist). ffmpeg writes such a key as
/// it is, so a WAV converted to FLAC carried `TSO2` / `TSOC` beside the `ALBUMARTISTSORT` /
/// `COMPOSERSORT` of every other file in the library. (raw id, the name the rest of the library uses.)
/// Every pair is a row of Mp3tag's mapping table, <https://docs.mp3tag.de/mapping/>. RIFF INFO ids are
/// not here: the ones Mp3tag knows ffmpeg maps itself, and the rest are dropped (`riff.rs`).
const FOREIGN_FIELDS: [(&str, &str); 6] = [
    ("TSO2", "ALBUMARTISTSORT"),
    ("TSOC", "COMPOSERSORT"),
    ("TSOP", "ARTISTSORT"),
    ("TSOA", "ALBUMSORT"),
    ("TSOT", "TITLESORT"),
    ("TEXT", "LYRICIST"),
];

/// ffmpeg's own names for fields that a Vorbis comment (FLAC, Ogg, Opus) should carry under the name
/// Mp3tag and the rest of the library use. ffmpeg writes a Vorbis field under the key it holds
/// internally, so a MP3's `TSOA` came out as `album-sort` and its `TIT1` as `grouping`. (ffmpeg's key,
/// the name written.) Only for Vorbis-comment outputs: in an ID3 or MP4 output ffmpeg maps these keys
/// to the real frame or atom itself. Not here on purpose: `performer` (ID3 `TPE3`, Mp3tag's CONDUCTOR) -
/// a FLAC source's own `PERFORMER` field reaches ffmpeg under the same key, and renaming it would turn
/// a performer into a conductor.
const VORBIS_NAMES: [(&str, &str); 10] = [
    ("album-sort", "ALBUMSORT"),
    ("artist-sort", "ARTISTSORT"),
    ("title-sort", "TITLESORT"),
    ("encoded_by", "ENCODEDBY"),
    ("grouping", "CONTENTGROUP"),
    // The same fields as an M4A source hands them over (its sort atoms).
    ("sort_album", "ALBUMSORT"),
    ("sort_artist", "ARTISTSORT"),
    ("sort_album_artist", "ALBUMARTISTSORT"),
    ("sort_name", "TITLESORT"),
    ("sort_composer", "COMPOSERSORT"),
];

/// The attributes of a WMA source, which ffmpeg hands over under their `WM/...` names - it maps only
/// the few it knows (title, artist, album...). Renamed to what a FLAC source of the same field
/// delivers, so a WMA converts like any other file instead of arriving as `TXXX:WM/Year`. Every
/// attribute of `asf::NAMES` that ffmpeg does not map is here (a test holds that).
/// (attribute, the key a source of any other format delivers.)
pub(crate) const WMA_FIELDS: [(&str, &str); 16] = [
    ("WM/Year", "date"),
    ("WM/Lyrics", "lyrics"),
    ("WM/Writer", "LYRICIST"),
    ("WM/Conductor", "CONDUCTOR"),
    ("WM/ContentGroupDescription", "CONTENTGROUP"),
    ("WM/SubTitle", "SUBTITLE"),
    ("WM/AlbumSortOrder", "ALBUMSORT"),
    ("WM/ArtistSortOrder", "ARTISTSORT"),
    ("WM/AlbumArtistSortOrder", "ALBUMARTISTSORT"),
    ("WM/TitleSortOrder", "TITLESORT"),
    ("WM/BeatsPerMinute", "BPM"),
    ("WM/InitialKey", "INITIALKEY"),
    ("WM/ISRC", "ISRC"),
    ("WM/Mood", "MOOD"),
    ("WM/OriginalReleaseYear", "ORIGYEAR"),
    ("MusicBrainz/Album Type", "RELEASETYPE"),
];

/// ID3 frames that are never renamed to a field: the date frames have a code path of their own (`date`,
/// see `tag_arguments`) and the encoder tag is ffmpeg's.
const ID3_KEPT: [&str; 5] = ["TYER", "TDAT", "TIME", "TDRC", "TSSE"];

/// `tags` with the raw spelling of every field in `table` replaced by its standard name - kept if the
/// file already has the standard name - and the list of renames made: (raw key, standard name, value).
fn rename_fields(
    tags: &HashMap<String, String>,
    table: &[(&str, &str)],
) -> (HashMap<String, String>, Vec<(String, String, String)>) {
    let mut out = tags.clone();
    let mut renamed = Vec::new();
    for (raw, standard) in table.iter().copied() {
        let Some(key) = out.keys().find(|k| k.eq_ignore_ascii_case(raw)).cloned() else { continue };
        let value = out.remove(&key).unwrap_or_default();
        let has_standard = out.keys().any(|k| k.eq_ignore_ascii_case(standard));
        if !has_standard {
            out.insert(standard.to_owned(), value.clone());
        }
        renamed.push((key, standard.to_owned(), value));
    }
    (out, renamed)
}

/// The spellings a tagger program might use for the field ffmpeg calls `key`: the key itself, its
/// upper-case form with and without underscores (`encoder_settings` -> `ENCODERSETTINGS`), and the
/// tagger's name where ffmpeg's differs.
pub(crate) fn tagger_spellings(key: &str) -> Vec<String> {
    let mut v = vec![key.to_owned(), key.to_ascii_uppercase(), key.replace('_', "").to_ascii_uppercase()];
    for (tagger, ff) in TAGGER_TO_FFMPEG {
        if key.eq_ignore_ascii_case(ff) {
            v.push(tagger.to_owned());
        }
    }
    if key.eq_ignore_ascii_case("comment") {
        v.push("COMMENT".to_owned());
    }
    v.dedup();
    v
}

/// The embedded cover picture as a small square JPEG (`size` px), or `None` when the file has no
/// picture or ffmpeg cannot read it. Cropped to fill, so every thumbnail is the same shape.
pub fn cover_jpeg(ffmpeg: &Path, input: &Path, size: u32) -> Option<Vec<u8>> {
    let filter = format!("scale={size}:{size}:force_original_aspect_ratio=increase,crop={size}:{size}");
    let out = command(ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-an", "-map", "0:v:0?", "-frames:v", "1", "-vf"])
        .arg(filter)
        .args(["-f", "mjpeg", "pipe:1"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    // A JPEG starts FF D8; anything shorter is an error message or an empty stream.
    (out.status.success() && out.stdout.len() > 100 && out.stdout.starts_with(&[0xFF, 0xD8])).then_some(out.stdout)
}

/// `out_time_us=123456` from `-progress pipe:1`. (`out_time_ms` is also microseconds, an old
/// ffmpeg quirk - `out_time_us` is the unambiguous one.)
pub fn parse_progress_line(line: &str) -> Option<u64> {
    line.trim().strip_prefix("out_time_us=")?.parse().ok()
}

/// The full argument list for one conversion.
pub fn build_args(
    input: &Path,
    output: &Path,
    preset: &Preset,
    cleanup: &TagCleanup,
    source_tags: &HashMap<String, String>,
    // Fields of the source to leave out because a tag beside them already holds the same data
    // (`riff::info_only_keys`).
    ignored: &[String],
    with_cover: bool,
    source: SourceKind,
    // The audio filters and tag fields of the effects chain (empty = none).
    effects: &crate::effects::EffectsPlan,
) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    a.push(input.to_string_lossy().into_owned());
    let cover = with_cover && preset.format.carries_cover_art() && !cleanup.remove_cover;
    if cover {
        // ⚠ CLAUDE: the picture is taken from a SECOND input - the same file again - and not mapped
        // from the first. With the picture on input 0, ffmpeg reports `out_time_us=N/A` for the whole
        // run (the single-frame video stream never advances its clock), so the progress sat at 0 until
        // the file finished (found 2026-10-13, reproduced by `progress_climbs_for_a_file_with_cover_art`).
        a.extend(["-i".to_owned(), input.to_string_lossy().into_owned()]);
    }
    a.extend(["-map", "0:a:0"].map(str::to_owned));
    if cover {
        // `?` makes the picture optional: most files have none.
        a.extend(["-map", "1:v:0?", "-c:v", "copy", "-disposition:v:0", "attached_pic"].map(str::to_owned));
    }
    a.extend(tag_arguments(preset, cleanup, source_tags, ignored, source));
    // RIFF INFO's `ISRC` is "Source" (who supplied the original material), not the recording code, and
    // Mp3tag does not write an ISRC there. The WAV's ID3 chunk (`TSRC`) carries the real one.
    if preset.format == Format::Wav {
        a.extend(["-metadata", "ISRC="].map(str::to_owned));
    }
    // The effects' filters, then the conversion to 16 bit with dither - the last thing done to the sound.
    let mut filters = effects.filters.clone();
    // ffmpeg's AAC encoder refuses a stream whose channels have a count and no layout ("Unsupported
    // channel layout '1 channels'"), which is what the TTA decoder hands over: a TTA file could not be
    // converted to AAC, M4A or a ringtone. Naming a layout of the same count fixes it, for every source.
    if preset.is_aac() {
        filters.insert(0, "aformat=channel_layouts=mono|stereo|2.1|3.0|4.0|5.0|5.1|6.1|7.1".to_owned());
    }
    filters.extend(preset.dither_filter().map(str::to_owned));
    if !filters.is_empty() {
        a.extend(["-af".to_owned(), filters.join(",")]);
    }
    // A format only ffmpeg writes has no tag pass afterwards: the effect's fields go in here. Every
    // other format gets them from the tag layer, after the cleaning.
    if crate::tags::target_tag_type(preset.format).is_none() && preset.format != Format::Wma && preset.format.holds_tags() {
        for (k, v) in &effects.tags {
            a.extend(["-metadata".to_owned(), format!("{k}={v}")]);
        }
    }
    a.extend(preset.codec_args());
    if preset.ffmpeg_writes_id3() {
        let v = if preset.id3_version == crate::preset::Id3Version::V24 { "4" } else { "3" };
        a.extend(["-id3v2_version", v].map(str::to_owned));
    }
    a.extend(["-progress", "pipe:1", "-nostats"].map(str::to_owned));
    a.push(output.to_string_lossy().into_owned());
    a
}

/// The source's fields as this output will see them: the ones to leave out removed, the foreign and
/// ffmpeg-internal names replaced by their standard ones; and the renames made (raw key, standard name,
/// value).
fn prepare_tags(
    preset: &Preset,
    source_tags: &HashMap<String, String>,
    ignored: &[String],
    source: SourceKind,
) -> (HashMap<String, String>, Vec<(String, String, String)>) {
    let mut kept = source_tags.clone();
    kept.retain(|k, _| !ignored.iter().any(|i| i == k));
    let mut table: Vec<(&str, &str)> = FOREIGN_FIELDS.to_vec();
    table.extend(WMA_FIELDS);
    // In an ID3 source `performer` IS the conductor (`TPE3`). An ID3 output turns the key back into
    // that frame itself; any other output would carry a PERFORMER field, a different thing - so it is
    // named for what it is. A Vorbis source's own PERFORMER (no ID3 in it) is left alone.
    if source.id3 && !matches!(preset.format, Format::Mp3 | Format::Aiff | Format::Wav) {
        table.push(("performer", "CONDUCTOR"));
    }
    // An ID3 frame ffmpeg has no key for reaches it under its own id (`TPE4`, `TBPM`, `TKEY`...): it
    // goes through as that frame to another ID3 file, but a FLAC or an M4A would carry a field
    // called `TBPM`. Every frame of the table is renamed to the field's name; the ID3 outputs turn
    // that back into the frame (`tags::promote_custom_frames`).
    for fd in crate::mapping::FIELDS {
        for id in [fd.id3_v23, fd.id3_v24].into_iter().flatten() {
            let known = table.iter().any(|(raw, _)| raw.eq_ignore_ascii_case(id));
            if !known && !id.contains(':') && !ID3_KEPT.contains(&id) {
                table.push((id, fd.name));
            }
        }
    }
    if preset.format.writes_vorbis_comments() {
        table.extend(VORBIS_NAMES);
    }
    rename_fields(&kept, &table)
}

/// The fields the output ends up carrying from the source, by name - what the tag layer may add to a
/// format ffmpeg drops free-form fields from (M4A). Stray date frames are not among them: the date is
/// `date`'s job (see `tag_arguments`).
pub fn effective_tags(
    preset: &Preset,
    source_tags: &HashMap<String, String>,
    ignored: &[String],
    source: SourceKind,
) -> HashMap<String, String> {
    let (mut tags, _) = prepare_tags(preset, source_tags, ignored, source);
    // A total merged into its number (see `tag_arguments`) is not a field of its own any more.
    if !preset.format.writes_vorbis_comments() && preset.format != Format::Wma {
        tags.retain(|k, _| !matches!(k.to_ascii_lowercase().as_str(), "tracktotal" | "totaltracks" | "disctotal" | "totaldiscs"));
    }
    let has_date = tags.keys().any(|k| k.eq_ignore_ascii_case("date"));
    for stray in ["TYER", "TDAT", "TIME"] {
        if let Some(key) = tags.keys().find(|k| k.eq_ignore_ascii_case(stray)).cloned() {
            let value = tags.remove(&key).unwrap_or_default();
            if !has_date && stray == "TYER" {
                tags.insert("date".to_owned(), value);
            }
        }
    }
    tags
}

/// The arguments that decide which tags reach the output: copy the source's, then rename, empty or
/// leave out fields (foreign ids, ffmpeg's internal names, stray date frames, the cleaning entry).
/// One function so the conversion and the throwaway file the WAV's ID3 chunk is built from (
/// `tags::donor_id3`) are told exactly the same.
pub fn tag_arguments(
    preset: &Preset,
    cleanup: &TagCleanup,
    source_tags: &HashMap<String, String>,
    ignored: &[String],
    source: SourceKind,
) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    a.extend(["-map_metadata", "0"].map(str::to_owned));
    // ffmpeg's Ogg writer takes the comments from the STREAM, and a stream language (an M4A's `und`)
    // beats the file's own LANGUAGE field. Emptying it lets the field through.
    if matches!(preset.format, Format::Ogg | Format::Opus | Format::OggFlac) {
        a.extend(["-metadata:s:a:0", "language="].map(str::to_owned));
    }
    if source.tags_on_stream {
        // Merged into the file's (empty) set. Not done for every source: an M4A's stream carries
        // `handler_name` and the like, which would become tags of the output.
        a.extend(["-map_metadata:g", "0:s:a:0"].map(str::to_owned));
    }
    // The container's own timestamp is not a tag and is never carried (ffmpeg copies it with the rest).
    a.extend(["-metadata", "creation_time="].map(str::to_owned));
    // The source's tags with the foreign spellings renamed to the names every other file of the
    // library uses (see `FOREIGN_FIELDS`); everything below reads this, not the raw map.
    let (tags, renamed) = prepare_tags(preset, source_tags, ignored, source);
    let source_tags = &tags;
    // Tags the user listed by an exact name are dropped here too, by ffmpeg itself. That is the only
    // way for the formats the tag library cannot write (WMA, WAV, AC-3) and it also keeps the AIFF
    // text chunks (which hold the comment apart from the ID3 tag) from being written. A wildcard is
    // left to the tag library, which sees every name in the file.
    let emptied = std::cell::RefCell::new(Vec::<String>::new());
    let empty = |key: String, a: &mut Vec<String>| {
        let mut done = emptied.borrow_mut();
        if !done.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
            a.push("-metadata".to_owned());
            a.push(format!("{key}="));
            done.push(key);
        }
    };
    for key in ignored {
        empty(key.clone(), &mut a);
    }
    if cleanup.mode == CleanupMode::Keep {
        for name in &cleanup.exceptions {
            let name = name.trim();
            if name.is_empty() || name.contains('*') {
                continue;
            }
            empty(ffmpeg_key(name), &mut a);
        }
    }
    // The file's own tags decide the rest: every tag the source carries that the cleanup drops is
    // emptied here too, which is what makes a wildcard and "delete all, except..." reach the formats
    // ffmpeg alone writes (AC-3, raw AAC, MP2) and the text chunks of AIFF and WAV.
    if cleanup.edits_tags() {
        let mut keys: Vec<&String> = source_tags.keys().collect();
        keys.sort();
        for key in keys {
            if cleanup.drops(&crate::tags::tagger_names(key)) {
                empty(key.clone(), &mut a);
            }
        }
    }
    // ⚠ CLAUDE: a source can carry the ID3v2.3 date frames as keys of their own (a WAV whose ID3 tag has a
    // `TYER` frame holding the whole date, found 2026-10-14 on a WAV of the Orange Crush LP). ffmpeg
    // reads them as ordinary fields named `TYER` / `TDAT` / `TIME`, and when it writes ID3v2.3 it
    // writes that stray `TYER=2025-11-24` over the real year it derives from `date` - a year taggers
    // show as empty. The date is `date`'s job: the stray keys are emptied, and when the source has no
    // `date` the year they held becomes it.
    let source_has = |k: &str| source_tags.keys().any(|s| s.eq_ignore_ascii_case(k));
    if !source_has("date") {
        if let Some((_, v)) = source_tags.iter().find(|(k, _)| k.eq_ignore_ascii_case("TYER")) {
            a.push("-metadata".to_owned());
            a.push(format!("date={v}"));
        }
    } else if let Some((_, v)) = source_tags.iter().find(|(k, _)| k.eq_ignore_ascii_case("date")) {
        // ⚠ CLAUDE: a date with a stray carriage return after it (`2026-02-01\r`, found 2026-10-17 on a FLAC another
        // tool had rewritten) is copied by ffmpeg as it is, and an MP3 then got TWO year frames - ffmpeg's and the
        // tag layer's (`tags::restore_full_date`). The probe has trimmed the value: ffmpeg is handed that one.
        if !v.is_empty() && !emptied.borrow().iter().any(|k| k.eq_ignore_ascii_case("date")) {
            a.push("-metadata".to_owned());
            a.push(format!("date={v}"));
        }
    }
    for key in ["TYER", "TDAT", "TIME"] {
        if source_has(key) {
            empty(key.to_owned(), &mut a);
        }
    }
    // The renamed fields: the raw spelling is emptied (ffmpeg would write it as a field of its own),
    // and the value goes in under the standard name unless the cleanup dropped it.
    for (raw, standard, value) in &renamed {
        empty(raw.clone(), &mut a);
        if !emptied.borrow().iter().any(|k| k.eq_ignore_ascii_case(standard)) {
            a.push("-metadata".to_owned());
            a.push(format!("{standard}={value}"));
        }
    }
    // A track or disc number and its total are two fields in a Vorbis comment (TRACKNUMBER=3,
    // TRACKTOTAL=10) and ONE in ID3 and MP4 (`3/10`, Mp3tag's TRACK). ffmpeg does not merge them, so a
    // FLAC converted to MP3 carried TRCK=3 beside a custom TRACKTOTAL.
    if !preset.format.writes_vorbis_comments() && preset.format != Format::Wma {
        for (number, totals) in [("track", ["tracktotal", "totaltracks"]), ("disc", ["disctotal", "totaldiscs"])] {
            let get = |k: &str| source_tags.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_owned());
            let total = totals.iter().find_map(|t| get(t)).filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()));
            if let (Some(n), Some(total)) = (get(number), total) {
                if !n.contains('/') && !n.is_empty() && !emptied.borrow().iter().any(|k| k.eq_ignore_ascii_case(number)) {
                    a.push("-metadata".to_owned());
                    a.push(format!("{number}={n}/{total}"));
                }
            }
            for t in totals {
                if source_tags.keys().any(|k| k.eq_ignore_ascii_case(t)) {
                    empty((*t).to_owned(), &mut a);
                }
            }
        }
    }
    // ⚠ CLAUDE: ffmpeg does not write `encoded_by` into an ID3 tag when the field comes from a WAV that
    // also has an INFO `ITCH` (the value is read, shown by `ffmpeg -i`, and then lost on the way out;
    // found 2026-10-14 on a WAV Mp3tag wrote). Given as an argument it is written, so it is.
    if let Some((_, value)) = source_tags.iter().find(|(k, _)| k.eq_ignore_ascii_case("encoded_by")) {
        let dropped = emptied.borrow().iter().any(|k| k.eq_ignore_ascii_case("encoded_by"));
        if !dropped && !cleanup.drops(&crate::tags::tagger_names("encoded_by")) {
            a.push("-metadata".to_owned());
            a.push(format!("encoded_by={value}"));
        }
    }
    a
}

/// What `Destination` and `Preset` decide together, as one value the queue can carry around.
#[derive(Clone, Debug)]
pub struct Plan {
    pub input: PathBuf,
    pub preset: Preset,
    pub destination: Destination,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;

    const BANNER: &str = "\
Input #0, mp3, from 'x.mp3':
  Metadata:
    title           : Jóga
    artist          : Björk
    album           : Homogenic
    track           : 3/10
  Duration: 00:03:41.50, start: 0.025057, bitrate: 192 kb/s
  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 192 kb/s
    Metadata:
      encoder         : LAME3.100
  Stream #0:1: Video: mjpeg, yuvj444p, 500x500
At least one output file must be specified";

    #[test]
    fn probe_reads_duration_and_only_container_tags() {
        let p = parse_probe(BANNER);
        assert_eq!(p.duration_us, 221_500_000);
        assert_eq!((p.codec.as_str(), p.bitrate_kbps), ("mp3", Some(192)));
        assert_eq!(p.tags["title"], "Jóga");
        assert_eq!(p.tags["track"], "3/10");
        assert!(!p.tags.contains_key("encoder"), "a non-tag stream field leaked in");
    }

    #[test]
    fn ogg_and_opus_keep_their_tags_on_the_stream() {
        let text = "Input #0, ogg, from 'x.ogg':
  Duration: 00:00:01.00, start: 0.0, bitrate: 42 kb/s
  Stream #0:0: Audio: vorbis, 44100 Hz, mono, fltp, 80 kb/s
    Metadata:
      ENCODER         : Lavc libvorbis
      TITLE           : Song
      ARTIST          : Me
      TRACKNUMBER     : 3
";
        let p = parse_probe(text);
        assert_eq!(p.tags["title"], "Song");
        assert_eq!(p.tags["artist"], "Me");
        assert_eq!(p.tags["track"], "3");
        assert!(!p.tags.contains_key("encoder"));
        assert!(p.source.tags_on_stream, "the tags of an Ogg are on its stream: -map_metadata 0 alone loses them");
        // A file that carries its tags itself (an MP3) needs no stream copy: it would bring
        // `handler_name` and the like along.
        assert!(!parse_probe(BANNER).source.tags_on_stream);
        // The banner is an MP3: an ID3 source.
        assert!(parse_probe(BANNER).source.id3);
        assert!(!p.source.id3, "an Ogg is not");
    }

    #[test]
    fn a_source_on_the_stream_gets_its_stream_tags_copied_and_one_off_it_does_not() {
        let p = Preset::new("t", Format::Mp3);
        let c = TagCleanup::keep();
        let on = tag_arguments(&p, &c, &HashMap::new(), &[], SourceKind { tags_on_stream: true, id3: false }).join(" ");
        let off = tag_arguments(&p, &c, &HashMap::new(), &[], SourceKind::default()).join(" ");
        assert!(on.contains("-map_metadata 0 -map_metadata:g 0:s:a:0"), "{on}");
        assert!(!off.contains("0:s:a:0"), "{off}");
    }

    #[test]
    fn the_foreign_spellings_of_every_source_format_are_renamed_to_the_field() {
        let mut src: HashMap<String, String> = HashMap::new();
        for (k, v) in [("wm/writer", "L"), ("WM/BeatsPerMinute", "120"), ("TPE4", "M"), ("TBPM", "121"), ("sort_album", "S"), ("TYER", "1999")] {
            src.insert(k.to_owned(), v.to_owned());
        }
        let flac = effective_tags(&Preset::new("t", Format::Flac), &src, &[], SourceKind::default());
        let has = |k: &str| flac.keys().any(|x| x.eq_ignore_ascii_case(k));
        assert!(has("LYRICIST") && has("BPM") && has("MIXARTIST") && has("ALBUMSORT"), "{flac:?}");
        assert!(!has("wm/writer") && !has("TPE4") && !has("sort_album"), "{flac:?}");
        // A date frame is not turned into a field of its own: the date has its own path.
        assert!(has("TYER") || has("date"), "{flac:?}");
    }

    #[test]
    fn the_container_block_wins_over_the_stream_block() {
        let text = "Input #0, mp3, from 'x.mp3':
  Metadata:
    title           : Container
  Duration: 00:00:01.00, start: 0.0, bitrate: 1 kb/s
  Stream #0:0: Audio: mp3
    Metadata:
      title           : Stream
";
        assert_eq!(parse_probe(text).tags["title"], "Container");
    }

    #[test]
    fn cover_thumbnails_from_a_real_file() {
        let Some(ff) = ffmpeg_for_tests() else { return };
        let dir = std::env::temp_dir().join(format!("yaiol-cover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str], out: &Path| {
            let mut c = command(&ff);
            c.args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).arg(out);
            assert!(c.status().unwrap().success());
        };
        let png = dir.join("c.png");
        run(&["-f", "lavfi", "-i", "color=c=red:s=300x200", "-frames:v", "1"], &png);
        let with = dir.join("with.flac");
        let png_s = png.to_string_lossy().into_owned();
        run(&["-f", "lavfi", "-i", "sine=duration=1", "-i", &png_s, "-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"], &with);
        let without = dir.join("without.flac");
        run(&["-f", "lavfi", "-i", "sine=duration=1", "-c:a", "flac"], &without);
        let jpg = cover_jpeg(&ff, &with, 64).expect("a cover was embedded");
        assert!(jpg.starts_with(&[0xFF, 0xD8]));
        assert!(cover_jpeg(&ff, &without, 64).is_none());
        assert!(cover_jpeg(&ff, &dir.join("missing.flac"), 64).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn listed_exact_names_become_empty_metadata_arguments() {
        let p = Preset::new("t", Format::Wav);
        let mut c = TagCleanup::new("c");
        c.exceptions = vec!["COMMENT".into(), "ALBUMARTIST".into(), "REPLAYGAIN_*".into()];
        let a = build_args(Path::new("i.flac"), Path::new("o.wav"), &p, &c, &HashMap::new(), &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default());
        let s = a.join(" ");
        assert!(s.contains("-metadata COMMENT= -metadata album_artist="), "{s}");
        assert!(!s.contains("REPLAYGAIN"), "a wildcard with no matching source tag is not an argument");
    }

    #[test]
    fn foreign_four_letter_ids_are_written_under_the_standard_names() {
        let p = Preset::new("t", Format::Flac);
        let source: HashMap<String, String> = [("TSO2", "yaiol"), ("TSOC", "Gabard, Yannick"), ("TEXT", "Yannick Gabard"), ("ICMS", "x")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let s = build_args(Path::new("i.wav"), Path::new("o.flac"), &p, &TagCleanup::keep(), &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(s.contains("-metadata TSO2= ") && s.contains("-metadata ALBUMARTISTSORT=yaiol"), "{s}");
        assert!(s.contains("-metadata TEXT= ") && s.contains("-metadata LYRICIST=Yannick Gabard"), "{s}");
        assert!(s.contains("-metadata COMPOSERSORT=Gabard, Yannick"), "{s}");
        assert!(!s.contains("ICMS"), "an id with no standard name is left alone: {s}");
        // A file that already has the standard name keeps its own value and only loses the raw key.
        let both: HashMap<String, String> =
            [("TSO2", "raw"), ("albumartistsort", "own")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        let s = build_args(Path::new("i.wav"), Path::new("o.flac"), &p, &TagCleanup::keep(), &both, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(s.contains("-metadata TSO2="), "{s}");
        // The cleanup still wins: a dropped standard name is not written back.
        let mut c = TagCleanup::new("c");
        c.exceptions = vec!["ALBUMARTISTSORT".into()];
        let s = build_args(Path::new("i.wav"), Path::new("o.flac"), &p, &c, &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(!s.contains("ALBUMARTISTSORT=yaiol"), "{s}");
    }

    #[test]
    fn vorbis_outputs_get_the_library_names_for_ffmpegs_internal_ones() {
        let source: HashMap<String, String> = [("album-sort", "A"), ("artist-sort", "B"), ("grouping", "Pop"), ("performer", "Cond")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let args = |f: Format| build_args(Path::new("i.mp3"), Path::new("o"), &Preset::new("t", f), &TagCleanup::keep(), &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        let s = args(Format::Flac);
        assert!(s.contains("-metadata ALBUMSORT=A") && s.contains("-metadata ARTISTSORT=B"), "{s}");
        assert!(s.contains("-metadata CONTENTGROUP=Pop") && s.contains("-metadata grouping= "), "{s}");
        assert!(!s.contains("CONDUCTOR") && !s.contains("-metadata performer="), "performer is left alone: {s}");
        // An ID3 output keeps ffmpeg's keys: it writes the real frames from them.
        let s = args(Format::Mp3);
        assert!(!s.contains("ALBUMSORT") && !s.contains("CONTENTGROUP"), "{s}");
    }

    #[test]
    fn a_track_and_its_total_are_one_field_outside_vorbis_comments() {
        let source: HashMap<String, String> = [("track", "3"), ("tracktotal", "10"), ("disc", "1"), ("totaldiscs", "2")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let args = |fmt: Format| build_args(Path::new("i.flac"), Path::new("o"), &Preset::new("t", fmt), &TagCleanup::keep(), &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        for fmt in [Format::Mp3, Format::Aac, Format::Wav] {
            let s = args(fmt);
            assert!(s.contains("-metadata track=3/10") && s.contains("-metadata disc=1/2"), "{fmt:?}: {s}");
            assert!(s.contains("-metadata tracktotal= ") && s.contains("-metadata totaldiscs= "), "{fmt:?}: {s}");
        }
        // A Vorbis comment keeps the two fields apart, as Mp3tag writes them.
        let s = args(Format::Flac);
        assert!(!s.contains("track=3/10") && !s.contains("tracktotal="), "{s}");
        // A number that already holds its total is left as it is.
        let both: HashMap<String, String> =
            [("track", "3/12"), ("tracktotal", "10")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        let s = build_args(Path::new("i"), Path::new("o"), &Preset::new("t", Format::Mp3), &TagCleanup::keep(), &both, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(!s.contains("track=3/10"), "{s}");
    }

    #[test]
    fn a_wav_does_not_get_the_recording_code_in_its_info_list_as_a_source() {
        let s = build_args(Path::new("i.flac"), Path::new("o.wav"), &Preset::new("t", Format::Wav), &TagCleanup::keep(), &HashMap::new(), &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(s.contains("-metadata ISRC="), "{s}");
        let s = build_args(Path::new("i.flac"), Path::new("o.mp3"), &Preset::new("t", Format::Mp3), &TagCleanup::keep(), &HashMap::new(), &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(!s.contains("-metadata ISRC="), "{s}");
    }

    #[test]
    fn a_stray_tyer_key_is_emptied_and_never_beats_the_date() {
        let p = Preset::new("t", Format::Mp3);
        let with = |pairs: &[(&str, &str)]| -> String {
            let source: HashMap<String, String> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            build_args(Path::new("i.wav"), Path::new("o.mp3"), &p, &TagCleanup::keep(), &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ")
        };
        let s = with(&[("date", "2025-11-24"), ("tyer", "1999-01-01"), ("title", "T")]);
        assert!(s.contains("-metadata TYER="), "{s}");
        // The real date is the one handed to ffmpeg (trimmed: see the note beside it), never the stray frame's.
        assert!(s.contains("-metadata date=2025-11-24") && !s.contains("date=1999"), "{s}");
        // No date at all: the year the stray frame held becomes the date.
        let s = with(&[("tyer", "2019")]);
        assert!(s.contains("-metadata date=2019") && s.contains("-metadata TYER="), "{s}");
        // Nothing stray, nothing added.
        assert!(!with(&[("date", "2025")]).contains("TYER"));
    }

    #[test]
    fn what_the_file_carries_decides_a_wildcard_and_delete_all() {
        let p = Preset::new("t", Format::Wav);
        let source: HashMap<String, String> = [("title", "T"), ("album_artist", "A"), ("replaygain_track_gain", "-6 dB")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        // Keep everything except a wildcard: only the matching source tag is emptied.
        let mut c = TagCleanup::new("c");
        c.exceptions = vec!["REPLAYGAIN_*".into()];
        let s = build_args(Path::new("i.flac"), Path::new("o.wav"), &p, &c, &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(s.contains("-metadata replaygain_track_gain="), "{s}");
        assert!(!s.contains("-metadata title=") && !s.contains("-metadata album_artist="), "{s}");
        // Delete everything except one: the listed one is the only tag NOT emptied.
        c.mode = CleanupMode::Delete;
        c.exceptions = vec!["ALBUMARTIST".into()];
        let s = build_args(Path::new("i.flac"), Path::new("o.wav"), &p, &c, &source, &[], false, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(s.contains("-metadata title=") && s.contains("-metadata replaygain_track_gain="), "{s}");
        assert!(!s.contains("-metadata album_artist="), "{s}");
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=1500000"), Some(1_500_000));
        assert_eq!(parse_progress_line("out_time_ms=1500000"), None);
        assert_eq!(parse_progress_line("progress=continue"), None);
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
    }

    #[test]
    fn args_for_an_mp3_with_cover() {
        let p = Preset::new("t", Format::Mp3);
        let a = build_args(Path::new("in.flac"), Path::new("out.mp3"), &p, &TagCleanup::keep(), &HashMap::new(), &[], true, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
        assert!(a.contains("-i in.flac -i in.flac -map 0:a:0 -map 1:v:0? -c:v copy -disposition:v:0 attached_pic"));
        assert!(a.contains("-map_metadata 0"));
        assert!(a.contains("-c:a libmp3lame -b:a 192k"));
        assert!(a.contains("-id3v2_version 3"));
        let mut p24 = p.clone();
        p24.id3_version = crate::preset::Id3Version::V24;
        assert!(build_args(Path::new("in.flac"), Path::new("out.mp3"), &p24, &TagCleanup::keep(), &HashMap::new(), &[], true, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ").contains("-id3v2_version 4"));
        assert!(a.ends_with("-progress pipe:1 -nostats out.mp3"));
    }

    #[test]
    fn formats_that_cannot_hold_a_picture_never_map_one() {
        for f in [Format::Wav, Format::Ogg, Format::Opus] {
            let a = build_args(Path::new("i"), Path::new("o"), &Preset::new("t", f), &TagCleanup::keep(), &HashMap::new(), &[], true, SourceKind::default(), &crate::effects::EffectsPlan::default()).join(" ");
            assert!(!a.contains("v:0") && a.matches("-i ").count() == 1, "{f:?}");
        }
    }
}
