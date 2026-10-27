//! engine — everything that is not the window.
//!
//! No UI dependency: the app crate depends on this, never the reverse, so the toolkit stays
//! replaceable. Pure functions and data only.

pub mod asf;
pub mod cleanup;
pub mod destination;
pub mod download;
pub mod effects;
pub mod ffmpeg;
pub mod ffmpeg_effects;
pub mod format;
pub mod i18n;
pub mod lyrics;
pub mod mapping;
pub mod options;
pub mod preset;
pub mod queue;
pub mod riff;
pub mod settings;
pub mod tags;
pub mod thumbnail;

pub use cleanup::{CleanupMode, LyricsMode, TagCleanup};
pub use destination::{Destination, Overwrite};
pub use format::Format;
pub use options::{EncoderOptions, FlacStereo, OpusApplication, OpusBandwidth, OpusVbr};
pub use preset::{Id3Version, Preset, QualityKind, RateControl};
pub use queue::{JobSpec, JobStatus, Runner};

use std::cmp::Ordering;

/// Languages whose text runs right to left, by workspace language code.
pub fn is_rtl(lang: &str) -> bool {
    matches!(lang, "ar" | "he" | "fa" | "ur")
}

/// Compare two dotted versions ("1.2.3" vs "1.2.10") number by number. Missing segments are 0 and
/// a non-numeric segment reads as 0, so a malformed beacon can never panic - it just compares low.
/// The twin of `cmpVersion` in the shared update-check.js; the two must agree.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let o = pa
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&pb.get(i).copied().unwrap_or(0));
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}


/// Alphabetical, case-insensitive, with runs of digits compared as numbers - so `Opus 64` comes
/// before `Opus 128`, which a plain text sort gets backwards.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.extend(c.to_lowercase()),
                _ => out.push((digit, c.to_lowercase().collect())),
            }
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = match (x.0, y.0) {
            (true, true) => {
                let (nx, ny) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                nx.len().cmp(&ny.len()).then_with(|| nx.cmp(ny))
            }
            _ => x.1.cmp(&y.1),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

/// Extensions the converter accepts as input (lowercase, no dot).
pub const AUDIO_EXTENSIONS: [&str; 23] = [
    "wav", "flac", "mp3", "m4a", "aac", "ogg", "oga", "opus", "wma", "aiff", "aif", "ape", "wv", "ac3", "m4r",
    // The app's own outputs it could not read back: a TTA, an MP2, a Wave64 and an AU file were refused
    // by the file picker and, added another way, converted without their cover (2026-10-15); and the
    // other audio-only containers ffmpeg decodes.
    "tta", "mp2", "w64", "au", "snd", "aifc", "caf", "mpc",
];

/// True when `file_name` ends in one of [`AUDIO_EXTENSIONS`], case-insensitively.
pub fn is_audio_file(file_name: &str) -> bool {
    file_name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_reads_numbers_as_numbers_and_ignores_case() {
        let mut v = vec!["Opus 128 kbps", "opus 64 kbps", "MP3 VBR high", "MP3 192 kbps", "AAC", "MP3 128 kbps"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["AAC", "MP3 128 kbps", "MP3 192 kbps", "MP3 VBR high", "opus 64 kbps", "Opus 128 kbps"]);
    }

    #[test]
    fn right_to_left_languages() {
        for code in ["ar", "he", "fa", "ur"] {
            assert!(is_rtl(code), "{code}");
        }
    }

    #[test]
    fn left_to_right_languages_and_variants() {
        for code in ["en", "fr", "zh_CN", "pt_BR"] {
            assert!(!is_rtl(code), "{code}");
        }
    }

    #[test]
    fn versions_compare_numerically_not_textually() {
        assert_eq!(compare_versions("1.2.10", "1.2.3"), Ordering::Greater);
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.9.9", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn malformed_versions_never_panic() {
        assert_eq!(compare_versions("", "0.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("abc", "1.0.0"), Ordering::Less);
    }



    #[test]
    fn accepts_known_extensions_in_any_case() {
        assert!(is_audio_file("Track 01.WAV"));
        assert!(is_audio_file("a.b.flac"));
    }

    #[test]
    fn rejects_other_files_and_extensionless_names() {
        assert!(!is_audio_file("cover.jpg"));
        assert!(!is_audio_file("wav"));
    }
}

/// Every audio file under `path` (or `path` itself), in a stable order. A folder is searched
/// recursively, to a depth that cannot loop through a link.
pub fn collect_audio(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    fn walk(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>, depth: u32) {
        if path.is_dir() {
            if depth > 12 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(path) else { return };
            let mut entries: Vec<std::path::PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
            entries.sort();
            for e in entries {
                walk(&e, out, depth + 1);
            }
        } else if path.file_name().is_some_and(|n| is_audio_file(&n.to_string_lossy())) {
            out.push(path.to_path_buf());
        }
    }
    walk(path, out, 0);
}

/// How a run ended: how many were converted, skipped, failed - `3 converted · 1 skipped · 2 failed`.
/// Empty when none of the three happened (nothing ran, or it was only cancelled). The one text the
/// window's status line, its operating-system notice and the command line all print.
pub fn run_summary<'a>(statuses: impl Iterator<Item = &'a JobStatus>, lang: &str) -> String {
    let (mut done, mut skipped, mut failed) = (0, 0, 0);
    for s in statuses {
        match s {
            JobStatus::Done { .. } => done += 1,
            JobStatus::Skipped { .. } => skipped += 1,
            JobStatus::Failed { .. } => failed += 1,
            _ => {}
        }
    }
    let t = |key: &str| i18n::t(lang, key);
    let mut parts = Vec::new();
    if done > 0 {
        parts.push(format!("{done} {}", t("lblMainNotifyDone")));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} {}", t("lblMainNotifySkipped")));
    }
    if failed > 0 {
        parts.push(format!("{failed} {}", t("lblMainNotifyFailed")));
    }
    parts.join(" · ")
}
