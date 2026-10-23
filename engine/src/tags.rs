//! Repeated tag fields, restored by a tag library.
//!
//! ffmpeg reads a repeated field (two `ALBUMARTIST` lines in a FLAC) as ONE value joined with `;`,
//! and no argument brings the separate values back. After ffmpeg has written the audio, the fields
//! that have several values in the SOURCE are written onto the output as separate values, with
//! `lofty`.
//!
//! ⚠ CLAUDE: this PATCHES, it does not replace the tag. Everything else ffmpeg wrote - free-form
//! fields such as a custom `AMLP_ID`, the picture - is left byte for byte, because the generic tag
//! type cannot represent a free-form field and a full rewrite would lose them.
//!
//! Formats `lofty` cannot write (WMA, raw AC-3) and WAV are left exactly as ffmpeg wrote them.

use crate::{
    cleanup::TagCleanup,
    format::Format,
    preset::{Id3Version, Preset},
};
use lofty::{
    config::{ParseOptions, ParsingMode, WriteOptions},
    file::{AudioFile, TaggedFileExt},
    flac::FlacFile,
    id3::v2::{
        BinaryFrame, CommentFrame, ExtendedTextFrame, Frame, FrameFlags, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame, TextInformationFrame,
        TimestampFormat, TimestampFrame, UnsynchronizedTextFrame,
    },
    iff::{aiff::AiffFile, wav::WavFile},
    mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File},
    mpeg::MpegFile,
    TextEncoding,
    ogg::{OpusFile, VorbisFile, tag::VorbisComments},
    tag::{items::Timestamp, ItemKey, ItemValue, TagType},
};
use std::{collections::HashMap, path::Path};

use crate::cleanup::LyricsMode;

/// What a patch did, for tests.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Report {
    /// Fields that had several values in the source and were written back as separate values.
    pub restored: usize,
}

/// The tag family each output format keeps its tags in; `None` = not handled here.
pub fn target_tag_type(format: Format) -> Option<TagType> {
    match format {
        Format::Mp3 | Format::Aiff => Some(TagType::Id3v2),
        Format::Aac | Format::Alac | Format::M4r => Some(TagType::Mp4Ilst),
        Format::Flac | Format::Ogg | Format::Opus => Some(TagType::VorbisComments),
        // WAV keeps an ID3v2 chunk beside its RIFF INFO list; the chunk is written here.
        Format::Wav => Some(TagType::Id3v2),
        // Written by ffmpeg alone (or nothing at all, for a bare stream): no tag-library pass.
        Format::Wma | Format::Ac3 | Format::AacAdts | Format::OggFlac | Format::WavPack | Format::Tta | Format::Mp2
        | Format::W64 | Format::Au => None,
    }
}

/// The front cover (or, failing that, the first picture) of `source`, if the library can read it.
fn source_cover(ffmpeg: &Path, source: &Path) -> Option<crate::asf::Cover> {
    // The tag library does not read ASF: a WMA's picture is read by the module that writes them.
    if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("wma") || e.eq_ignore_ascii_case("asf")) {
        return crate::asf::picture(source).or_else(|| ffmpeg_cover(ffmpeg, source));
    }
    // The tag library first; ffmpeg for what it cannot read or place (TTA, an APE tag it does not map).
    lofty_cover(source).or_else(|| ffmpeg_cover(ffmpeg, source))
}

fn lofty_cover(source: &Path) -> Option<crate::asf::Cover> {
    let file = lofty::read_from_path(source).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let pic = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;
    let mime = pic.mime_type().map(|m| m.as_str().to_owned()).unwrap_or_else(|| "image/jpeg".to_owned());
    Some(crate::asf::Cover { mime, data: pic.data().to_vec() })
}

