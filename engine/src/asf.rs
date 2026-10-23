//! WMA tags: the part ffmpeg's ASF muxer does not do.
//!
//! ffmpeg writes a WMA with its fields under whatever name they had in the source (`date`,
//! `RELEASETYPE`...) and puts a picture in as a second video stream, which no player shows. Windows
//! Media Player, Mp3tag and MusicBee read the `WM/...` attribute names and a `WM/Picture` record in
//! the header, so after ffmpeg is done this module rewrites the header in place:
//!
//! - every attribute that has a standard `WM/...` (or `MusicBrainz/...`) name gets it;
//! - the tags the preset lists are dropped, wildcards included;
//! - the cover goes in as `WM/Picture` in a Metadata Library object inside the header extension.
//!
//! The audio packets are not touched and the index is by packet number, so nothing after the header
//! needs a new offset - only the sizes in the header itself and the file size it records.

use crate::cleanup::TagCleanup;
use std::path::Path;

/// One picture for `WM/Picture`: MIME type + bytes.
pub struct Cover {
    pub mime: String,
    pub data: Vec<u8>,
}

const HEADER: [u8; 16] = guid(0x75B22630, 0x668E, 0x11CF, [0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C]);
const FILE_PROPS: [u8; 16] = guid(0x8CABDCA1, 0xA947, 0x11CF, [0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65]);
const CONTENT_DESC: [u8; 16] = guid(0x75B22633, 0x668E, 0x11CF, [0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C]);
const EXT_CONTENT_DESC: [u8; 16] = guid(0xD2D0A440, 0xE307, 0x11D2, [0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50]);
const HEADER_EXT: [u8; 16] = guid(0x5FBF03B5, 0xA92E, 0x11CF, [0x8E, 0xE3, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65]);
const METADATA_LIBRARY: [u8; 16] = guid(0x44231C94, 0x9498, 0x49D1, [0xA1, 0x41, 0x1D, 0x13, 0x4E, 0x45, 0x70, 0x54]);

const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

/// Each standard attribute name and the other spellings it answers to (Mp3tag, Vorbis, ID3 frame).
/// The first four live in the fixed content-description block, not in the extended one.
const NAMES: &[(&str, &[&str])] = &[
    ("Title", &["TITLE", "TIT2"]),
    ("Author", &["ARTIST", "TPE1"]),
    ("Copyright", &["COPYRIGHT", "TCOP"]),
    ("Description", &["COMMENT", "DESCRIPTION", "COMM"]),
    ("WM/AlbumTitle", &["ALBUM", "TALB"]),
    ("WM/AlbumArtist", &["ALBUMARTIST", "ALBUM ARTIST", "TPE2"]),
    ("WM/AlbumArtistSortOrder", &["ALBUMARTISTSORT", "TSO2"]),
    ("WM/ArtistSortOrder", &["ARTISTSORT", "TSOP"]),
    ("WM/TitleSortOrder", &["TITLESORT", "TSOT"]),
    ("WM/AlbumSortOrder", &["ALBUMSORT", "TSOA"]),
    ("WM/Composer", &["COMPOSER", "TCOM"]),
    ("WM/Genre", &["GENRE", "TCON"]),
    ("WM/TrackNumber", &["TRACK", "TRACKNUMBER", "TRCK"]),
    ("WM/PartOfSet", &["DISCNUMBER", "DISC", "TPOS"]),
    ("WM/Year", &["YEAR", "DATE", "TDRC", "TYER"]),
    ("WM/Writer", &["LYRICIST", "WRITER", "TEXT"]),
    ("WM/Conductor", &["CONDUCTOR", "TPE3"]),
    ("WM/Publisher", &["PUBLISHER", "LABEL", "ORGANIZATION", "TPUB"]),
    ("WM/ISRC", &["ISRC", "TSRC"]),
    ("WM/BeatsPerMinute", &["BPM", "TBPM"]),
    ("WM/Mood", &["MOOD"]),
    ("WM/Lyrics", &["LYRICS", "UNSYNCEDLYRICS", "USLT"]),
    ("WM/Language", &["LANGUAGE", "TLAN"]),
    ("WM/EncodedBy", &["ENCODEDBY", "TENC"]),
    ("WM/EncodingSettings", &["ENCODERSETTINGS", "ENCODER", "TSSE"]),
    ("WM/ContentGroupDescription", &["CONTENTGROUP", "GROUPING", "TIT1"]),
    ("WM/InitialKey", &["INITIALKEY", "TKEY"]),
    ("WM/SubTitle", &["SUBTITLE", "TIT3"]),
    ("WM/OriginalAlbumTitle", &["ORIGALBUM", "TOAL"]),
    ("WM/OriginalArtist", &["ORIGARTIST", "TOPE"]),
    ("WM/OriginalLyricist", &["ORIGLYRICIST", "TOLY"]),
    ("WM/OriginalReleaseYear", &["ORIGYEAR", "TORY"]),
    ("MusicBrainz/Album Type", &["RELEASETYPE", "MUSICBRAINZ_ALBUMTYPE"]),
    ("MusicBrainz/Track Id", &["MUSICBRAINZ_TRACKID"]),
    ("MusicBrainz/Album Id", &["MUSICBRAINZ_ALBUMID"]),
    ("MusicBrainz/Artist Id", &["MUSICBRAINZ_ARTISTID"]),
    ("MusicBrainz/Album Artist Id", &["MUSICBRAINZ_ALBUMARTISTID"]),
    ("MusicBrainz/Release Group Id", &["MUSICBRAINZ_RELEASEGROUPID"]),
];

