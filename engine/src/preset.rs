//! A preset: one named set of encoding options. The window shows a preset picker; the advanced
//! options live in the preset editor. Everything here is pure - building ffmpeg arguments is a
//! function of the preset, so it is tested without running ffmpeg.

use crate::{format::Format, options::EncoderOptions};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Which ID3v2 version an MP3 / AIFF is tagged with. Both keep a repeated field as separate values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Id3Version {
    /// 2.3, UTF-16: the version every player reads, and what Mp3tag writes by default.
    #[default]
    #[serde(alias = "auto", alias = "v23")]
    V23,
    /// 2.4, UTF-8.
    #[serde(alias = "v24")]
    V24,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RateControl {
    /// Constant bitrate: `bitrate_kbps` is the target.
    Cbr,
    /// Variable bitrate: `vbr_level` is the quality target (higher is better).
    Vbr,
    /// Average bitrate (MP3 only): `bitrate_kbps` is the average the encoder steers to.
    Abr,
}

/// What the single quality control means for a given preset. The window shows ONE number field and its
/// label, range and unit follow this - so it never offers a setting the format lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityKind {
    /// WAV: nothing to choose.
    None,
    Bitrate,
    VbrLevel,
    /// FLAC: the compression effort (the audio is lossless either way).
    Compression,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualityRange {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Unit suffix; empty for a unitless level.
    pub unit: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub format: Format,
    pub rate_control: RateControl,
    pub bitrate_kbps: u32,
    /// "Higher is better", 0..=max for the encoder - mapped to the encoder's own scale in
    /// `codec_args` (LAME's `-q` runs the other way).
    pub vbr_level: u32,
    /// FLAC compression level, 0..=12.
    pub compression_level: u32,
    /// `None` = keep the source's.
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    /// WAV and FLAC only. `None` = the format's default (16).
    pub bit_depth: Option<u8>,
    /// Dither when the output is 16 bit: the lower bits are rounded with a little noise, which hides
    /// the distortion plain rounding makes on a 24-bit source.
    pub dither: bool,
    /// MP3 and AIFF only (they carry an ID3 tag).
    pub id3_version: Id3Version,
    /// The switches an encoder has beyond the quality knob (see `options`).
    pub options: EncoderOptions,
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            id: String::new(),
            name: String::new(),
            format: Format::Mp3,
            rate_control: RateControl::Cbr,
            bitrate_kbps: 192,
            vbr_level: 7,
            compression_level: 5,
            sample_rate: None,
            channels: None,
            bit_depth: None,
            dither: false,
            id3_version: Id3Version::V23,
            options: EncoderOptions::default(),
        }
    }
}

/// The id of the choice "keep the format of each file": not a stored preset but a rule, resolved
/// for every input when its job runs (see `Preset::reproducing`).
pub const ORIGINAL_ID: &str = "original";

/// The longest ringtone iOS accepts.
pub const RINGTONE_MAX_SECONDS: u32 = 40;

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// MPEG audio layer 2 accepts only these rates; the nearest one is used.
fn mp2_bitrate(kbps: u32) -> u32 {
    const RATES: [u32; 14] = [32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384];
    RATES.into_iter().min_by_key(|r| r.abs_diff(kbps)).unwrap_or(192)
}

/// A unique id without a dependency: wall-clock nanoseconds plus a process counter.
pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

impl Preset {
    pub fn new(name: &str, format: Format) -> Preset {
        Preset { id: new_id(), name: name.to_owned(), format, ..Preset::default() }.normalized()
    }

    /// The choice "(Original)": the format is decided per input when the job runs.
    pub fn original() -> Preset {
        Preset { id: ORIGINAL_ID.to_owned(), ..Preset::default() }
    }

    pub fn is_original(&self) -> bool {
        self.id == ORIGINAL_ID
    }

