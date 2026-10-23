//! Lyrics: the `.lrc` text format, deciding whether a text is timed, the entries of an ID3 `SYLT` frame
//! and back, and the `.lrc` file that sits beside a source.
//!
//! A timed lyric is plain text whose lines start with a stamp - `[mm:ss.xx]`, `[mm:ss]` or `[mm:ss:xx]`.
//! A FLAC has no field for "synchronised lyrics": the `.lrc` text goes into the same `LYRICS` tag as a
//! plain lyric, and only the stamps tell the two apart. That is the whole rule of `is_timed`.

use std::path::{Path, PathBuf};

/// A stamp at byte `i` of `s`: its length in bytes and its time in milliseconds.
fn stamp_at(s: &str, i: usize) -> Option<(usize, u32)> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    let m0 = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == m0 || j - m0 > 3 || b.get(j) != Some(&b':') {
        return None;
    }
    let minutes: u32 = s[m0..j].parse().ok()?;
    j += 1;
    let s0 = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j - s0 != 2 {
        return None;
    }
    let seconds: u32 = s[s0..j].parse().ok()?;
    let mut fraction = 0u32;
    if matches!(b.get(j), Some(b'.') | Some(b':')) {
        j += 1;
        let f0 = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let digits = j - f0;
        if digits == 0 || digits > 3 {
            return None;
        }
        let raw: u32 = s[f0..j].parse().ok()?;
        fraction = match digits {
            1 => raw * 100,
            2 => raw * 10,
            _ => raw,
        };
    }
    if b.get(j) != Some(&b']') {
        return None;
    }
    Some((j + 1 - i, (minutes * 60 + seconds) * 1000 + fraction))
}

/// The stamps that open `line` (a line can carry several: `[00:12.00][01:30.00]chorus`) and the text after them.
fn split_line(line: &str) -> (Vec<u32>, &str) {
    let mut stamps = Vec::new();
    let mut i = 0;
    while let Some((len, ms)) = stamp_at(line, i) {
        stamps.push(ms);
        i += len;
    }
    (stamps, &line[i..])
}

