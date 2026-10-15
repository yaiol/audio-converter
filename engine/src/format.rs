//! The output formats the converter can produce, and what each one needs from ffmpeg.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Mp3,
    Aac,
    Flac,
    Wav,
    Ogg,
    Opus,
    Wma,
    Aiff,
    Alac,
    Ac3,
    M4r,
    AacAdts,
    OggFlac,
    WavPack,
    Tta,
    Mp2,
    W64,
    Au,
}

impl Format {
    /// Does ffmpeg write this format's tags as Vorbis comments (field name = text before the `=`)?
    pub fn writes_vorbis_comments(self) -> bool {
        matches!(self, Format::Flac | Format::Ogg | Format::Opus | Format::OggFlac)
    }

    /// Every output format, alphabetical by display name (case-insensitive) - the order the
    /// format picker shows. A test keeps it that way when a format is added.
    pub const ALL: [Format; 18] = [
        Format::Aac,
        Format::AacAdts,
        Format::Ac3,
        Format::Aiff,
        Format::Alac,
        Format::Au,
        Format::Flac,
        Format::OggFlac,
        Format::M4r,
        Format::Mp2,
        Format::Mp3,
        Format::Ogg,
        Format::Opus,
        Format::Tta,
        Format::Wav,
        Format::W64,
        Format::WavPack,
        Format::Wma,
    ];

