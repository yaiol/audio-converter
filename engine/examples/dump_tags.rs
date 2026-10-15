//! Prints the RAW tag names a file carries, per tag family - what a tagger would show as the field
//! name - so the mapping of one field across formats can be read off real files.
//!
//!   cargo run -q -p audio-converter-engine --example dump_tags -- <files...>
//!
//! One line per field: `<family>\t<name>\t<value, 40 chars>`. Families: vorbis, id3, ilst, riff-info,
//! asf. Pictures are listed as `(picture)`.

use lofty::{
    config::ParseOptions,
    file::AudioFile,
    flac::FlacFile,
    mp4::{AtomData, AtomIdent, Mp4File},
    ogg::{OggPictureStorage, OpusFile, VorbisFile},
};
use std::{fs::File, path::Path};

fn short(s: &str) -> String {
    let one: String = s.chars().filter(|c| !c.is_control()).take(40).collect();
    one
}

/// Text of an ID3 text field: the encoding byte then the text (ISO-8859-1, UTF-16 with or without a
/// BOM, UTF-8).
fn decode(enc: u8, b: &[u8]) -> String {
    let s = match enc {
        1 | 2 => {
            let (le, body) = match b {
                [0xFF, 0xFE, rest @ ..] => (true, rest),
                [0xFE, 0xFF, rest @ ..] => (false, rest),
                rest => (enc == 1, rest),
            };
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
                .collect();
            String::from_utf16_lossy(&units)
        }
        3 => String::from_utf8_lossy(b).into_owned(),
        _ => b.iter().map(|c| *c as char).collect(),
    };
    s.trim_matches('\u{0}').to_owned()
}