/// The standard name an attribute should carry, if it has one different from its own.
fn standard_name(name: &str) -> Option<&'static str> {
    NAMES
        .iter()
        .skip(4)
        .find(|(_, aliases)| aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
        .map(|(asf, _)| *asf)
}

/// Every spelling `name` answers to, for matching the user's removal list.
fn spellings(name: &str) -> Vec<String> {
    let mut v = vec![name.to_owned()];
    for (asf, aliases) in NAMES {
        let hit = asf.eq_ignore_ascii_case(name) || aliases.iter().any(|a| a.eq_ignore_ascii_case(name));
        if hit {
            v.push((*asf).to_owned());
            v.extend(aliases.iter().map(|a| (*a).to_owned()));
        }
    }
    v
}

fn removed(cleanup: &TagCleanup, name: &str) -> bool {
    cleanup.drops(&spellings(name))
}

struct Attribute {
    name: String,
    kind: u16,
    value: Vec<u8>,
}

fn u16_at(b: &[u8], o: usize) -> Result<usize, String> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or_else(|| "truncated WMA header".to_owned())
}

fn u64_at(b: &[u8], o: usize) -> Result<u64, String> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().expect("8 bytes")))
        .ok_or_else(|| "truncated WMA header".to_owned())
}

fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\u{0}').to_owned()
}

fn utf16_z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    v.extend_from_slice(&[0, 0]);
    v
}

fn object(guid: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(24 + body.len());
    v.extend_from_slice(guid);
    v.extend_from_slice(&((24 + body.len()) as u64).to_le_bytes());
    v.extend_from_slice(body);
    v
}

fn parse_extended(body: &[u8]) -> Result<Vec<Attribute>, String> {
    let n = u16_at(body, 0)?;
    let mut p = 2;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let nl = u16_at(body, p)?;
        let name = utf16(body.get(p + 2..p + 2 + nl).ok_or("truncated WMA attribute")?);
        p += 2 + nl;
        let kind = u16_at(body, p)? as u16;
        let vl = u16_at(body, p + 2)?;
        let value = body.get(p + 4..p + 4 + vl).ok_or("truncated WMA attribute")?.to_vec();
        p += 4 + vl;
        out.push(Attribute { name, kind, value });
    }
    Ok(out)
}

fn write_extended(attrs: &[Attribute]) -> Vec<u8> {
    let mut b = (attrs.len() as u16).to_le_bytes().to_vec();
    for a in attrs {
        let name = utf16_z(&a.name);
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        b.extend_from_slice(&name);
        b.extend_from_slice(&a.kind.to_le_bytes());
        b.extend_from_slice(&(a.value.len() as u16).to_le_bytes());
        b.extend_from_slice(&a.value);
    }
    b
}