/// The first picture of any file ffmpeg can open, as it is stored (copied, not re-encoded).
fn ffmpeg_cover(ffmpeg: &Path, source: &Path) -> Option<crate::asf::Cover> {
    let out = crate::ffmpeg::command(ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(source)
        .args(["-an", "-map", "0:v:0", "-c:v", "copy", "-f", "image2pipe", "pipe:1"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let data = out.stdout;
    let mime = if data.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        return None;
    };
    (out.status.success() && data.len() > 100).then(|| crate::asf::Cover { mime: mime.to_owned(), data })
}

/// The source's cover as a picture the tag library can place (a WMA's is read by `asf::picture`).
fn cover_picture(ffmpeg: &Path, source: &Path) -> Option<lofty::picture::Picture> {
    let c = source_cover(ffmpeg, source)?;
    let mut pic = lofty::picture::Picture::from_reader(&mut c.data.as_slice()).ok()?;
    pic.set_pic_type(lofty::picture::PictureType::CoverFront);
    Some(pic)
}

/// A WavPack's cover: an APEv2 binary item, `Cover Art (Front)`, whose value is a file name, a zero
/// byte and the picture (ffmpeg's WavPack muxer takes no picture).
fn wavpack_cover(ffmpeg: &Path, output: &Path, source: &Path) -> Result<usize, String> {
    use lofty::{ape::{ApeItem, ApeTag}, config::WriteOptions, tag::ItemValue, wavpack::WavPackFile};
    let Some(c) = source_cover(ffmpeg, source) else { return Ok(0) };
    let mut f = std::fs::File::open(output).map_err(|e| format!("cannot open the output: {e}"))?;
    let mut file = WavPackFile::read_from(&mut f, ParseOptions::new().parsing_mode(ParsingMode::Relaxed))
        .map_err(|e| format!("tag patch failed: {e}"))?;
    drop(f);
    let ext = if c.mime.contains("png") { "png" } else { "jpg" };
    let mut value = format!("cover.{ext}").into_bytes();
    value.push(0);
    value.extend_from_slice(&c.data);
    let item = ApeItem::new("Cover Art (Front)".to_owned(), ItemValue::Binary(value)).map_err(|e| e.to_string())?;
    if file.ape().is_none() {
        file.set_ape(ApeTag::new());
    }
    file.ape_mut().ok_or("no APE tag")?.insert(item);
    file.save_to_path(output, WriteOptions::new()).map_err(|e| format!("tag patch failed: {e}"))?;
    Ok(1)
}

/// A TTA's cover: an `APIC` frame in an ID3v2 tag at the start of the file, where the TTA format puts
/// one (ffmpeg writes the text fields as an APEv2 tag at the end, which holds no picture it can read
/// back; the tag library has no TTA support, so the frame is built by hand; v2.3 and v2.4 differ only
/// in how a frame's size is written).
fn tta_cover(ffmpeg: &Path, output: &Path, source: &Path) -> Result<usize, String> {
    let Some(c) = source_cover(ffmpeg, source) else { return Ok(0) };
    let mut b = std::fs::read(output).map_err(|e| format!("cannot read the output: {e}"))?;
    let syncsafe = |n: usize| [((n >> 21) & 0x7f) as u8, ((n >> 14) & 0x7f) as u8, ((n >> 7) & 0x7f) as u8, (n & 0x7f) as u8];
    if b.len() < 10 || &b[..3] != b"ID3" {
        // No tag yet: an empty v2.4 one to put the frame in.
        let mut head = b"ID3\x04\x00\x00".to_vec();
        head.extend_from_slice(&syncsafe(0));
        head.extend_from_slice(&b);
        b = head;
    }
    let version = b[3];
    if version != 3 && version != 4 {
        return Err(format!("unsupported ID3v2.{version} tag"));
    }
    let tag_size = (usize::from(b[6] & 0x7f) << 21) | (usize::from(b[7] & 0x7f) << 14) | (usize::from(b[8] & 0x7f) << 7) | usize::from(b[9] & 0x7f);
    let mut body = vec![0u8]; // ISO-8859-1
    body.extend_from_slice(c.mime.as_bytes());
    body.push(0);
    body.push(3); // front cover
    body.push(0); // no description
    body.extend_from_slice(&c.data);
    let mut frame = b"APIC".to_vec();
    if version == 4 {
        frame.extend_from_slice(&syncsafe(body.len()));
    } else {
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    }
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&body);
    let end = 10 + tag_size;
    let mut out = b[..end].to_vec();
    out.extend_from_slice(&frame);
    let new_size = syncsafe(tag_size + frame.len());
    out[6..10].copy_from_slice(&new_size);
    out.extend_from_slice(&b[end..]);
    std::fs::write(output, out).map_err(|e| format!("cannot write the output: {e}"))?;
    Ok(1)
}

/// What a source holds that ffmpeg's banner does not show, as (Mp3tag name, text), read with the tag
/// library so a conversion does not lose it: the language of any file (ffmpeg keeps it out of the
/// listing - it is a stream property to it - and the writers that go by the listing never see it),
/// and an M4A's conductor, publisher, encoded-by and BPM (atoms its demuxer does not expose).
/// `have` is what ffmpeg already reported; a field it reported is never repeated.
pub fn source_only_fields(source: &Path, have: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !have.keys().any(|k| k.eq_ignore_ascii_case("language")) {
        if let Some(lang) = lofty::read_from_path(source).ok().and_then(|f| {
            let tag = f.primary_tag().or_else(|| f.first_tag())?;
            tag.get_string(ItemKey::Language).map(str::to_owned)
        }) {
            let lang = lang.trim().to_owned();
            if !lang.is_empty() {
                out.push(("LANGUAGE".to_owned(), lang));
            }
        }
    }
    // A WMA keeps it as `WM/Language`, which ffmpeg treats the same way.
    if out.is_empty() && source.extension().is_some_and(|e| e.eq_ignore_ascii_case("wma") || e.eq_ignore_ascii_case("asf")) {
        if let Ok(attrs) = crate::asf::attributes(source) {
            if let Some((_, v)) = attrs.into_iter().find(|(k, v)| k == "WM/Language" && !v.trim().is_empty()) {
                out.push(("LANGUAGE".to_owned(), v.trim().to_owned()));
            }
        }
    }
    out.extend(mp4_dropped_fields(source, have));
    // Timed lyrics held in an ID3 `SYLT` frame: ffmpeg does not list it, so a conversion lost them. They go on as `.lrc` text.
    if let Some(text) = source_sylt_text(source) {
        out.push(("LYRICS".to_owned(), text));
    }
    out
}

fn mp4_dropped_fields(source: &Path, have: &HashMap<String, String>) -> Vec<(String, String)> {
    const ATOMS: [&str; 4] = ["\u{a9}con", "\u{a9}pub", "\u{a9}enc", "tmpo"];
    let Ok(mut f) = std::fs::File::open(source) else { return Vec::new() };
    let parse = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let Ok(file) = Mp4File::read_from(&mut f, parse) else { return Vec::new() };
    let ilst = file.ilst();
    let Some(ilst) = ilst else { return Vec::new() };
    let mut out = Vec::new();
    // The language: a free-form atom (the table gives it no atom of its own).
    if !have.keys().any(|k| k.eq_ignore_ascii_case("language")) {
        let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: "LANGUAGE".into() };
        let text = ilst.get(&ident).and_then(|a| a.data().find_map(|d| if let AtomData::UTF8(t) | AtomData::UTF16(t) = d { Some(t.trim().to_owned()) } else { None }));
        if let Some(t) = text.filter(|t| !t.is_empty()) {
            out.push(("LANGUAGE".to_owned(), t));
        }
    }
    for fd in crate::mapping::FIELDS {
        let Some(atom) = fd.mp4.filter(|a| ATOMS.contains(a)) else { continue };
        // ffmpeg already hands the field over: nothing to read. ORGANIZATION is the one spelling that does not
        // count - it names the same field but can hold another value than the atom (a PUBLISHER next to it).
        if have.keys().any(|k| !k.eq_ignore_ascii_case("ORGANIZATION") && crate::mapping::find_by_name(k).is_some_and(|x| std::ptr::eq(x, fd))) {
            continue;
        }
        let Some(bytes) = crate::mapping::atom_bytes(atom) else { continue };
        let Some(a) = ilst.get(&AtomIdent::Fourcc(bytes)) else { continue };
        let text = a.data().find_map(|d| match d {
            AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
            AtomData::SignedInteger(n) => Some(n.to_string()),
            AtomData::UnsignedInteger(n) => Some(n.to_string()),
            AtomData::Unknown { data, .. } if !data.is_empty() && data.len() <= 4 => {
                Some(data.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)).to_string())
            }
            _ => None,
        });
        if let Some(t) = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty() && t != "0") {
            out.push((fd.name.to_owned(), t));
        }
    }
    out
}

/// What `source` holds: its fields that carry more than one text value (keyed by generic name), and
/// its whole generic tag (every field the library knows, every picture).
fn read_source(source: &Path) -> Result<(HashMap<ItemKey, Vec<String>>, Option<lofty::tag::Tag>), String> {
    let src = lofty::probe::Probe::open(source)
        .and_then(|p| p.options(ParseOptions::new().parsing_mode(ParsingMode::Relaxed)).read())
        .map_err(|e| format!("cannot read the source tags: {e}"))?;
    let mut groups: HashMap<ItemKey, Vec<String>> = HashMap::new();
    let tag = src.primary_tag().or_else(|| src.first_tag()).cloned();
    if let Some(tag) = &tag {
        for item in tag.items() {
            if let ItemValue::Text(t) = item.value() {
                if !t.trim().is_empty() {
                    groups.entry(item.key()).or_default().push(t.clone());
                }
            }
        }
    }
    groups.retain(|_, v| v.len() > 1);
    Ok((groups, tag))
}

/// Field names ffmpeg keeps in lower case (`title`, `date`, `album`...) written the way every tagger
/// shows a Vorbis field, in capitals. Vorbis ignores the case, so this changes no meaning; it makes a
/// FLAC made from an MP3 carry the same names as one made from a FLAC. Names that already hold a
/// capital are left exactly as they are (a custom field keeps the case its owner gave it), and so is
/// `encoder`.
fn upper_vorbis_names(vc: &mut VorbisComments) -> usize {
    let lower: Vec<String> = vc
        .items()
        .map(|(k, _)| k.to_owned())
        .filter(|k| !k.eq_ignore_ascii_case("encoder") && k.chars().any(|c| c.is_ascii_lowercase()) && !k.chars().any(|c| c.is_ascii_uppercase()))
        .collect();
    let mut n = 0;
    for k in lower {
        let values: Vec<String> = vc.remove(&k).collect();
        for v in values {
            vc.push(k.to_ascii_uppercase(), v);
        }
        n += 1;
    }
    n
}

fn patch_vorbis(vc: &mut VorbisComments, groups: &HashMap<ItemKey, Vec<String>>) -> usize {
    let mut n = 0;
    for (key, values) in groups {
        let Some(name) = key.map_key(TagType::VorbisComments) else { continue };
        vc.remove(name).for_each(drop);
        for v in values {
            vc.push(name.to_owned(), v.clone());
        }
        n += 1;
    }
    n
}