/// The frames of the first ID3v2.3 / v2.4 tag found in `bytes` (the start of an MP3, or inside the
/// `id3 ` chunk of a WAV / AIFF), read raw - lofty would upgrade a v2.3 `TYER`/`TDAT` to `TDRC` and hide
/// what is really in the file.
fn id3(bytes: &[u8]) {
    let Some(at) = bytes.windows(4).position(|w| w[0] == b'I' && w[1] == b'D' && w[2] == b'3' && (w[3] == 3 || w[3] == 4)) else {
        return;
    };
    let v4 = bytes[at + 3] == 4;
    let syncsafe = |b: &[u8]| ((b[0] as usize & 127) << 21) | ((b[1] as usize & 127) << 14) | ((b[2] as usize & 127) << 7) | (b[3] as usize & 127);
    let end = (at + 10 + syncsafe(&bytes[at + 6..at + 10])).min(bytes.len());
    let mut p = at + 10;
    while p + 10 <= end {
        let id = String::from_utf8_lossy(&bytes[p..p + 4]).into_owned();
        if !id.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            break;
        }
        let size = if v4 { syncsafe(&bytes[p + 4..p + 8]) } else { u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize };
        let body = &bytes[p + 10..(p + 10 + size).min(bytes.len())];
        let (name, value) = match id.as_str() {
            "APIC" => (id.clone(), "(picture)".to_owned()),
            "TXXX" | "WXXX" if !body.is_empty() => {
                let enc = body[0];
                let rest = &body[1..];
                let wide = enc == 1 || enc == 2;
                let mut i = 0;
                while i < rest.len() && !(if wide { i + 1 < rest.len() && rest[i] == 0 && rest[i + 1] == 0 } else { rest[i] == 0 }) {
                    i += if wide { 2 } else { 1 };
                }
                let desc = decode(enc, &rest[..i.min(rest.len())]);
                let val = decode(enc, rest.get(i + if wide { 2 } else { 1 }..).unwrap_or(&[]));
                (format!("{id}:{desc}"), val)
            }
            "COMM" | "USLT" if body.len() > 4 => {
                let enc = body[0];
                let rest = &body[4..];
                let wide = enc == 1 || enc == 2;
                let mut i = 0;
                while i < rest.len() && !(if wide { i + 1 < rest.len() && rest[i] == 0 && rest[i + 1] == 0 } else { rest[i] == 0 }) {
                    i += if wide { 2 } else { 1 };
                }
                (id.clone(), decode(enc, rest.get(i + if wide { 2 } else { 1 }..).unwrap_or(&[])))
            }
            _ if id.starts_with('T') && !body.is_empty() => (id.clone(), decode(body[0], &body[1..])),
            _ => (id.clone(), String::new()),
        };
        println!("id3\t{name}\t{}", short(&value));
        p += 10 + size;
    }
}

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        println!("## {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let parse = ParseOptions::new();
        let open = || File::open(path).expect("open");
        match ext.as_str() {
            "flac" => {
                let f = FlacFile::read_from(&mut open(), parse).expect("flac");
                if let Some(vc) = f.vorbis_comments() {
                    for (k, v) in vc.items() {
                        println!("vorbis\t{k}\t{}", short(v));
                    }
                }
                if !f.pictures().is_empty() {
                    println!("vorbis\t(picture block)\t");
                }
            }
            "ogg" | "oga" => {
                if let Ok(f) = VorbisFile::read_from(&mut open(), ParseOptions::new()) {
                    for (k, v) in f.vorbis_comments().items() {
                        println!("vorbis\t{k}\t{}", short(v));
                    }
                } else {
                    println!("vorbis\t(not a Vorbis stream: FLAC in Ogg)\t");
                }
            }
            "opus" => {
                let f = OpusFile::read_from(&mut open(), parse).expect("opus");
                for (k, v) in f.vorbis_comments().items() {
                    println!("vorbis\t{k}\t{}", short(v));
                }
            }
            "mp3" | "aif" | "aiff" => id3(&std::fs::read(path).expect("read")),
            "wav" => {
                let bytes = std::fs::read(path).expect("read");
                id3(&bytes);
                // The INFO list, read raw: lofty refuses some WAVs a tagger wrote (an extensible fmt).
                let mut p = 12;
                while p + 8 <= bytes.len() {
                    let id = &bytes[p..p + 4];
                    let size = u32::from_le_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize;
                    let end = (p + 8 + size).min(bytes.len());
                    if id == b"LIST" && bytes.get(p + 8..p + 12) == Some(b"INFO") {
                        let mut q = p + 12;
                        while q + 8 <= end {
                            let key = String::from_utf8_lossy(&bytes[q..q + 4]).into_owned();
                            let n = u32::from_le_bytes([bytes[q + 4], bytes[q + 5], bytes[q + 6], bytes[q + 7]]) as usize;
                            let val = String::from_utf8_lossy(&bytes[q + 8..(q + 8 + n).min(end)]).into_owned();
                            println!("riff-info\t{key}\t{}", short(val.trim_matches('\u{0}')));
                            q += 8 + n + (n & 1);
                        }
                    }
                    p += 8 + size + (size & 1);
                }
            }
            "m4a" | "m4r" | "mp4" => {
                let f = Mp4File::read_from(&mut open(), parse).expect("m4a");
                if let Some(ilst) = f.ilst() {
                    for atom in ilst {
                        let name = match atom.ident() {
                            AtomIdent::Fourcc(b) => b.iter().map(|c| if *c == 0xA9 { '\u{a9}' } else { *c as char }).collect::<String>(),
                            AtomIdent::Freeform { mean, name } => format!("----:{mean}:{name}"),
                        };
                        let value = match atom.data().next() {
                            Some(AtomData::Picture(_)) => "(picture)".to_owned(),
                            Some(AtomData::UTF8(s)) | Some(AtomData::UTF16(s)) => short(s),
                            Some(_) => "(binary)".to_owned(),
                            None => String::new(),
                        };
                        println!("ilst\t{name}\t{value}");
                    }
                }
            }
            "wma" => match audio_converter_engine::asf::attributes(path) {
                Ok(a) => a.iter().for_each(|(n, v)| println!("asf	{n}	{}", short(v))),
                Err(e) => println!("asf	(error)	{e}"),
            },
            other => println!("(no reader for .{other})"),
        }
    }
}