/// The (name, text value) of every attribute the WMA at `path` carries, as they are stored: the
/// non-empty ones of the fixed content-description block (`Title`, `Author`, `Copyright`,
/// `Description`) and every extended one (binary values read as an empty text).
pub fn attributes(path: &Path) -> Result<Vec<(String, String)>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read the WMA: {e}"))?;
    if bytes.len() < 30 || bytes[..16] != HEADER {
        return Err("not an ASF file".to_owned());
    }
    let header_size = (u64_at(&bytes, 16)? as usize).min(bytes.len());
    let mut out = Vec::new();
    let mut p = 30;
    while p + 24 <= header_size {
        let size = u64_at(&bytes, p + 16)? as usize;
        if size < 24 || p + size > header_size {
            break;
        }
        let mut g = [0u8; 16];
        g.copy_from_slice(&bytes[p..p + 16]);
        let body = &bytes[p + 24..p + size];
        if g == EXT_CONTENT_DESC {
            for a in parse_extended(body)? {
                let text = if a.kind == 0 { utf16(&a.value) } else { String::new() };
                out.push((a.name, text));
            }
        } else if g == CONTENT_DESC && body.len() >= 10 {
            let mut off = 10;
            for (i, (name, _)) in NAMES.iter().take(4).enumerate() {
                let len = u16_at(body, i * 2)?;
                if len > 2 {
                    if let Some(s) = body.get(off..off + len) {
                        out.push(((*name).to_owned(), utf16(s)));
                    }
                }
                off += len;
            }
        }
        p += size;
    }
    Ok(out)
}

/// The picture of a WMA (`WM/Picture`), wherever the file that wrote it put it: in the extended
/// content description (a tagger such as Mp3tag) or in a metadata library of the header extension
/// (this module's own writer, Windows Media Player). ffmpeg shows it as a video stream only
/// sometimes, and the tag library does not read ASF at all, so a WMA source's cover is read here.
pub fn picture(path: &Path) -> Option<Cover> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 30 || bytes[..16] != HEADER {
        return None;
    }
    let header_size = (u64_at(&bytes, 16).ok()? as usize).min(bytes.len());
    // One WM/Picture value: picture type (1), data length (4), MIME and description (UTF-16, zero
    // terminated), then the picture.
    let parse = |v: &[u8]| -> Option<Cover> {
        let len = u32::from_le_bytes(v.get(1..5)?.try_into().ok()?) as usize;
        let mut p = 5;
        let mut strings = Vec::new();
        for _ in 0..2 {
            let start = p;
            while p + 1 < v.len() && !(v[p] == 0 && v[p + 1] == 0) {
                p += 2;
            }
            strings.push(utf16(v.get(start..p)?));
            p += 2;
        }
        let data = v.get(p..p + len.min(v.len().saturating_sub(p)))?.to_vec();
        if data.is_empty() {
            return None;
        }
        let mime = if strings[0].is_empty() { "image/jpeg".to_owned() } else { strings[0].clone() };
        Some(Cover { mime, data })
    };
    let mut p = 30;
    while p + 24 <= header_size {
        let size = u64_at(&bytes, p + 16).ok()? as usize;
        if size < 24 || p + size > header_size {
            break;
        }
        let mut g = [0u8; 16];
        g.copy_from_slice(&bytes[p..p + 16]);
        let body = &bytes[p + 24..p + size];
        if g == EXT_CONTENT_DESC {
            if let Ok(attrs) = parse_extended(body) {
                if let Some(c) = attrs.iter().find(|a| a.name == "WM/Picture").and_then(|a| parse(&a.value)) {
                    return Some(c);
                }
            }
        } else if g == HEADER_EXT && body.len() > 22 {
            // guid (16) + reserved (2) + data size (4), then the children.
            let mut q = 22;
            while q + 24 <= body.len() {
                let csize = u64_at(body, q + 16).ok()? as usize;
                if csize < 24 || q + csize > body.len() {
                    break;
                }
                if body[q..q + 16] == METADATA_LIBRARY {
                    let lib = &body[q + 24..q + csize];
                    let count = u16_at(lib, 0).ok()?;
                    let mut r = 2;
                    for _ in 0..count {
                        // language list index, stream number, name length, data type, data length
                        let name_len = u16_at(lib, r + 4).ok()?;
                        let data_len = u32::from_le_bytes(lib.get(r + 8..r + 12)?.try_into().ok()?) as usize;
                        let name = utf16(lib.get(r + 12..r + 12 + name_len)?);
                        let data = lib.get(r + 12 + name_len..r + 12 + name_len + data_len)?;
                        if name == "WM/Picture" {
                            if let Some(c) = parse(data) {
                                return Some(c);
                            }
                        }
                        r += 12 + name_len + data_len;
                    }
                }
                q += csize;
            }
        }
        p += size;
    }
    None
}