/// Write each repeated field as ONE text frame whose values are separated by a NUL - the way Mp3tag,
/// foobar2000 and others store several values, in 2.3 and 2.4 alike.
///
/// ⚠ CLAUDE: the frames are built by hand rather than through the generic tag. The generic
/// conversion keeps several values for some fields (the album artist) and silently keeps only the
/// FIRST for others (the album-artist SORT field, found on a real album 2026-10-13). It also removes
/// the `TXXX` copy ffmpeg made of a field it had no frame for, so the file carries each field once.
fn patch_id3(tag: &mut Id3v2Tag, groups: &HashMap<ItemKey, Vec<String>>, v24: bool) -> usize {
    let encoding = if v24 { TextEncoding::UTF8 } else { TextEncoding::UTF16 };
    let mut n = 0;
    for (key, values) in groups {
        let Some(frame) = key.map_key(TagType::Id3v2) else { continue };
        let joined = values.join("\u{0}");
        if let Some(desc) = frame.strip_prefix("TXXX:") {
            tag.insert_user_text(desc.to_owned(), joined);
        } else if frame.len() == 4 {
            let id = FrameId::Valid(frame.to_owned().into());
            let _ = tag.remove(&id).count();
            tag.insert(Frame::Text(TextInformationFrame::new(id, encoding, joined)));
        } else {
            continue;
        }
        // ffmpeg writes a field it has no frame for as `TXXX:<NAME>`, joined with a semicolon.
        if let Some(name) = key.map_key(TagType::VorbisComments) {
            let _ = tag.remove_user_text(name);
            let _ = tag.remove_user_text(&name.to_ascii_lowercase());
        }
        n += 1;
    }
    n
}

/// Names ffmpeg's MP4 muxer already writes as standard atoms; everything else a source carries is a
/// free-form field that ffmpeg drops from an M4A.
const MP4_STANDARD: [&str; 24] = [
    "title", "artist", "album_artist", "album", "date", "comment", "genre", "track", "disc", "composer",
    "copyright", "encoder", "grouping", "lyrics", "description", "synopsis", "show", "episode_id", "network",
    "performer", "year", "compilation", "media_type", "tracknumber",
];

/// The source's free-form fields: (NAME, value), upper-cased the way Vorbis and ID3 users write them.
fn custom_fields(tags: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = tags
        .iter()
        .filter(|(k, _)| !MP4_STANDARD.contains(&k.as_str()))
        .map(|(k, val)| (k.to_ascii_uppercase(), val.clone()))
        .collect();
    v.sort();
    v
}

/// The ID3 tag ffmpeg would write for this source, taken from a throwaway half-second MP3.
///
/// ⚠ CLAUDE: a WAV\'s ID3 chunk is built from THIS and not from the library\'s generic tag, so a
/// field keeps the name it has in every other output (the generic conversion renames some:
/// `RELEASETYPE` becomes `MusicBrainz Album Type`). ffmpeg\'s own WAV muxer cannot write an ID3 chunk.
fn donor_id3(ffmpeg: &Path, source: &Path, v24: bool, tag_args: &[String]) -> Result<Id3v2Tag, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("yaiol-id3-donor-{}-{nanos}.mp3", std::process::id()));
    let out = crate::ffmpeg::command(ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        // Long enough for the tag library to find MPEG frames in: 50 ms was not, for a WMA (its decoder
        // hands over its first block late and the donor came out with no parseable frame, so the WAV
        // got no ID3 chunk at all).
        .args(["-t", "0.5", "-map", "0:a:0"])
        .args(tag_args)
        .args(["-c:a", "libmp3lame", "-b:a", "32k"])
        .args(["-id3v2_version", if v24 { "4" } else { "3" }])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let result = if out.status.success() {
        let mut f = std::fs::File::open(&tmp).map_err(|e| e.to_string())?;
        MpegFile::read_from(&mut f, ParseOptions::new().parsing_mode(ParsingMode::Relaxed))
            .map_err(|e| e.to_string())
            .and_then(|mut m| m.id3v2_mut().map(|t| t.clone()).ok_or_else(|| "the donor has no ID3 tag".to_owned()))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    };
    let _ = std::fs::remove_file(&tmp);
    result
}

/// ID3v2.4-only frames, and the placeholder id each is written under in a v2.3 tag.
const PLACEHOLDERS: [(&str, &str); 3] = [("TZOA", "TSOA"), ("TZOP", "TSOP"), ("TZOT", "TSOT")];

/// Where the ID3 tag of a written file starts: 0 for an MP3, inside the `ID3 ` chunk of a WAV / AIFF.
fn id3_offset(path: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[..3] == b"ID3" {
        return Some(0);
    }
    let little = match &head[..4] {
        b"RIFF" => true,
        b"FORM" => false,
        _ => return None,
    };
    let mut pos = 12u64;
    loop {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 8];
        f.read_exact(&mut h).ok()?;
        let size = if little { u32::from_le_bytes([h[4], h[5], h[6], h[7]]) } else { u32::from_be_bytes([h[4], h[5], h[6], h[7]]) } as u64;
        if h[..4].eq_ignore_ascii_case(b"id3 ") {
            return Some(pos + 8);
        }
        pos += 8 + size + (size & 1);
    }
}

/// Rename the placeholder frames of a written ID3v2.3 tag to their real ids, in place: four bytes each
/// in the tag's own area, nothing else is touched. See `PLACEHOLDERS`.
fn fix_placeholder_frames(path: &Path) -> Result<usize, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let Some(at) = id3_offset(path) else { return Ok(0) };
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    let mut head = [0u8; 10];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    if &head[..3] != b"ID3" || head[3] != 3 {
        return Ok(0);
    }
    let size = ((head[6] as usize & 127) << 21) | ((head[7] as usize & 127) << 14) | ((head[8] as usize & 127) << 7) | (head[9] as usize & 127);
    let mut body = vec![0u8; size];
    f.read_exact(&mut body).map_err(|e| e.to_string())?;
    let mut changed = 0;
    let mut p = 0;
    while p + 10 <= body.len() {
        let id = [body[p], body[p + 1], body[p + 2], body[p + 3]];
        if id == [0; 4] {
            break;
        }
        let len = u32::from_be_bytes([body[p + 4], body[p + 5], body[p + 6], body[p + 7]]) as usize;
        if let Some((_, real)) = PLACEHOLDERS.iter().find(|(ph, _)| ph.as_bytes() == id) {
            f.seek(SeekFrom::Start(at + 10 + p as u64)).map_err(|e| e.to_string())?;
            f.write_all(real.as_bytes()).map_err(|e| e.to_string())?;
            changed += 1;
        }
        p += 10 + len;
    }
    Ok(changed)
}