/// `<mm:ss.xx>` word stamps of the enhanced format, removed from a line's text.
fn without_word_stamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let close = after.find('>');
        let inner_is_stamp = close.is_some_and(|c| {
            let inner = &after[..c];
            let swapped = format!("[{inner}]");
            stamp_at(&swapped, 0).is_some_and(|(len, _)| len == swapped.len())
        });
        if let (true, Some(c)) = (inner_is_stamp, close) {
            out.push_str(&rest[..open]);
            rest = &after[c + 1..];
        } else {
            out.push_str(&rest[..open + 1]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// A header line (`[ti:Title]`, `[ar:Artist]`, `[offset:+200]`...): letters, a colon, anything, nothing after it.
fn is_header(line: &str) -> bool {
    let t = line.trim();
    if !(t.starts_with('[') && t.ends_with(']')) {
        return false;
    }
    match t[1..].find(':') {
        Some(c) if c > 0 => t[1..1 + c].chars().all(|ch| ch.is_ascii_alphabetic() || ch == '#'),
        _ => false,
    }
}

/// `[offset:+200]`, in milliseconds (0 when there is none).
fn offset_ms(text: &str) -> i64 {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("[offset:") {
            if let Some(v) = rest.strip_suffix(']') {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

/// True when at least two lines carry a stamp: a timed lyric. A header line, a section label (`[Chorus]`)
/// or a lone stamp does not make a text timed.
pub fn is_timed(text: &str) -> bool {
    text.lines().filter(|l| !split_line(l.trim_start_matches('\u{feff}')).0.is_empty()).take(2).count() >= 2
}

/// Every timed line as (milliseconds, words), in time order; the `[offset:]` header is applied.
pub fn timed_lines(text: &str) -> Vec<(u32, String)> {
    let offset = offset_ms(text);
    let mut out = Vec::new();
    for line in text.lines() {
        let (stamps, rest) = split_line(line.trim_start_matches('\u{feff}'));
        if stamps.is_empty() {
            continue;
        }
        let words = without_word_stamps(rest).trim().to_owned();
        for ms in stamps {
            let t = (i64::from(ms) - offset).max(0) as u32;
            out.push((t, words.clone()));
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

/// The words only: stamps (line and word) and header lines removed, blank lines kept as verse breaks.
pub fn strip_stamps(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}');
        let (stamps, rest) = split_line(line);
        if stamps.is_empty() && is_header(line) {
            continue;
        }
        let body = if stamps.is_empty() { line } else { rest };
        lines.push(without_word_stamps(body).trim_end().to_owned());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    lines.join("\n")
}

/// The non-empty lines of a plain lyric (for a `SYLT` that is forced on a text with no timing).
pub fn plain_lines(text: &str) -> Vec<String> {
    strip_stamps(text).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// Timed entries written as `.lrc` text, one `[mm:ss.xx]words` line each.
pub fn to_lrc(entries: &[(u32, String)]) -> String {
    entries
        .iter()
        .map(|(ms, words)| {
            let words = words.trim_start_matches(['\n', '\r']);
            format!("[{:02}:{:02}.{:02}]{}", ms / 60000, (ms / 1000) % 60, (ms % 1000) / 10, words.trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `.lrc` file beside `source`: the same name, extension `lrc` in any case, in the same folder.
pub fn find_lrc(source: &Path) -> Option<PathBuf> {
    let dir = source.parent()?;
    let stem = source.file_stem()?.to_string_lossy().to_lowercase();
    let exact = source.with_extension("lrc");
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| {
        p.is_file()
            && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrc"))
            && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == stem)
    })
}

/// A `.lrc` file's bytes as text: UTF-8 (BOM or not), UTF-16 with a BOM, else Windows-1252 (the old ANSI files).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

/// Windows-1252: Latin-1 except the punctuation block 0x80-0x9F.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™',
        'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..=0x9F).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        char::from(b)
    }
}

/// The text of the `.lrc` file beside `source`, if there is one with something in it.
pub fn read_lrc(source: &Path) -> Option<String> {
    let path = find_lrc(source)?;
    let text = decode(&std::fs::read(path).ok()?);
    let text = text.trim_start_matches('\u{feff}').to_owned();
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LRC: &str = "[ti:Song]\n[ar:Me]\n[offset:+100]\n[00:18.439]Come all of you\n[00:23.060]Good news\n[01:07]Chorus line\n";

    #[test]
    fn a_text_is_timed_when_two_lines_carry_a_stamp() {
        assert!(is_timed(LRC));
        assert!(!is_timed("[ti:Song]\n[Chorus]\nLa la la\n[00:12.00]only one"));
        assert!(!is_timed("Plain words\nand more words"));
        assert!(is_timed("[00:01:50]a\n[00:02:00]b"), "the [mm:ss:xx] spelling counts");
        assert!(is_timed("[00:12.00][01:30.00]chorus\n[00:20.00]x"));
    }

    #[test]
    fn timed_lines_apply_the_offset_and_keep_every_stamp_of_a_line() {
        let l = timed_lines(LRC);
        assert_eq!(l[0], (18339, "Come all of you".to_owned()), "offset +100 ms moves the line 100 ms earlier");
        assert_eq!(l.len(), 3);
        assert_eq!(l[2].0, 66900);
        let multi = timed_lines("[00:12.00][01:30.00]chorus");
        assert_eq!(multi, vec![(12000, "chorus".to_owned()), (90000, "chorus".to_owned())]);
    }

    #[test]
    fn stripping_leaves_the_words_and_the_verse_breaks() {
        let s = strip_stamps("[ti:Song]\n[00:01.00]one <00:01.20>two\n\n[00:05.00]three\n");
        assert_eq!(s, "one two\n\nthree");
        assert_eq!(strip_stamps("Plain\nwords"), "Plain\nwords");
        assert_eq!(plain_lines("a\n\nb"), vec!["a", "b"]);
    }

    #[test]
    fn entries_make_lrc_text_and_back() {
        let e = vec![(18439, "Come".to_owned()), (23060, "Good".to_owned()), (166000, String::new())];
        let t = to_lrc(&e);
        assert_eq!(t, "[00:18.43]Come\n[00:23.06]Good\n[02:46.00]");
        assert_eq!(timed_lines(&t).len(), 3);
    }

    #[test]
    fn a_lrc_file_is_decoded_whatever_it_is_written_in() {
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'a']), "a");
        assert_eq!(decode(&[0xFF, 0xFE, b'a', 0, b'b', 0]), "ab");
        assert_eq!(decode(&[b'c', b'a', b'f', 0xE9]), "café", "Windows-1252 / Latin-1");
        assert_eq!(decode(&[0x93, b'x', 0x94]), "\u{201c}x\u{201d}");
        assert_eq!(decode("é".as_bytes()), "é");
    }

    #[test]
    fn the_lrc_beside_a_source_is_found_in_any_case() {
        let dir = std::env::temp_dir().join(format!("lrc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("01-Song.flac");
        std::fs::write(&src, b"x").unwrap();
        assert!(read_lrc(&src).is_none());
        std::fs::write(dir.join("01-song.LRC"), "[00:01.00]a\n[00:02.00]b").unwrap();
        assert_eq!(read_lrc(&src).as_deref(), Some("[00:01.00]a\n[00:02.00]b"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