/// Patch the WMA at `path`; returns how many things changed (renamed + removed + the cover).
///
/// `added` are fields an effect writes (ReplayGain): put in last, after the removals, so no cleaning
/// entry can take them away, replacing an attribute of the same name.
pub fn finish(path: &Path, cleanup: &TagCleanup, cover: Option<&Cover>, added: &[(String, String)]) -> Result<usize, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read the WMA: {e}"))?;
    if bytes.len() < 30 || bytes[..16] != HEADER {
        return Err("not an ASF file".to_owned());
    }
    let header_size = u64_at(&bytes, 16)? as usize;
    if header_size < 30 || header_size > bytes.len() {
        return Err("bad WMA header size".to_owned());
    }
    // The header's child objects: (guid, whole object bytes).
    let mut objects: Vec<([u8; 16], Vec<u8>)> = Vec::new();
    let mut p = 30;
    while p + 24 <= header_size {
        let size = u64_at(&bytes, p + 16)? as usize;
        if size < 24 || p + size > header_size {
            return Err("bad WMA object size".to_owned());
        }
        let mut g = [0u8; 16];
        g.copy_from_slice(&bytes[p..p + 16]);
        objects.push((g, bytes[p..p + size].to_vec()));
        p += size;
    }

    let mut changed = 0usize;
    let removing = cleanup.edits_tags();
    // Standard attributes the effects/source-only step is about to write: a spelling of one of them
    // (`ORGANIZATION` for `WM/Publisher`) keeps its own name, or the added value would overwrite it.
    let added_std: Vec<&str> = added.iter().filter_map(|(n, _)| standard_name(n)).collect();
    for (g, obj) in objects.iter_mut() {
        if *g == EXT_CONTENT_DESC {
            let attrs = parse_extended(&obj[24..])?;
            let mut out: Vec<Attribute> = Vec::with_capacity(attrs.len());
            for mut a in attrs {
                if removing && removed(cleanup, &a.name) {
                    changed += 1;
                    continue;
                }
                // ffmpeg writes a title or a copyright in the fixed content-description block AND as
                // an extended attribute under the upper-case field name: the second one is a duplicate
                // (a reader would show the field twice).
                if a.name.eq_ignore_ascii_case("TITLE") || a.name.eq_ignore_ascii_case("COPYRIGHT") {
                    changed += 1;
                    continue;
                }
                if let Some(std) = standard_name(&a.name) {
                    if a.name != std {
                        if out.iter().any(|o| o.name == std) {
                            // The standard name is already there: the same value adds nothing; ANOTHER value
                            // (`PUBLISHER` and `ORGANIZATION` in one source) is kept under its own name.
                            if out.iter().any(|o| o.name == std && o.value == a.value) {
                                changed += 1;
                                continue;
                            }
                        } else if !added_std.contains(&std) {
                            a.name = std.to_owned();
                            changed += 1;
                        }
                    }
                }
                out.push(a);
            }
            for (name, value) in added {
                // A field of the table is written under its WM/... name.
                let name = standard_name(name).map_or_else(|| name.clone(), str::to_owned);
                out.retain(|o| !o.name.eq_ignore_ascii_case(&name));
                out.push(Attribute { name, kind: 0, value: utf16_z(value) });
                changed += 1;
            }
            *obj = object(&EXT_CONTENT_DESC, &write_extended(&out));
        } else if *g == CONTENT_DESC && removing {
            // Five fixed strings: title, author, copyright, description, rating.
            let body = &obj[24..];
            let mut lens = [0usize; 5];
            for (i, l) in lens.iter_mut().enumerate() {
                *l = u16_at(body, i * 2)?;
            }
            let mut off = 10;
            let mut strings: Vec<&[u8]> = Vec::new();
            for l in lens {
                strings.push(body.get(off..off + l).ok_or("truncated WMA content description")?);
                off += l;
            }
            let mut nb = vec![0u8; 10];
            let mut data = Vec::new();
            for (i, s) in strings.iter().enumerate() {
                let drop = i < 4 && removed(cleanup, NAMES[i].0);
                if drop && !s.is_empty() {
                    changed += 1;
                }
                let keep: &[u8] = if drop { &[] } else { s };
                nb[i * 2..i * 2 + 2].copy_from_slice(&(keep.len() as u16).to_le_bytes());
                data.extend_from_slice(keep);
            }
            nb.extend_from_slice(&data);
            *obj = object(&CONTENT_DESC, &nb);
        } else if *g == HEADER_EXT {
            if let Some(c) = cover {
                let mut pic = vec![3u8];
                let mut inner = utf16_z(&c.mime);
                inner.extend_from_slice(&utf16_z(""));
                inner.extend_from_slice(&c.data);
                pic.extend_from_slice(&(c.data.len() as u32).to_le_bytes());
                pic.extend_from_slice(&inner);
                let name = utf16_z("WM/Picture");
                let mut lib = 1u16.to_le_bytes().to_vec();
                lib.extend_from_slice(&0u16.to_le_bytes()); // language list index
                lib.extend_from_slice(&0u16.to_le_bytes()); // stream: the whole file
                lib.extend_from_slice(&(name.len() as u16).to_le_bytes());
                lib.extend_from_slice(&1u16.to_le_bytes()); // byte array
                lib.extend_from_slice(&(pic.len() as u32).to_le_bytes());
                lib.extend_from_slice(&name);
                lib.extend_from_slice(&pic);
                let lib = object(&METADATA_LIBRARY, &lib);
                // Header extension: guid, size, reserved guid, reserved u16, data size, then children.
                let data_at = 24 + 16 + 2;
                let mut grown = obj.clone();
                grown.extend_from_slice(&lib);
                let data_size = (grown.len() - (data_at + 4)) as u32;
                let total = grown.len() as u64;
                grown[16..24].copy_from_slice(&total.to_le_bytes());
                grown[data_at..data_at + 4].copy_from_slice(&data_size.to_le_bytes());
                *obj = grown;
                changed += 1;
            }
        }
    }
    if changed == 0 {
        return Ok(0);
    }

    let body_len: usize = objects.iter().map(|(_, o)| o.len()).sum();
    let new_header = 30 + body_len;
    let new_total = new_header + (bytes.len() - header_size);
    let mut out = Vec::with_capacity(new_total);
    out.extend_from_slice(&bytes[..16]);
    out.extend_from_slice(&(new_header as u64).to_le_bytes());
    out.extend_from_slice(&bytes[24..30]);
    for (g, obj) in &objects {
        if *g == FILE_PROPS {
            // Body: file id (16), then the file size this header must agree with.
            let mut o = obj.clone();
            o[24 + 16..24 + 24].copy_from_slice(&(new_total as u64).to_le_bytes());
            out.extend_from_slice(&o);
        } else {
            out.extend_from_slice(obj);
        }
    }
    out.extend_from_slice(&bytes[header_size..]);

    let tmp = path.with_extension("wma.tmp");
    std::fs::write(&tmp, &out).map_err(|e| format!("cannot write the WMA: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace the WMA: {e}")
    })?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_release_types_get_their_windows_names() {
        assert_eq!(standard_name("date"), Some("WM/Year"));
        assert_eq!(standard_name("RELEASETYPE"), Some("MusicBrainz/Album Type"));
        assert_eq!(standard_name("lyricist"), Some("WM/Writer"));
        assert_eq!(standard_name("AMLP_ID"), None);
        // The fixed block's names are never an extended attribute.
        assert_eq!(standard_name("title"), None);
    }

    #[test]
    fn a_removal_entry_matches_whichever_spelling() {
        let mut c = TagCleanup::new("t");
        c.exceptions = vec!["ENCODERSETTINGS".to_owned()];
        assert!(removed(&c, "WM/EncodingSettings"));
        assert!(!removed(&c, "WM/Year"));
        c.exceptions = vec!["WM/*".to_owned()];
        assert!(removed(&c, "WM/Year"));
        // Delete everything but the listed: the listed one stays, the rest goes.
        c.mode = crate::cleanup::CleanupMode::Delete;
        c.exceptions = vec!["YEAR".to_owned()];
        assert!(!removed(&c, "WM/Year"));
        assert!(removed(&c, "WM/EncodingSettings"));
    }
}