/// Turn the custom text frames ffmpeg wrote for fields that have a frame of their own into that frame:
/// `TXXX:LYRICIST` becomes `TEXT`, `TXXX:album-sort` `TSOA`, `TXXX:USLT` a real `USLT`, and so on, from
/// Mp3tag's table (`mapping`). A real frame the file already has wins; the custom copy is then dropped -
/// unless it holds a DIFFERENT value (a FLAC with both `PUBLISHER` and `ORGANIZATION`, two spellings of one
/// field): then it stays custom, so no value is ever lost. A field the table does not know stays custom.
/// Returns how many were converted.
fn promote_custom_frames(tag: &mut Id3v2Tag, v24: bool, later: &[(String, String)]) -> usize {
    let encoding = if v24 { TextEncoding::UTF8 } else { TextEncoding::UTF16 };
    let customs: Vec<(String, String)> = tag
        .iter()
        .filter_map(|fr| match fr {
            Frame::UserText(t) => Some((t.description.to_string(), t.content.to_string())),
            _ => None,
        })
        .collect();
    let mut n = 0;
    for (desc, value) in customs {
        let Some(field) = crate::mapping::find_by_name(&desc) else { continue };
        let Some(id) = (if v24 { field.id3_v24 } else { field.id3_v23 }) else { continue };
        // Another spelling (`ORGANIZATION`) is not promoted when the field's own name is about to be written
        // (`later`: the fields added after this step): it would take the frame and be overwritten, losing its value.
        if !desc.eq_ignore_ascii_case(field.name)
            && later.iter().any(|(k, _)| crate::mapping::find_by_name(k).is_some_and(|f| std::ptr::eq(f, field)))
        {
            continue;
        }
        // The date frames are the date's job (see `ffmpeg::tag_arguments`), never a custom copy's.
        if matches!(id, "TYER" | "TDRC") {
            continue;
        }
        // ⚠ CLAUDE: lofty DISCARDS TSOA / TSOP / TSOT (and TMOO) when it writes ID3v2.3, though Mp3tag
        // writes them there and players read them. They go in under a placeholder id the library lets
        // through (`TZOA`...) and `fix_placeholder_frames` renames them in the written file.
        let placeholder = if v24 { None } else { PLACEHOLDERS.iter().find(|(_, real)| *real == id).map(|(p, _)| *p) };
        let frame_id = FrameId::Valid(placeholder.unwrap_or(id).to_owned().into());
        let real_id = FrameId::Valid(id.to_owned().into());
        let held = tag.get(&frame_id).or_else(|| tag.get(&real_id));
        // A real text frame the file already has, with another value: the custom copy is a second value, not a duplicate.
        if let Some(Frame::Text(t)) = held {
            if t.value.trim() != value.trim() {
                continue;
            }
        }
        if held.is_none() {
            match id {
                "USLT" => {
                    let frame = UnsynchronizedTextFrame::new(encoding, *b"eng", String::new(), value);
                    tag.insert(Frame::UnsynchronizedText(frame));
                }
                "COMM" => {
                    let frame = CommentFrame::new(encoding, *b"eng", String::new(), value);
                    tag.insert(Frame::Comment(frame));
                }
                _ => {
                    tag.insert(Frame::Text(TextInformationFrame::new(frame_id, encoding, value)));
                }
            }
        }
        let _ = tag.remove_user_text(&desc);
        n += 1;
    }
    n
}


// ── lyrics: SYLT and USLT ────────────────────────────────────────────────────

/// The first `SYLT` frame of an ID3 tag as `.lrc` text.
fn sylt_text(tag: &Id3v2Tag) -> Option<String> {
    tag.iter().find_map(|fr| match fr {
        Frame::Binary(b) if b.id().as_str() == "SYLT" => {
            let sylt = SynchronizedTextFrame::parse(&b.as_bytes(), FrameFlags::default()).ok()?;
            let entries: Vec<(u32, String)> = sylt
                .content
                .iter()
                .map(|(t, text)| (if sylt.timestamp_format == TimestampFormat::MPEG { (f64::from(*t) * 26.122) as u32 } else { *t }, text.clone()))
                .collect();
            (!entries.is_empty()).then(|| crate::lyrics::to_lrc(&entries))
        }
        _ => None,
    })
}

/// A source's `SYLT` (MP3, AIFF, the ID3 chunk of a WAV) as `.lrc` text.
fn source_sylt_text(source: &Path) -> Option<String> {
    let parse = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let mut f = std::fs::File::open(source).ok()?;
    let ext = source.extension()?.to_string_lossy().to_lowercase();
    let tag: Id3v2Tag = match ext.as_str() {
        "mp3" | "mp2" => MpegFile::read_from(&mut f, parse).ok()?.id3v2()?.clone(),
        "aif" | "aiff" | "aifc" => AiffFile::read_from(&mut f, parse).ok()?.id3v2()?.clone(),
        "wav" | "wave" => WavFile::read_from(&mut f, parse).ok()?.id3v2()?.clone(),
        _ => return None,
    };
    sylt_text(&tag)
}

/// The text the lyrics are written from: the last `LYRICS` of `added` (a `.lrc` embedded, or a source's `SYLT`),
/// and `added` without it - `add_id3` would write it as a custom frame.
fn split_lyrics(added: &[(String, String)]) -> (Option<String>, Vec<(String, String)>) {
    let text = added.iter().rev().find(|(n, _)| is_lyrics_name(n)).map(|(_, v)| v.clone());
    (text, added.iter().filter(|(n, _)| !is_lyrics_name(n)).cloned().collect())
}

/// Is `name` a spelling of the lyrics field (`LYRICS`, `UNSYNCEDLYRICS`, `USLT`)?
fn is_lyrics_name(name: &str) -> bool {
    crate::mapping::find_by_name(name).is_some_and(|fd| fd.name == "UNSYNCEDLYRICS")
}

/// Write the lyrics of an ID3 tag as the frames `mode` asks for. `text` is what the source or a `.lrc` brought; with
/// none, the tag's own `USLT` (ffmpeg's copy of the source's lyrics) is used. Every existing lyrics frame is replaced.
/// Auto = one frame by the text (timed -> `SYLT`, plain -> `USLT`); Duplicate = both; Uslt / Sylt = that one.
fn apply_lyrics(tag: &mut Id3v2Tag, v24: bool, mode: LyricsMode, strip_time: bool, text: Option<&str>) -> usize {
    let existing = tag.iter().find_map(|fr| match fr {
        Frame::UnsynchronizedText(f) if !f.content.trim().is_empty() => Some((f.content.to_string(), f.language)),
        _ => None,
    });
    let language = existing.as_ref().map_or(*b"eng", |(_, l)| *l);
    let Some(text) = text.map(str::to_owned).or_else(|| existing.map(|(t, _)| t)) else { return 0 };
    let timed = crate::lyrics::is_timed(&text);
    // The default path leaves a plain lyric exactly as it is.
    if mode == LyricsMode::Auto && !strip_time && !timed {
        return 0;
    }
    // Every lyrics frame goes, whatever its kind: the plain and the synchronised ones and a custom `LYRICS` frame an
    // older copy of the same text may still sit in.
    let spellings = crate::mapping::spellings("LYRICS");
    tag.retain(|fr| match fr {
        Frame::UnsynchronizedText(_) => false,
        Frame::Binary(b) => b.id().as_str() != "SYLT",
        Frame::UserText(t) => !spellings.iter().any(|s| s.eq_ignore_ascii_case(&t.description)),
        _ => true,
    });
    let (uslt, sylt) = match mode {
        LyricsMode::Auto => (!timed, timed),
        LyricsMode::Duplicate => (true, true),
        LyricsMode::Uslt => (true, false),
        LyricsMode::Sylt => (false, true),
    };
    let encoding = if v24 { TextEncoding::UTF8 } else { TextEncoding::UTF16 };
    let mut n = 0;
    if uslt {
        let words = if strip_time { crate::lyrics::strip_stamps(&text) } else { text.clone() };
        tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(encoding, language, String::new(), words)));
        n += 1;
    }
    if sylt {
        let entries: Vec<(u32, String)> = if timed {
            crate::lyrics::timed_lines(&text)
        } else {
            crate::lyrics::plain_lines(&text).into_iter().map(|l| (0, l)).collect()
        };
        if !entries.is_empty() {
            let frame = SynchronizedTextFrame::new(encoding, language, TimestampFormat::MS, SyncTextContentType::Lyrics, None, entries);
            if let Ok(bytes) = frame.as_bytes(WriteOptions::new().use_id3v23(!v24)) {
                tag.insert(Frame::Binary(BinaryFrame::new(FrameId::Valid("SYLT".into()), bytes)));
                n += 1;
            }
        }
    }
    n
}

