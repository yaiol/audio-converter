//! What a WAV file's own chunks say, for the one question ffmpeg cannot answer: which of the fields
//! it reports came from the RIFF INFO list under an id it has no mapping for.
//!
//! Mp3tag, the reference, reads nine INFO ids (`IPRD` album, `IART` artist, `ICMT` comment, `ICOP`
//! copyright, `IGNR` genre, `ILNG` language, `INAM` title, `ITRK` track, `ICRD` year) and ignores every other
//! one - checked 2026-10-14 by opening a WAV that carries 122 ids, each valued with its own name. ffmpeg
//! maps those and a few more, and hands every other INFO id on as a field of its own (`ICMS`, `IMUS`,
//! `iaar`, `TORG`...), so a WAV converted to FLAC or MP3 came out with fields its owner never sees.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// The ids of the `LIST INFO` entries of the WAV at `path` that ffmpeg reported under their own
/// name (so `present` has them): the ones it has no mapping for. Empty for anything that is not a WAV,
/// and for a file that cannot be read.
pub fn unmapped_info_keys(path: &Path, present: &HashMap<String, String>) -> Vec<String> {
    let is_wav = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return Vec::new();
    }
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let Some((_, info)) = scan(&mut f) else { return Vec::new() };
    // ffmpeg lower-cases the keys it reports; the file may spell the id either way.
    info.iter().filter_map(|id| present.keys().find(|k| k.eq_ignore_ascii_case(id)).cloned()).collect()
}

/// Walk the top-level chunks (never reading the audio): (has an `id3 ` chunk, the ids inside LIST INFO).
fn scan<R: Read + Seek>(r: &mut R) -> Option<(bool, Vec<String>)> {
    let mut head = [0u8; 12];
    r.read_exact(&mut head).ok()?;
    if &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let (mut has_id3, mut info) = (false, Vec::new());
    loop {
        let mut h = [0u8; 8];
        if r.read_exact(&mut h).is_err() {
            break;
        }
        let size = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as u64;
        let id = &h[..4];
        if id.eq_ignore_ascii_case(b"id3 ") {
            has_id3 = true;
        }
        if id == b"LIST" && size >= 4 && size < 1_000_000 {
            let mut body = vec![0u8; size as usize];
            r.read_exact(&mut body).ok()?;
            if &body[..4] == b"INFO" {
                let mut q = 4usize;
                while q + 8 <= body.len() {
                    let key = String::from_utf8_lossy(&body[q..q + 4]).into_owned();
                    let n = u32::from_le_bytes([body[q + 4], body[q + 5], body[q + 6], body[q + 7]]) as usize;
                    info.push(key);
                    q += 8 + n + (n & 1);
                }
            }
            if size & 1 == 1 {
                r.seek(SeekFrom::Current(1)).ok()?;
            }
        } else {
            r.seek(SeekFrom::Current((size + (size & 1)) as i64)).ok()?;
        }
    }
    Some((has_id3, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        if body.len() % 2 == 1 {
            v.push(0);
        }
        v
    }

    fn wav(with_id3: bool) -> Vec<u8> {
        let mut info = b"INFO".to_vec();
        info.extend(chunk(b"ICMS", b"lab"));
        info.extend(chunk(b"INAM", b"T"));
        let mut body = b"WAVE".to_vec();
        body.extend(chunk(b"fmt ", &[0u8; 16]));
        body.extend(chunk(b"LIST", &info));
        body.extend(chunk(b"data", &[0u8; 5]));
        if with_id3 {
            body.extend(chunk(b"id3 ", b"ID3"));
        }
        let mut v = b"RIFF".to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn the_info_ids_of_a_wav_are_read_from_its_chunk_headers() {
        let (has, ids) = scan(&mut Cursor::new(wav(true))).unwrap();
        assert!(has);
        assert_eq!(ids, ["ICMS", "INAM"]);
        let (has, _) = scan(&mut Cursor::new(wav(false))).unwrap();
        assert!(!has);
        assert!(scan(&mut Cursor::new(b"not a wav at all".to_vec())).is_none());
    }
}