    /// The preset an input resolves "(Original)" to: its own format, at the bitrate it had (a lossless
    /// format has none), everything else as the format's own defaults - the sample rate and channels
    /// stay the source's.
    pub fn reproducing(format: Format, bitrate_kbps: Option<u32>) -> Preset {
        let mut p = Preset { id: ORIGINAL_ID.to_owned(), format, ..Preset::default() };
        if let Some(b) = bitrate_kbps {
            p.bitrate_kbps = b;
        }
        p.normalized()
    }

    /// Variable bitrate exists for these. AAC (ffmpeg's native encoder has no usable VBR) and Opus
    /// (always variable, steered by its bitrate) do not offer the choice.
    pub fn supports_vbr(&self) -> bool {
        matches!(self.format, Format::Mp3 | Format::Ogg)
    }

    /// Formats whose tag is an ID3v2 tag - in a WAV, an `id3 ` chunk beside the RIFF INFO list - so
    /// the version is a choice.
    pub fn supports_id3_version(&self) -> bool {
        matches!(self.format, Format::Mp3 | Format::Aiff | Format::Wav)
    }

    /// Formats where ffmpeg itself writes the ID3 tag (and so takes `-id3v2_version`). For WAV the
    /// tag chunk is written afterwards by the tag library: ffmpeg's WAV muxer has no such option.
    pub fn ffmpeg_writes_id3(&self) -> bool {
        matches!(self.format, Format::Mp3 | Format::Aiff)
    }

    /// MP3 alone has an average-bitrate mode.
    pub fn supports_abr(&self) -> bool {
        self.format == Format::Mp3
    }

    pub fn supports_bit_depth(&self) -> bool {
        matches!(
            self.format,
            Format::Flac | Format::OggFlac | Format::Wav | Format::W64 | Format::Au | Format::Aiff | Format::Alac
        )
    }

    /// Does this output end up 16 bit, so a dither means something?
    pub fn is_16_bit(&self) -> bool {
        self.supports_bit_depth() && self.bit_depth.unwrap_or(16) == 16
    }