/// Give the tag the source's WHOLE date. ffmpeg writes only the year when it writes ID3v2.3 (`TYER=2024`
/// for `2024-12-31T23:59:59`), so the day and the time of the source were lost on every MP3, AIFF and WAV.
/// The tag library already writes a full timestamp the v2.3 way (`TYER` + `TDAT` day-month + `TIME`
/// hour-minute; v2.4's `TDRC` keeps it whole), so it only has to be handed the date. A date that is only
/// a year, or that does not start with the year the tag holds, is left alone: the tag is what the
/// cleaning and the stray-frame repair have already decided. Returns 1 when the date was widened.
fn restore_full_date(tag: &mut Id3v2Tag, source_tags: &HashMap<String, String>) -> usize {
    let get = |k: &str| source_tags.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim());
    // The same rule as `ffmpeg::tag_arguments`: `date`, else the whole date a source kept in a `TYER`.
    let Some(raw) = get("date").filter(|v| !v.is_empty()).or_else(|| get("TYER").filter(|v| !v.is_empty())) else { return 0 };
    let Ok(full) = raw.replace(' ', "T").parse::<Timestamp>() else { return 0 };
    if full.month.is_none() {
        return 0;
    }
    let id = FrameId::Valid("TDRC".into());
    let held = match tag.get(&id) {
        Some(Frame::Timestamp(f)) => f.timestamp,
        _ => return 0,
    };
    if held.year != full.year || held.month.is_some() {
        return 0;
    }
    tag.insert(Frame::Timestamp(TimestampFrame::new(id, TextEncoding::UTF16, full)));
    1
}

/// Write free-form fields as the iTunes-style `----:com.apple.iTunes:NAME` atoms every MP4 tagger
/// reads. A field the file already has is left alone.
fn write_customs(ilst: &mut Ilst, customs: &[(String, String)]) -> usize {
    let mut n = 0;
    // A field's own name goes before another spelling of it (`PUBLISHER` before `ORGANIZATION`): when a source
    // holds both, the real one takes the atom and the other stays a free-form field.
    let mut ordered: Vec<&(String, String)> = customs.iter().collect();
    ordered.sort_by_key(|(name, _)| crate::mapping::find_by_name(name).is_some_and(|fd| !fd.name.eq_ignore_ascii_case(name)));
    for (name, value) in ordered {
        // A field Mp3tag's table gives an atom of its own is written as that atom, not free-form.
        if let Some(atom) = crate::mapping::find_by_name(name).and_then(|fd| fd.mp4).and_then(crate::mapping::atom_bytes) {
            let ident = AtomIdent::Fourcc(atom);
            if ilst.get(&ident).is_none() {
                let data = match &atom {
                    // tmpo holds a whole number; a BPM that is not one (128.5) is kept free-form, not lost.
                    b"tmpo" => value.trim().parse::<u16>().ok().map(|bpm| AtomData::Unknown {
                        code: lofty::mp4::DataType::BeSignedInteger,
                        data: bpm.to_be_bytes().to_vec(),
                    }),
                    b"cpil" => Some(AtomData::Bool(matches!(value.trim(), "1" | "true" | "yes"))),
                    // These are written by ffmpeg or hold numbers: not made from text here.
                    b"trkn" | b"disk" => None,
                    _ => Some(AtomData::UTF8(value.clone())),
                };
                if let Some(data) = data {
                    ilst.insert(Atom::new(ident, data));
                    n += 1;
                    continue;
                }
                // No atom could be made from the text: fall through to a free-form one - except the
                // two number atoms, which ffmpeg writes itself.
                if matches!(&atom, b"trkn" | b"disk") {
                    continue;
                }
            } else {
                // The atom is there already. The same text is a duplicate (dropped); another text is a second
                // value under another spelling (`PUBLISHER` and `ORGANIZATION`) and is kept as a free-form one.
                let same = ilst.get(&ident).is_some_and(|a| {
                    a.data().any(|d| matches!(d, AtomData::UTF8(s) | AtomData::UTF16(s) if s.trim() == value.trim()))
                });
                if same || matches!(&atom, b"trkn" | b"disk" | b"tmpo" | b"cpil") {
                    continue;
                }
            }
        }
        let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: name.clone().into() };
        if ilst.get(&ident).is_some() {
            continue;
        }
        ilst.insert(Atom::new(ident, AtomData::UTF8(value.clone())));
        n += 1;
    }
    n
}

/// `ilst` identifiers are either a four-byte code (`©ART`) or a free-form `----:mean:name`.
fn ilst_ident(name: &str) -> Option<AtomIdent<'static>> {
    if let Some(rest) = name.strip_prefix("----:") {
        let (mean, field) = rest.split_once(':')?;
        return Some(AtomIdent::Freeform { mean: mean.to_owned().into(), name: field.to_owned().into() });
    }
    let bytes: Vec<u8> = name.chars().map(|c| u8::try_from(u32::from(c)).ok()).collect::<Option<_>>()?;
    Some(AtomIdent::Fourcc(bytes.try_into().ok()?))
}

fn patch_ilst(ilst: &mut Ilst, groups: &HashMap<ItemKey, Vec<String>>) -> usize {
    let mut n = 0;
    for (key, values) in groups {
        let Some(ident) = key.map_key(TagType::Mp4Ilst).and_then(ilst_ident) else { continue };
        ilst.remove(&ident).for_each(drop);
        let mut it = values.iter();
        let Some(first) = it.next() else { continue };
        let mut atom = Atom::new(ident, AtomData::UTF8(first.clone()));
        for v in it {
            atom.push_data(AtomData::UTF8(v.clone()));
        }
        ilst.insert(atom);
        n += 1;
    }
    n
}