    /// Stable identifier, used in the UI's `<select>` values and nowhere shown to the user.
    pub fn id(self) -> &'static str {
        match self {
            Format::Mp3 => "mp3",
            Format::Aac => "aac",
            Format::Flac => "flac",
            Format::Wav => "wav",
            Format::Ogg => "ogg",
            Format::Opus => "opus",
            Format::Wma => "wma",
            Format::Aiff => "aiff",
            Format::Alac => "alac",
            Format::Ac3 => "ac3",
            Format::M4r => "m4r",
            Format::AacAdts => "aacadts",
            Format::OggFlac => "oggflac",
            Format::WavPack => "wavpack",
            Format::Tta => "tta",
            Format::Mp2 => "mp2",
            Format::W64 => "w64",
            Format::Au => "au",
        }
    }

    pub fn from_id(id: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The output format that keeps what a source file is, from its audio codec as ffmpeg names it
    /// (the extension tells the containers of one codec apart). `None` = no output format holds it.
    pub fn keeping(codec: &str, ext: &str) -> Option<Format> {
        let ext = ext.to_ascii_lowercase();
        Some(match codec {
            "mp3" => Format::Mp3,
            "mp2" => Format::Mp2,
            "aac" if ext == "aac" => Format::AacAdts,
            "aac" => Format::Aac,
            "alac" => Format::Alac,
            "flac" if ext == "oga" || ext == "ogg" => Format::OggFlac,
            "flac" => Format::Flac,
            "vorbis" => Format::Ogg,
            "opus" => Format::Opus,
            "wmav1" | "wmav2" => Format::Wma,
            "ac3" => Format::Ac3,
            "wavpack" => Format::WavPack,
            "tta" => Format::Tta,
            c if c.starts_with("pcm_") => match ext.as_str() {
                "aif" | "aiff" | "aifc" => Format::Aiff,
                "w64" => Format::W64,
                "au" | "snd" => Format::Au,
                _ => Format::Wav,
            },
            _ => return None,
        })
    }

    /// File extension of the output (no dot).
    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp3 => "mp3",
            Format::Aac => "m4a",
            Format::Flac => "flac",
            Format::Wav => "wav",
            Format::Ogg => "ogg",
            Format::Opus => "opus",
            Format::Wma => "wma",
            Format::Aiff => "aiff",
            // Apple's own extension for ALAC; the same as AAC's, so the two share a name.
            Format::Alac => "m4a",
            Format::Ac3 => "ac3",
            Format::M4r => "m4r",
            Format::AacAdts => "aac",
            Format::OggFlac => "oga",
            Format::WavPack => "wv",
            Format::Tta => "tta",
            Format::Mp2 => "mp2",
            Format::W64 => "w64",
            Format::Au => "au",
        }
    }

    /// Display name. Format names are proper nouns, so they are not translated.
    pub fn label(self) -> &'static str {
        match self {
            Format::Mp3 => "MP3",
            Format::Aac => "AAC (M4A)",
            Format::Flac => "FLAC",
            Format::Wav => "WAV",
            Format::Ogg => "Ogg Vorbis",
            Format::Opus => "Opus",
            Format::Wma => "WMA",
            Format::Aiff => "AIFF",
            Format::Alac => "ALAC (M4A)",
            Format::Ac3 => "AC-3",
            Format::M4r => "M4R (iPhone ringtone)",
            Format::AacAdts => "AAC (raw ADTS)",
            Format::OggFlac => "FLAC in Ogg",
            Format::WavPack => "WavPack",
            Format::Tta => "TTA",
            Format::Mp2 => "MP2",
            Format::W64 => "Wave64",
            Format::Au => "AU",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(
            self,
            Format::Flac | Format::OggFlac | Format::Wav | Format::W64 | Format::Aiff | Format::Alac | Format::WavPack | Format::Tta
        )
    }

    /// The ffmpeg encoder. ⚠ Only encoders that an LGPL build includes: `aac` is ffmpeg's native one;
    /// `libfdk_aac` is non-free and must never appear here.
    pub fn encoder(self) -> &'static str {
        match self {
            Format::Mp3 => "libmp3lame",
            Format::Aac => "aac",
            Format::Flac => "flac",
            Format::Wav => "pcm_s16le",
            Format::Ogg => "libvorbis",
            Format::Opus => "libopus",
            // ffmpeg's native WMA v2 encoder: part of every build, LGPL.
            Format::Wma => "wmav2",
            // Big-endian PCM, as the AIFF container requires.
            Format::Aiff => "pcm_s16be",
            // Apple Lossless, ffmpeg's native encoder.
            Format::Alac => "alac",
            // Dolby Digital, ffmpeg's native encoder. A raw AC-3 stream holds no tags and no picture.
            Format::Ac3 => "ac3",
            // A ringtone is AAC in an MP4 container under another name (see `Preset::codec_args`).
            Format::M4r | Format::AacAdts => "aac",
            Format::OggFlac => "flac",
            Format::WavPack => "wavpack",
            Format::Tta => "tta",
            Format::Mp2 => "mp2",
            Format::W64 => "pcm_s16le",
            Format::Au => "pcm_s16be",
        }
    }

    /// Does the output carry the source's tags at all? A raw stream is just audio.
    pub fn holds_tags(self) -> bool {
        !matches!(self, Format::Ac3 | Format::AacAdts | Format::Mp2 | Format::W64)
    }

    /// Does the OUTPUT carry the source's cover? ffmpeg writes it for four formats
    /// (`carries_cover_art`); the tag layer writes it for Ogg Vorbis, Opus, AIFF, WAV, WMA, WavPack and
    /// TTA. Not FLAC-in-Ogg (nothing in the toolchain writes a picture there), nor the formats with no
    /// tags at all, nor the ringtone.
    pub fn holds_cover(self) -> bool {
        matches!(
            self,
            Format::Mp3
                | Format::Aac
                | Format::Alac
                | Format::Flac
                | Format::Wav
                | Format::Wma
                | Format::Ogg
                | Format::Opus
                | Format::Aiff
                | Format::WavPack
                | Format::Tta
        )
    }

    /// Can ffmpeg itself put an attached picture in the container? Its ogg/opus/wav muxers cannot.
    pub fn carries_cover_art(self) -> bool {
        matches!(self, Format::Mp3 | Format::Aac | Format::Alac | Format::Flac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for f in Format::ALL {
            assert_eq!(Format::from_id(f.id()), Some(f));
        }
        assert_eq!(Format::from_id("nope"), None);
    }

    #[test]
    fn the_list_is_alphabetical_by_label() {
        let labels: Vec<String> = Format::ALL.iter().map(|f| f.label().to_ascii_lowercase()).collect();
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted);
    }

    #[test]
    fn no_non_free_encoder_is_ever_named() {
        for f in Format::ALL {
            assert!(!f.encoder().contains("fdk"), "{:?}", f);
        }
    }
}