    /// The filter that converts to 16 bit with dither, when the preset asks for it.
    pub fn dither_filter(&self) -> Option<&'static str> {
        (self.dither && self.is_16_bit()).then_some("aresample=osf=s16:dither_method=triangular")
    }

    pub fn is_flac(&self) -> bool {
        matches!(self.format, Format::Flac | Format::OggFlac)
    }

    pub fn is_aac(&self) -> bool {
        matches!(self.format, Format::Aac | Format::M4r | Format::AacAdts)
    }

    pub fn quality_kind(&self) -> QualityKind {
        match self.format {
            Format::Wav | Format::W64 | Format::Au | Format::Aiff | Format::Alac | Format::Tta => QualityKind::None,
            Format::Flac | Format::OggFlac | Format::WavPack => QualityKind::Compression,
            Format::Mp3 | Format::Ogg if self.rate_control == RateControl::Vbr => QualityKind::VbrLevel,
            _ => QualityKind::Bitrate,
        }
    }

    pub fn quality_range(&self) -> Option<QualityRange> {
        match self.quality_kind() {
            QualityKind::None => None,
            QualityKind::Compression => {
                let max = if self.format == Format::WavPack { 8.0 } else { 12.0 };
                Some(QualityRange { min: 0.0, max, step: 1.0, unit: "" })
            }
            QualityKind::VbrLevel => {
                let max = if self.format == Format::Ogg { 10.0 } else { 9.0 };
                Some(QualityRange { min: 0.0, max, step: 1.0, unit: "" })
            }
            QualityKind::Bitrate => {
                let (min, max) = match self.format {
                    Format::Opus => (16.0, 256.0),
                    Format::Aac | Format::M4r | Format::AacAdts => (64.0, 320.0),
                    Format::Wma => (32.0, 320.0),
                    Format::Mp2 => (32.0, 384.0),
                    // AC-3 accepts only a fixed set of rates; multiples of 64 from 64 are all in it.
                    Format::Ac3 => (64.0, 640.0),
                    _ => (32.0, 320.0),
                };
                let step = if self.format == Format::Ac3 { 64.0 } else { 8.0 };
                Some(QualityRange { min, max, step, unit: "kbps" })
            }
        }
    }

    pub fn quality_value(&self) -> f64 {
        match self.quality_kind() {
            QualityKind::None => 0.0,
            QualityKind::Compression => f64::from(self.compression_level),
            QualityKind::VbrLevel => f64::from(self.vbr_level),
            QualityKind::Bitrate => f64::from(self.bitrate_kbps),
        }
    }

    pub fn set_quality(&mut self, value: f64) {
        let v = value.round().max(0.0) as u32;
        match self.quality_kind() {
            QualityKind::None => {}
            QualityKind::Compression => self.compression_level = v,
            QualityKind::VbrLevel => self.vbr_level = v,
            QualityKind::Bitrate => self.bitrate_kbps = v,
        }
        *self = self.clone().normalized();
    }

    /// Sample rates the encoder accepts, for the editor's picker.
    pub fn sample_rates(&self) -> &'static [u32] {
        match self.format {
            Format::Opus => &[8000, 12000, 16000, 24000, 48000],
            Format::Mp3 => &[8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000],
            Format::Aac | Format::M4r | Format::AacAdts | Format::Ogg => &[8000, 16000, 22050, 24000, 32000, 44100, 48000],
            Format::Mp2 => &[16000, 22050, 24000, 32000, 44100, 48000],
            Format::Wma => &[8000, 11025, 16000, 22050, 32000, 44100, 48000],
            Format::Ac3 => &[32000, 44100, 48000],
            Format::Flac
            | Format::OggFlac
            | Format::Wav
            | Format::W64
            | Format::Au
            | Format::Aiff
            | Format::Alac
            | Format::WavPack
            | Format::Tta => &[8000, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000],
        }
    }

    pub fn bit_depths(&self) -> &'static [u8] {
        match self.format {
            Format::Flac | Format::OggFlac | Format::Alac => &[16, 24],
            Format::Wav | Format::W64 | Format::Au | Format::Aiff => &[16, 24, 32],
            _ => &[],
        }
    }

    /// Clamp every field into what the chosen format accepts. Called after any edit, so a preset
    /// that was switched from WAV to Opus cannot keep a 96 kHz rate or a 24-bit depth.
    pub fn normalized(mut self) -> Preset {
        if !self.supports_vbr() {
            self.rate_control = RateControl::Cbr;
        }
        if self.rate_control == RateControl::Abr && !self.supports_abr() {
            self.rate_control = RateControl::Cbr;
        }
        // The options of other encoders do not travel with a preset that is not one of them.
        if self.format != Format::Mp3 {
            self.options.reset_mp3();
        }
        if self.format != Format::Ogg {
            self.options.reset_ogg();
        }
        if self.format != Format::Opus {
            self.options.reset_opus();
        }
        if !self.is_flac() {
            self.options.reset_flac();
        }
        if !self.is_aac() {
            self.options.reset_aac();
        }
        self.options.clamp();
        if let Some(r) = self.quality_range() {
            let v = self.quality_value().clamp(r.min, r.max) as u32;
            match self.quality_kind() {
                QualityKind::Compression => self.compression_level = v,
                QualityKind::VbrLevel => self.vbr_level = v,
                QualityKind::Bitrate => self.bitrate_kbps = v,
                QualityKind::None => {}
            }
        }
        if let Some(sr) = self.sample_rate {
            if !self.sample_rates().contains(&sr) {
                self.sample_rate = None;
            }
        }
        if let Some(c) = self.channels {
            if c != 1 && c != 2 {
                self.channels = None;
            }
        }
        match self.bit_depth {
            Some(d) if self.bit_depths().contains(&d) => {}
            _ => self.bit_depth = None,
        }
        if !self.is_16_bit() {
            self.dither = false;
        }
        if !self.supports_id3_version() {
            self.id3_version = Id3Version::V23;
        }
        self
    }

    /// The audio-encoding half of an ffmpeg command line (everything after `-i <input>` that is
    /// about the sound, none of the stream mapping).
    pub fn codec_args(&self) -> Vec<String> {
        let p = self.clone().normalized();
        let o = &p.options;
        let mut a: Vec<String> = Vec::new();
        let mut push = |s: &str| a.push(s.to_owned());
        match p.format {
            Format::Mp3 => {
                push("-c:a");
                push("libmp3lame");
                match p.rate_control {
                    RateControl::Vbr => {
                        push("-q:a");
                        // LAME: 0 is best, 9 worst. The preset stores "higher is better".
                        push(&(9 - p.vbr_level.min(9)).to_string());
                    }
                    RateControl::Abr => {
                        push("-abr");
                        push("1");
                        push("-b:a");
                        push(&format!("{}k", p.bitrate_kbps));
                    }
                    RateControl::Cbr => {
                        push("-b:a");
                        push(&format!("{}k", p.bitrate_kbps));
                    }
                }
                if let Some(q) = o.mp3_algorithm {
                    push("-compression_level");
                    push(&q.to_string());
                }
                if let Some(j) = o.mp3_joint_stereo {
                    push("-joint_stereo");
                    push(if j { "1" } else { "0" });
                }
                if let Some(hz) = o.mp3_lowpass_hz {
                    push("-cutoff");
                    push(&hz.to_string());
                }
                if let Some(hz) = o.mp3_highpass_hz {
                    push("-af");
                    push(&format!("highpass=f={hz}"));
                }
                if !o.mp3_reservoir {
                    push("-reservoir");
                    push("0");
                }
                if o.mp3_copyright {
                    push("-copyright");
                    push("1");
                }
                if !o.mp3_original {
                    push("-original");
                    push("0");
                }
            }
            Format::Aac | Format::AacAdts => {
                push("-c:a");
                push("aac");
                push("-b:a");
                push(&format!("{}k", p.bitrate_kbps));
                if let Some(hz) = o.aac_cutoff_hz {
                    push("-cutoff");
                    push(&hz.to_string());
                }
                if p.format == Format::AacAdts {
                    // A bare AAC stream: no container, so no tags and no cover.
                    push("-f");
                    push("adts");
                }
            }
            Format::Flac | Format::OggFlac => {
                push("-c:a");
                push("flac");
                push("-compression_level");
                push(&p.compression_level.min(12).to_string());
                match p.bit_depth {
                    Some(24) => {
                        push("-sample_fmt");
                        push("s32");
                        push("-bits_per_raw_sample");
                        push("24");
                    }
                    _ => {
                        push("-sample_fmt");
                        push("s16");
                    }
                }
                if let Some(b) = o.flac_block_size {
                    push("-frame_size");
                    push(&b.to_string());
                }
                if let Some(s) = o.flac_stereo {
                    push("-ch_mode");
                    push(s.ffmpeg());
                }
                if let Some(n) = o.flac_max_lpc {
                    push("-max_prediction_order");
                    push(&n.to_string());
                }
                if let Some(n) = o.flac_lpc_precision {
                    push("-lpc_coeff_precision");
                    push(&n.to_string());
                }
                if let Some(n) = o.flac_min_partition {
                    push("-min_partition_order");
                    push(&n.to_string());
                }
                if let Some(n) = o.flac_max_partition {
                    push("-max_partition_order");
                    push(&n.to_string());
                }
                if o.flac_exhaustive {
                    push("-prediction_order_method");
                    push("search");
                }
                if p.format == Format::OggFlac {
                    push("-f");
                    push("ogg");
                }
            }
            Format::Wav | Format::W64 => {
                push("-c:a");
                push(match p.bit_depth {
                    Some(24) => "pcm_s24le",
                    Some(32) => "pcm_s32le",
                    _ => "pcm_s16le",
                });
                if p.format == Format::W64 {
                    push("-f");
                    push("w64");
                }
            }
            Format::Au => {
                push("-c:a");
                push(match p.bit_depth {
                    Some(24) => "pcm_s24be",
                    Some(32) => "pcm_s32be",
                    _ => "pcm_s16be",
                });
                push("-f");
                push("au");
            }
            Format::Ogg => {
                push("-c:a");
                push("libvorbis");
                if p.rate_control == RateControl::Vbr {
                    push("-q:a");
                    push(&p.vbr_level.min(10).to_string());
                } else {
                    push("-b:a");
                    push(&format!("{}k", p.bitrate_kbps));
                    // With a target bitrate, a floor and a ceiling make it a managed stream.
                    if let Some(k) = o.ogg_min_kbps {
                        push("-minrate");
                        push(&format!("{k}k"));
                    }
                    if let Some(k) = o.ogg_max_kbps {
                        push("-maxrate");
                        push(&format!("{k}k"));
                    }
                }
            }
            Format::WavPack => {
                push("-c:a");
                push("wavpack");
                push("-compression_level");
                push(&p.compression_level.min(8).to_string());
                // ffmpeg's WavPack encoder takes its block size from the FIRST frame it is given. A decoder
                // that trims its priming (MP3, AAC, Opus) hands over a first frame of a few dozen samples
                // and the encoder refuses to open ("invalid block size: 47") - so every MP3 failed to
                // convert to WavPack. A fixed size makes it independent of the source.
                push("-frame_size");
                push("4096");
            }
            Format::Tta => {
                push("-c:a");
                push("tta");
            }
            Format::Mp2 => {
                push("-c:a");
                push("mp2");
                push("-b:a");
                push(&format!("{}k", mp2_bitrate(p.bitrate_kbps)));
            }
            Format::Alac => {
                push("-c:a");
                push("alac");
                match p.bit_depth {
                    Some(24) => {
                        push("-sample_fmt");
                        push("s32p");
                        push("-bits_per_raw_sample");
                        push("24");
                    }
                    _ => {
                        push("-sample_fmt");
                        push("s16p");
                    }
                }
            }
            Format::Aiff => {
                push("-c:a");
                push(match p.bit_depth {
                    Some(24) => "pcm_s24be",
                    Some(32) => "pcm_s32be",
                    _ => "pcm_s16be",
                });
                // AIFF has only a few text chunks of its own; an ID3 chunk keeps every tag.
                push("-write_id3v2");
                push("1");
            }
            Format::M4r => {
                push("-c:a");
                push("aac");
                push("-b:a");
                push(&format!("{}k", p.bitrate_kbps));
                if let Some(hz) = o.aac_cutoff_hz {
                    push("-cutoff");
                    push(&hz.to_string());
                }
                // iOS accepts a ringtone up to 40 seconds; longer is silently ignored by the phone.
                push("-t");
                push(&RINGTONE_MAX_SECONDS.to_string());
                // `.m4r` is not a name ffmpeg knows, so the container is named explicitly.
                push("-f");
                push("ipod");
            }
            Format::Ac3 => {
                push("-c:a");
                push("ac3");
                push("-b:a");
                push(&format!("{}k", p.bitrate_kbps));
            }
            Format::Wma => {
                push("-c:a");
                push("wmav2");
                push("-b:a");
                push(&format!("{}k", p.bitrate_kbps));
            }
            Format::Opus => {
                push("-c:a");
                push("libopus");
                push("-b:a");
                push(&format!("{}k", p.bitrate_kbps));
                push("-vbr");
                push(o.opus_vbr.ffmpeg());
                if o.opus_application != crate::options::OpusApplication::Audio {
                    push("-application");
                    push(o.opus_application.ffmpeg());
                }
                if let Some(t) = o.opus_frame_tenths {
                    push("-frame_duration");
                    push(&format!("{}", f64::from(t) / 10.0));
                }
                if let Some(c) = o.opus_complexity {
                    push("-compression_level");
                    push(&c.to_string());
                }
                if let Some(b) = o.opus_bandwidth {
                    push("-cutoff");
                    push(&b.cutoff_hz().to_string());
                }
                if !o.opus_phase_inversion {
                    push("-apply_phase_inv");
                    push("0");
                }
            }
        }
        if let Some(sr) = p.sample_rate {
            push("-ar");
            push(&sr.to_string());
        }
        if let Some(ch) = p.channels {
            push("-ac");
            push(&ch.to_string());
        }
        a
    }

    /// The presets a fresh install starts with.
    pub fn defaults() -> Vec<Preset> {
        // Stable ids: the same on every install, so two defaults() calls are equal and a settings
        // file written by one run still points at the right preset in the next.
        fn make(id: &str, name: &str, format: Format, edit: impl FnOnce(&mut Preset)) -> Preset {
            let mut p = Preset { id: format!("default-{id}"), name: name.to_owned(), format, ..Preset::default() };
            edit(&mut p);
            p.normalized()
        }
        vec![
            make("mp3-192", "MP3 192 kbps", Format::Mp3, |_| {}),
            make("mp3-vbr", "MP3 VBR high", Format::Mp3, |p| {
                p.rate_control = RateControl::Vbr;
                p.vbr_level = 8;
            }),
            make("mp3-128", "MP3 128 kbps", Format::Mp3, |p| p.bitrate_kbps = 128),
            make("aac-192", "AAC 192 kbps", Format::Aac, |_| {}),
            make("opus-128", "Opus 128 kbps", Format::Opus, |p| p.bitrate_kbps = 128),
            make("wma-128", "WMA 128 kbps", Format::Wma, |p| p.bitrate_kbps = 128),
            make("ac3-192", "AC-3 192 kbps", Format::Ac3, |_| {}),
            make("m4r", "iPhone ringtone", Format::M4r, |p| p.bitrate_kbps = 128),
            make("ogg-q6", "Ogg Vorbis q6", Format::Ogg, |p| {
                p.rate_control = RateControl::Vbr;
                p.vbr_level = 6;
            }),
            make("flac", "FLAC", Format::Flac, |_| {}),
            make("wav-16", "WAV 16-bit", Format::Wav, |p| p.bit_depth = Some(16)),
            make("aiff-16", "AIFF 16-bit", Format::Aiff, |p| p.bit_depth = Some(16)),
            make("alac", "ALAC", Format::Alac, |_| {}),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(p: &Preset) -> String {
        p.codec_args().join(" ")
    }

    #[test]
    fn mp3_cbr_and_vbr() {
        let mut p = Preset::new("t", Format::Mp3);
        assert_eq!(args(&p), "-c:a libmp3lame -b:a 192k");
        p.rate_control = RateControl::Vbr;
        p.vbr_level = 9;
        assert_eq!(args(&p), "-c:a libmp3lame -q:a 0");
        p.vbr_level = 0;
        assert_eq!(args(&p), "-c:a libmp3lame -q:a 9");
    }

    #[test]
    fn a_ringtone_is_aac_cut_to_forty_seconds_in_an_ipod_container() {
        let p = Preset::new("t", Format::M4r);
        assert_eq!(args(&p), "-c:a aac -b:a 192k -t 40 -f ipod");
        assert!(!p.supports_vbr());
    }

    #[test]
    fn the_id3_version_only_belongs_to_formats_with_an_id3_tag() {
        let mut p = Preset::new("t", Format::Mp3);
        p.id3_version = Id3Version::V24;
        assert_eq!(p.clone().normalized().id3_version, Id3Version::V24);
        p.format = Format::Flac;
        assert_eq!(p.normalized().id3_version, Id3Version::V23);
    }

    #[test]
    fn ac3_steps_land_on_rates_the_encoder_accepts() {
        const VALID: [u32; 19] =
            [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640];
        let p = Preset::new("t", Format::Ac3);
        let r = p.quality_range().unwrap();
        let mut v = r.min;
        while v <= r.max {
            assert!(VALID.contains(&(v as u32)), "{v}");
            v += r.step;
        }
        assert_eq!(args(&p), "-c:a ac3 -b:a 192k");
        assert_eq!(p.sample_rates(), &[32000, 44100, 48000]);
    }

    #[test]
    fn alac_depths() {
        let mut p = Preset::new("t", Format::Alac);
        assert_eq!(args(&p), "-c:a alac -sample_fmt s16p");
        p.bit_depth = Some(24);
        assert_eq!(args(&p), "-c:a alac -sample_fmt s32p -bits_per_raw_sample 24");
        assert!(p.quality_range().is_none());
    }

    #[test]
    fn aiff_is_big_endian_pcm_with_id3_tags() {
        let mut p = Preset::new("t", Format::Aiff);
        assert_eq!(args(&p), "-c:a pcm_s16be -write_id3v2 1");
        p.bit_depth = Some(24);
        assert_eq!(args(&p), "-c:a pcm_s24be -write_id3v2 1");
        assert!(p.quality_range().is_none());
    }

    #[test]
    fn wma_uses_the_native_encoder_at_a_constant_bitrate() {
        let mut p = Preset::new("t", Format::Wma);
        p.rate_control = RateControl::Vbr;
        let p = p.normalized();
        assert_eq!(p.rate_control, RateControl::Cbr);
        assert_eq!(args(&p), "-c:a wmav2 -b:a 192k");
    }

    #[test]
    fn aac_is_always_constant_bitrate() {
        let mut p = Preset::new("t", Format::Aac);
        p.rate_control = RateControl::Vbr;
        let p = p.normalized();
        assert_eq!(p.rate_control, RateControl::Cbr);
        assert_eq!(args(&p), "-c:a aac -b:a 192k");
    }

    #[test]
    fn flac_and_wav_depths() {
        let mut p = Preset::new("t", Format::Flac);
        assert_eq!(args(&p), "-c:a flac -compression_level 5 -sample_fmt s16");
        p.bit_depth = Some(24);
        assert!(args(&p).ends_with("-sample_fmt s32 -bits_per_raw_sample 24"));
        let mut w = Preset::new("t", Format::Wav);
        w.bit_depth = Some(24);
        assert_eq!(args(&w), "-c:a pcm_s24le");
    }

    #[test]
    fn rate_and_channels_are_appended_only_when_set() {
        let mut p = Preset::new("t", Format::Mp3);
        assert!(!args(&p).contains("-ar"));
        p.sample_rate = Some(44100);
        p.channels = Some(1);
        assert!(args(&p).ends_with("-ar 44100 -ac 1"));
    }

    #[test]
    fn switching_format_drops_what_the_new_format_cannot_hold() {
        let mut p = Preset::new("t", Format::Wav);
        p.sample_rate = Some(96000);
        p.bit_depth = Some(24);
        p.format = Format::Opus;
        let p = p.normalized();
        assert_eq!(p.sample_rate, None);
        assert_eq!(p.bit_depth, None);
    }

    #[test]
    fn the_quality_range_follows_the_format() {
        let mut p = Preset::new("t", Format::Mp3);
        assert_eq!(p.quality_kind(), QualityKind::Bitrate);
        assert_eq!(p.quality_range().unwrap().unit, "kbps");
        p.rate_control = RateControl::Vbr;
        assert_eq!(p.quality_kind(), QualityKind::VbrLevel);
        p.format = Format::Wav;
        assert_eq!(p.quality_kind(), QualityKind::None);
        assert!(p.quality_range().is_none());
    }

    #[test]
    fn set_quality_clamps_to_the_range() {
        let mut p = Preset::new("t", Format::Opus);
        p.set_quality(5000.0);
        assert_eq!(p.bitrate_kbps, 256);
        p.set_quality(1.0);
        assert_eq!(p.bitrate_kbps, 16);
    }

    #[test]
    fn a_preset_that_never_opens_the_advanced_section_is_unchanged() {
        assert_eq!(args(&Preset::new("t", Format::Mp3)), "-c:a libmp3lame -b:a 192k");
        assert_eq!(args(&Preset::new("t", Format::Opus)), "-c:a libopus -b:a 192k -vbr on");
        let f = Preset::new("t", Format::Flac);
        assert_eq!(args(&f), "-c:a flac -compression_level 5 -sample_fmt s16");
    }

    #[test]
    fn mp3_options_become_lame_switches() {
        let mut p = Preset::new("t", Format::Mp3);
        p.rate_control = RateControl::Abr;
        p.options.mp3_algorithm = Some(0);
        p.options.mp3_joint_stereo = Some(false);
        p.options.mp3_lowpass_hz = Some(16000);
        p.options.mp3_highpass_hz = Some(80);
        p.options.mp3_reservoir = false;
        p.options.mp3_copyright = true;
        p.options.mp3_original = false;
        assert_eq!(
            args(&p),
            "-c:a libmp3lame -abr 1 -b:a 192k -compression_level 0 -joint_stereo 0 -cutoff 16000 -af highpass=f=80 \
             -reservoir 0 -copyright 1 -original 0"
        );
    }

    #[test]
    fn opus_options_become_libopus_switches() {
        use crate::options::{OpusApplication, OpusBandwidth, OpusVbr};
        let mut p = Preset::new("t", Format::Opus);
        p.options.opus_application = OpusApplication::Voip;
        p.options.opus_vbr = OpusVbr::Constrained;
        p.options.opus_frame_tenths = Some(25);
        p.options.opus_complexity = Some(6);
        p.options.opus_bandwidth = Some(OpusBandwidth::Wide);
        p.options.opus_phase_inversion = false;
        assert_eq!(
            args(&p),
            "-c:a libopus -b:a 192k -vbr constrained -application voip -frame_duration 2.5 -compression_level 6 \
             -cutoff 8000 -apply_phase_inv 0"
        );
    }

    #[test]
    fn flac_options_become_encoder_switches() {
        use crate::options::FlacStereo;
        let mut p = Preset::new("t", Format::OggFlac);
        p.options.flac_block_size = Some(4096);
        p.options.flac_stereo = Some(FlacStereo::MidSide);
        p.options.flac_max_lpc = Some(12);
        p.options.flac_lpc_precision = Some(14);
        p.options.flac_min_partition = Some(2);
        p.options.flac_max_partition = Some(6);
        p.options.flac_exhaustive = true;
        let a = args(&p);
        assert!(a.contains("-frame_size 4096 -ch_mode 3 -max_prediction_order 12 -lpc_coeff_precision 14"), "{a}");
        assert!(a.contains("-min_partition_order 2 -max_partition_order 6 -prediction_order_method search"), "{a}");
        assert!(a.ends_with("-f ogg"), "{a}");
    }

    #[test]
    fn the_options_of_another_format_do_not_travel_with_a_preset() {
        let mut p = Preset::new("t", Format::Flac);
        p.options.flac_block_size = Some(4096);
        p.options.mp3_lowpass_hz = Some(16000);
        p.format = Format::Opus;
        let p = p.normalized();
        assert_eq!(p.options, EncoderOptions::default());
    }

    #[test]
    fn mp2_only_gets_a_bitrate_it_knows() {
        let mut p = Preset::new("t", Format::Mp2);
        p.bitrate_kbps = 200;
        assert!(args(&p).contains("-b:a 192k") || args(&p).contains("-b:a 224k"), "{}", args(&p));
        assert_eq!(mp2_bitrate(40), 32);
    }

    #[test]
    fn defaults_are_valid_and_uniquely_identified() {
        let d = Preset::defaults();
        assert!(d.len() >= 6);
        let mut ids: Vec<_> = d.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), d.len());
        for p in d {
            assert_eq!(p.clone(), p.normalized());
        }
    }
}