/// Restore the repeated fields of `source` on `output` (already written by ffmpeg).
///
/// `Ok(None)`: nothing to do (format not handled, no tag, or no repeated field).
///
/// ⚠ CLAUDE: each family is patched through ITS OWN tag type (Vorbis comments, `ilst`, ID3v2),
/// never through the generic tag: the generic one cannot hold a free-form field and silently
/// drops it on a save - found 2026-10-13 when a custom `AMLP_ID` vanished from FLAC, M4A, Ogg and
/// Opus outputs (test: repeated_and_free_form_fields_survive_in_every_format_that_can_hold_them).
pub fn restore_repeated_fields(
    ffmpeg: &Path,
    source: &Path,
    output: &Path,
    preset: &Preset,
    cleanup: &TagCleanup,
    source_tags: &HashMap<String, String>,
    ignored: &[String],
    source_kind: crate::ffmpeg::SourceKind,
    // Fields an effect writes (ReplayGain). They are added LAST, after the cleaning, so a Tags entry
    // that deletes everything but a list cannot take them away.
    added: &[(String, String)],
) -> Result<Option<Report>, String> {
    let format = preset.format;
    let id3 = preset.id3_version;
    if format == Format::Wma {
        // ASF is not a lofty family: its header is rewritten by the engine's own module.
        let cover = if cleanup.remove_cover { None } else { source_cover(ffmpeg, source) };
        let n = crate::asf::finish(output, cleanup, cover.as_ref(), added)?;
        return Ok(Some(Report { restored: n }));
    }
    // Formats the tag library cannot write as a whole still get their picture here.
    if !cleanup.remove_cover {
        match format {
            Format::WavPack => return wavpack_cover(ffmpeg, output, source).map(|n| Some(Report { restored: n })),
            Format::Tta => return tta_cover(ffmpeg, output, source).map(|n| Some(Report { restored: n })),
            _ => {}
        }
    }
    let Some(target) = target_tag_type(format) else { return Ok(None) };
    // A source lofty cannot parse (some taggers write a WAV it rejects) is not an error: ffmpeg has
    // already carried its fields, and a WAV output still gets its ID3 chunk built from ffmpeg's view.
    let (groups, source_tag) = read_source(source).unwrap_or_default();
    let customs = if target == TagType::Mp4Ilst {
        custom_fields(&crate::ffmpeg::effective_tags(preset, source_tags, ignored, source_kind))
    } else {
        Vec::new()
    };
    let removing = cleanup.edits_tags();
    // A WAV always gets its ID3 chunk written: ffmpeg wrote only the RIFF INFO list.
    // An ID3 file is always looked at: ffmpeg writes some fields as custom frames (promoted below).
    let id3_target = target == TagType::Id3v2;
    // A Vorbis output is always looked at too: ffmpeg writes some of its names in lower case.
    let vorbis_target = target == TagType::VorbisComments;
    if groups.is_empty() && customs.is_empty() && !removing && format != Format::Wav && !id3_target && !vorbis_target && added.is_empty() {
        return Ok(None);
    }
    // ID3v2.3 holds several values too - as one UTF-16 frame with a NUL between them, the convention
    // Mp3tag and foobar2000 read and write - so the version is the user's choice, not a consequence.
    let write = WriteOptions::new().use_id3v23(id3 == Id3Version::V23);
    // Relaxed: a tagger's slightly malformed frame (Mp3tag writes a COMM some strict readers refuse)
    // must not make the whole tag layer give up.
    let parse = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let io = |e: std::io::Error| format!("cannot open the output: {e}");
    let err = |e: lofty::error::FileParseError| format!("tag patch failed: {e}");

    let restored = match (format, target) {
        (Format::Flac, _) => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = FlacFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let vc = file.vorbis_comments_mut().ok_or("no vorbis comments")?;
            let n = upper_vorbis_names(vc) + patch_vorbis(vc, &groups) + remove_listed_vorbis(vc, cleanup) + add_vorbis(vc, added);
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            n
        }
        (Format::Opus, _) => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = OpusFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let vc = file.vorbis_comments_mut();
            let n = upper_vorbis_names(vc) + patch_vorbis(vc, &groups) + remove_listed_vorbis(vc, cleanup) + add_vorbis(vc, added);
            let n = n + add_vorbis_cover(vc, ffmpeg, source, cleanup);
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            n
        }
        (Format::Ogg, _) => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = VorbisFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let vc = file.vorbis_comments_mut();
            let n = upper_vorbis_names(vc) + patch_vorbis(vc, &groups) + remove_listed_vorbis(vc, cleanup) + add_vorbis(vc, added);
            let n = n + add_vorbis_cover(vc, ffmpeg, source, cleanup);
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            n
        }
        (_, TagType::Mp4Ilst) => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = Mp4File::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let ilst = file.ilst_mut().ok_or("no ilst atom")?;
            let mut n = patch_ilst(ilst, &groups);
            n += write_customs(ilst, &customs);
            n += remove_listed_ilst(ilst, cleanup);
            n += add_ilst(ilst, added);
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            n
        }
        (Format::Aiff, _) => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = AiffFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let tag = file.id3v2_mut().ok_or("no ID3 chunk")?;
            let mut n = promote_custom_frames(tag, id3 == Id3Version::V24, added);
            n += restore_full_date(tag, source_tags);
            n += patch_id3(tag, &groups, id3 == Id3Version::V24) + remove_listed_id3(tag, cleanup);
            let (lyrics_text, rest) = split_lyrics(added);
            n += add_id3(tag, &rest, id3 == Id3Version::V24);
            n += apply_lyrics(tag, id3 == Id3Version::V24, cleanup.lyrics_mode, cleanup.lyrics_strip_time, lyrics_text.as_deref());
            if !cleanup.remove_cover && tag.iter().all(|fr| !matches!(fr, Frame::Picture(_))) {
                if let Some(pic) = cover_picture(ffmpeg, source) {
                    tag.insert_picture(pic);
                    n += 1;
                }
            }
            if n == 0 {
                return Ok(None);
            }
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            fix_placeholder_frames(output)?;
            n
        }
        (Format::Wav, _) => {
            // The ID3 chunk is built from the SOURCE's whole tag - fields, repeated values, the
            // picture - then the custom fields ffmpeg's tag list carried, then the user's removals.
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = WavFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let tag_args = crate::ffmpeg::tag_arguments(preset, cleanup, source_tags, ignored, source_kind);
            let mut id3_tag = match donor_id3(ffmpeg, source, id3 == Id3Version::V24, &tag_args) {
                Ok(tag) => tag,
                Err(_) => match &source_tag {
                    Some(t) => Id3v2Tag::from(t.clone()),
                    None => Id3v2Tag::new(),
                },
            };
            // The picture is not in the donor (it is audio only): it comes from the source tag.
            if !cleanup.remove_cover {
                let mut placed = false;
                if let Some(t) = &source_tag {
                    for pic in t.pictures() {
                        id3_tag.insert_picture(pic.clone());
                        placed = true;
                    }
                }
                // A source the tag library cannot read, or reads without its picture (a WMA, a TTA, a
                // WavPack), still has one: ffmpeg or the ASF reader finds it.
                if !placed {
                    if let Some(cover) = source_cover(ffmpeg, source) {
                        if let Ok(mut pic) = lofty::picture::Picture::from_reader(&mut cover.data.as_slice()) {
                            pic.set_pic_type(lofty::picture::PictureType::CoverFront);
                            id3_tag.insert_picture(pic);
                        }
                    }
                }
            }
            let mut n = promote_custom_frames(&mut id3_tag, id3 == Id3Version::V24, added);
            n += restore_full_date(&mut id3_tag, source_tags);
            n += patch_id3(&mut id3_tag, &groups, id3 == Id3Version::V24);
            n += remove_listed_id3(&mut id3_tag, cleanup);
            let (lyrics_text, rest) = split_lyrics(added);
            n += add_id3(&mut id3_tag, &rest, id3 == Id3Version::V24);
            n += apply_lyrics(&mut id3_tag, id3 == Id3Version::V24, cleanup.lyrics_mode, cleanup.lyrics_strip_time, lyrics_text.as_deref());
            file.set_id3v2(id3_tag);
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            fix_placeholder_frames(output)?;
            n.max(1)
        }
        _ => {
            let mut f = std::fs::File::open(output).map_err(io)?;
            let mut file = MpegFile::read_from(&mut f, parse).map_err(err)?;
            drop(f);
            let tag = file.id3v2_mut().ok_or("no ID3v2 tag")?;
            let mut n = promote_custom_frames(tag, id3 == Id3Version::V24, added);
            n += restore_full_date(tag, source_tags);
            n += patch_id3(tag, &groups, id3 == Id3Version::V24) + remove_listed_id3(tag, cleanup);
            let (lyrics_text, rest) = split_lyrics(added);
            n += add_id3(tag, &rest, id3 == Id3Version::V24);
            n += apply_lyrics(tag, id3 == Id3Version::V24, cleanup.lyrics_mode, cleanup.lyrics_strip_time, lyrics_text.as_deref());
            if n == 0 {
                return Ok(None);
            }
            file.save_to_path(output, write).map_err(|e| format!("tag patch failed: {e}"))?;
            fix_placeholder_frames(output)?;
            n
        }
    };
    Ok(Some(Report { restored }))
}

/// Does `name` match `pattern`? Case-insensitive; `*` stands for any run of characters.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.trim().to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    // The classic wildcard walk with one backtrack point.
    let (mut pi, mut ni, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}


/// The field names a tag editor shows, for the suggestions under the list of tags to remove (Mp3tag and
/// Vorbis spellings, the MusicBrainz Picard set). It is a help for typing, not a filter: any name may
/// be entered, including a custom field.
const KNOWN_NAMES: &[&str] = &[
    "ACOUSTID_FINGERPRINT", "ACOUSTID_ID", "ALBUM", "ALBUMARTIST", "ALBUMARTISTSORT", "ALBUMSORT", "ARTIST",
    "ARTISTS", "ARTISTSORT", "ASIN", "BARCODE", "BPM", "CATALOGNUMBER", "COMMENT", "COMPILATION", "COMPOSER",
    "COMPOSERSORT", "CONDUCTOR", "CONTENTGROUP", "COPYRIGHT", "DATE", "DISCNUMBER", "DISCSUBTITLE", "DISCTOTAL",
    "ENCODEDBY", "ENCODER", "ENCODERSETTINGS", "ENGINEER", "GENRE", "GROUPING", "INITIALKEY", "ISRC", "LABEL",
    "LANGUAGE", "LYRICIST", "LYRICS", "MEDIA", "MIXER", "MOOD", "MOVEMENT", "MOVEMENTNAME", "MOVEMENTTOTAL",
    "MUSICBRAINZ_ALBUMARTISTID", "MUSICBRAINZ_ALBUMID", "MUSICBRAINZ_ARTISTID", "MUSICBRAINZ_RELEASEGROUPID",
    "MUSICBRAINZ_RELEASETRACKID", "MUSICBRAINZ_TRACKID", "MUSICBRAINZ_WORKID", "ORIGINALALBUM", "ORIGINALARTIST",
    "ORIGINALDATE", "ORIGINALYEAR", "PERFORMER", "PRODUCER", "PUBLISHER", "RATING", "RELEASECOUNTRY",
    "RELEASESTATUS", "RELEASETYPE", "REMIXER", "REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_PEAK",
    "REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_PEAK", "SCRIPT", "SUBTITLE", "TITLE", "TITLESORT", "TOTALDISCS",
    "TOTALTRACKS", "TRACK", "TRACKNUMBER", "TRACKTOTAL", "UNSYNCEDLYRICS", "WORK", "WRITER", "YEAR",
];

/// Names to offer while `typed` is being written: those that start with it first, then those that merely
/// contain it, case-insensitive, without the ones `taken` already holds and without a name already
/// complete. Nothing for an empty entry or one with a wildcard - that is a pattern, not a name.
pub fn suggest_names(typed: &str, taken: &[String], limit: usize) -> Vec<&'static str> {
    let needle = typed.trim().to_uppercase();
    if needle.is_empty() || needle.contains('*') {
        return Vec::new();
    }
    let free = |n: &str| n != needle && !taken.iter().any(|t| t.trim().eq_ignore_ascii_case(n));
    let (mut starts, mut inside) = (Vec::new(), Vec::new());
    let table = crate::mapping::FIELDS.iter().map(|fd| fd.name);
    let mut seen: Vec<&str> = Vec::new();
    for n in table.chain(KNOWN_NAMES.iter().copied()).filter(|n| free(n)) {
        if seen.contains(&n) {
            continue;
        }
        seen.push(n);
        if n.starts_with(&needle) {
            starts.push(n);
        } else if n.contains(&needle) {
            inside.push(n);
        }
    }
    starts.extend(inside);
    starts.truncate(limit);
    starts
}

/// Every name a field answers to, so one entry in the list hits it whichever program's spelling was
/// typed: the frame id (`TSSE`), the Vorbis / Mp3tag name (`ENCODERSETTINGS`), or the description
/// of a user-text frame (`AMLP_ID`).
fn aliases(own: &str, own_family: TagType) -> Vec<String> {
    let mut v = vec![own.to_owned()];
    for s in crate::mapping::spellings(own) {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
            v.push(s);
        }
    }
    let key = ItemKey::from_key(own_family, &own.to_ascii_uppercase()).or_else(|| ItemKey::from_key(own_family, own));
    if let Some(key) = key {
        for family in [TagType::VorbisComments, TagType::Id3v2, TagType::Mp4Ilst] {
            if let Some(n) = key.map_key(family) {
                if !v.iter().any(|x| x.eq_ignore_ascii_case(n)) {
                    v.push(n.to_owned());
                }
            }
        }
    }
    v
}

pub(crate) fn listed(patterns: &[String], names: &[String]) -> bool {
    patterns.iter().any(|p| !p.trim().is_empty() && names.iter().any(|n| name_matches(p, n)))
}

// ⚠ CLAUDE: the three `remove_listed_*` below drop a FIELD by name and never the picture: the cover is
// its own switch (`remove_cover`, applied where the picture is taken from the source), so "delete every
// tag" must not take it along.
/// Ogg Vorbis and Opus carry a cover as a `METADATA_BLOCK_PICTURE` comment, which ffmpeg does not write.
fn add_vorbis_cover(vc: &mut VorbisComments, ffmpeg: &Path, source: &Path, cleanup: &TagCleanup) -> usize {
    use lofty::ogg::OggPictureStorage;
    if cleanup.remove_cover || !vc.pictures().is_empty() {
        return 0;
    }
    let Some(pic) = cover_picture(ffmpeg, source) else { return 0 };
    let info = lofty::picture::PictureInformation::from_picture(&pic).ok();
    vc.insert_picture(pic, info).map_or(0, |_| 1)
}

/// Put the fields an effect writes into a Vorbis comment block, replacing a field of the same name
/// the source brought along (a ReplayGain tag from the source describes the SOURCE, not this file).
fn add_vorbis(vc: &mut VorbisComments, added: &[(String, String)]) -> usize {
    for (name, value) in added {
        // The lyrics added (an embedded .lrc, a source's SYLT) replace every spelling of the field (LYRICS, UNSYNCEDLYRICS).
        if is_lyrics_name(name) {
            for spelling in crate::mapping::spellings(name) {
                vc.remove(&spelling).for_each(drop);
            }
        }
        vc.remove(name).for_each(drop);
        vc.push(name.clone(), value.clone());
    }
    added.len()
}

/// Frames a field is never written as by `add_id3`: the v2.4-only sort frames need the placeholder
/// step of `promote_custom_frames`, and the date frames are the date's own business.
const ID3_KEPT_AS_CUSTOM: [&str; 9] = ["TSOA", "TSOP", "TSOT", "TMOO", "TYER", "TDAT", "TIME", "TDRC", "TSSE"];

/// The same for ID3: a custom text frame (`TXXX`) per field, any frame of that name replaced.
fn add_id3(tag: &mut Id3v2Tag, added: &[(String, String)], v24: bool) -> usize {
    let encoding = if v24 { TextEncoding::UTF8 } else { TextEncoding::UTF16 };
    for (name, value) in added {
        let same: Vec<String> = tag
            .iter()
            .filter_map(|fr| match fr {
                Frame::UserText(t) if t.description.eq_ignore_ascii_case(name) => Some(t.description.to_string()),
                _ => None,
            })
            .collect();
        for d in same {
            let _ = tag.remove_user_text(&d);
        }
        // A field the table gives a text frame (CONDUCTOR is TPE3) is written as that frame.
        let frame = crate::mapping::find_by_name(name)
            .and_then(|fd| if v24 { fd.id3_v24 } else { fd.id3_v23 })
            .filter(|id| id.len() == 4 && id.starts_with('T') && !ID3_KEPT_AS_CUSTOM.contains(id));
        match frame {
            Some(id) => {
                let fid = FrameId::Valid(id.to_owned().into());
                let _ = tag.remove(&fid).count();
                tag.insert(Frame::Text(TextInformationFrame::new(fid, encoding, value.clone())));
            }
            None => {
                tag.insert(Frame::UserText(ExtendedTextFrame::new(encoding, name.clone(), value.clone())));
            }
        }
    }
    added.len()
}

/// The same for MP4: the iTunes-style free-form atom every tagger reads.
fn add_ilst(ilst: &mut Ilst, added: &[(String, String)]) -> usize {
    for (name, value) in added {
        // A field the table gives an atom is written as that atom (`write_customs` knows how, and
        // leaves one the file already has alone).
        if let Some(fd) = crate::mapping::find_by_name(name).filter(|fd| fd.mp4.is_some()) {
            // Added lyrics are authoritative: the atom the file already holds (ffmpeg's copy of the source's lyrics) gives way.
            if let (true, Some(bytes)) = (is_lyrics_name(name), fd.mp4.and_then(crate::mapping::atom_bytes)) {
                let _ = ilst.remove(&AtomIdent::Fourcc(bytes));
            }
            write_customs(ilst, &[(name.clone(), value.clone())]);
            continue;
        }
        let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: name.clone().into() };
        let _ = ilst.remove(&ident);
        ilst.insert(Atom::new(ident, AtomData::UTF8(value.clone())));
    }
    added.len()
}

fn remove_listed_id3(tag: &mut Id3v2Tag, cleanup: &TagCleanup) -> usize {
    let before = tag.len();
    tag.retain(|frame| {
        let names = match frame {
            Frame::Picture(_) => return true,
            Frame::UserText(f) => vec![f.description.to_string()],
            other => aliases(other.id().as_str(), TagType::Id3v2),
        };
        !cleanup.drops(&names)
    });
    before - tag.len()
}

fn remove_listed_vorbis(vc: &mut VorbisComments, cleanup: &TagCleanup) -> usize {
    let mut keys: Vec<String> = vc.items().map(|(k, _)| k.to_owned()).collect();
    keys.sort();
    keys.dedup();
    let mut n = 0;
    for key in keys {
        if key.eq_ignore_ascii_case("METADATA_BLOCK_PICTURE") {
            continue;
        }
        if cleanup.drops(&aliases(&key, TagType::VorbisComments)) {
            n += vc.remove(&key).count();
        }
    }
    n
}

fn remove_listed_ilst(ilst: &mut Ilst, cleanup: &TagCleanup) -> usize {
    let mut removed = 0;
    ilst.retain(|atom| {
        let names = match atom.ident() {
            AtomIdent::Freeform { name, .. } => vec![name.to_string()],
            AtomIdent::Fourcc(b) => {
                if b == b"covr" {
                    return true;
                }
                let s: String = b.iter().map(|c| *c as char).collect();
                aliases(&s, TagType::Mp4Ilst)
            }
        };
        let keep = !cleanup.drops(&names);
        if !keep {
            removed += 1;
        }
        keep
    });
    removed
}

/// Every spelling a tagger might use for the field ffmpeg calls `key` (`album_artist`, `track`...):
/// its own name, the tagger's name for it, and the ID3 / Vorbis spellings of that.
pub(crate) fn tagger_names(key: &str) -> Vec<String> {
    let mut v = Vec::new();
    for candidate in crate::ffmpeg::tagger_spellings(key) {
        for n in aliases(&candidate, TagType::VorbisComments) {
            if !v.iter().any(|x: &String| x.eq_ignore_ascii_case(&n)) {
                v.push(n);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn suggestions_start_with_what_is_typed_then_contain_it() {
        let v = suggest_names("co", &[], 40);
        assert_eq!(v[0], "COMMENT");
        assert!(v.contains(&"COMPOSER") && v.contains(&"COPYRIGHT"));
        // A prefix match always ranks above a name that only contains the text.
        let v = suggest_names("artist", &[], 40);
        let first_inside = v.iter().position(|n| !n.starts_with("ARTIST")).unwrap_or(v.len());
        assert!(v[..first_inside].iter().all(|n| n.starts_with("ARTIST")));
        assert!(v[first_inside..].iter().all(|n| !n.starts_with("ARTIST")));
        assert!(v.contains(&"ARTISTS") && v.contains(&"ALBUMARTIST"));
    }

    #[test]
    fn suggestions_skip_what_is_taken_complete_or_a_pattern() {
        let taken = vec!["comment".to_owned()];
        assert!(!suggest_names("co", &taken, 40).contains(&"COMMENT"));
        assert!(!suggest_names("album", &[], 40).contains(&"ALBUM"));
        assert!(suggest_names("", &[], 40).is_empty());
        assert!(suggest_names("REPLAYGAIN_*", &[], 40).is_empty());
    }
    use super::*;

    #[test]
    fn wildcard_names() {
        assert!(name_matches("replaygain_*", "REPLAYGAIN_TRACK_GAIN"));
        assert!(name_matches("ENCODER*", "encodersettings"));
        assert!(name_matches("*GAIN", "replaygain_album_gain"));
        assert!(name_matches("comment", "COMMENT"));
        assert!(!name_matches("comment", "comments"));
        assert!(!name_matches("artist", "albumartist"));
        assert!(name_matches("*", "anything"));
    }

    #[test]
    fn one_entry_answers_to_every_spelling() {
        // The Mp3tag / Vorbis name and the ID3 frame id are the same field.
        let names = aliases("TSSE", TagType::Id3v2);
        assert!(listed(&["ENCODERSETTINGS".to_owned()], &names), "{names:?}");
        assert!(listed(&["tsse".to_owned()], &names));
    }

    #[test]
    fn handled_formats() {
        assert_eq!(target_tag_type(Format::Mp3), Some(TagType::Id3v2));
        assert_eq!(target_tag_type(Format::Flac), Some(TagType::VorbisComments));
        assert_eq!(target_tag_type(Format::Aac), Some(TagType::Mp4Ilst));
        assert_eq!(target_tag_type(Format::Wma), None);
        assert_eq!(target_tag_type(Format::Ac3), None);
    }
}


