//! The encoder-specific options of a preset: the switches an encoder has beyond the one quality setting.
//!
//! Every field means "leave the encoder's own choice" in its default state, so a preset that never
//! opens the advanced section produces exactly the command line it did before these existed. Fields
//! that belong to another format are reset by `Preset::normalized`, so a preset switched from FLAC to
//! Opus does not carry a block size around.

use serde::{Deserialize, Serialize};

/// Opus: what the encoder is tuned for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpusApplication {
    /// General music and mixed content.
    #[default]
    Audio,
    /// Speech.
    Voip,
    /// The shortest algorithmic delay; for live use.
    LowDelay,
}

impl OpusApplication {
    pub fn ffmpeg(self) -> &'static str {
        match self {
            OpusApplication::Audio => "audio",
            OpusApplication::Voip => "voip",
            OpusApplication::LowDelay => "lowdelay",
        }
    }
}

/// Opus bitrate mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpusVbr {
    #[default]
    On,
    Constrained,
    /// Hard constant bitrate.
    Off,
}

impl OpusVbr {
    pub fn ffmpeg(self) -> &'static str {
        match self {
            OpusVbr::On => "on",
            OpusVbr::Constrained => "constrained",
            OpusVbr::Off => "off",
        }
    }
}

/// Opus audio bandwidth ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpusBandwidth {
    Narrow,
    Medium,
    Wide,
    SuperWide,
    Full,
}

impl OpusBandwidth {
    pub const ALL: [OpusBandwidth; 5] =
        [OpusBandwidth::Narrow, OpusBandwidth::Medium, OpusBandwidth::Wide, OpusBandwidth::SuperWide, OpusBandwidth::Full];

    /// The cutoff handed to libopus, which picks the matching band.
    pub fn cutoff_hz(self) -> u32 {
        match self {
            OpusBandwidth::Narrow => 4000,
            OpusBandwidth::Medium => 6000,
            OpusBandwidth::Wide => 8000,
            OpusBandwidth::SuperWide => 12000,
            OpusBandwidth::Full => 20000,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            OpusBandwidth::Narrow => "narrow",
            OpusBandwidth::Medium => "medium",
            OpusBandwidth::Wide => "wide",
            OpusBandwidth::SuperWide => "superwide",
            OpusBandwidth::Full => "full",
        }
    }

    pub fn from_id(id: &str) -> Option<OpusBandwidth> {
        OpusBandwidth::ALL.into_iter().find(|b| b.id() == id)
    }
}

/// FLAC stereo decorrelation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlacStereo {
    Independent,
    LeftSide,
    RightSide,
    MidSide,
}

impl FlacStereo {
    pub const ALL: [FlacStereo; 4] = [FlacStereo::Independent, FlacStereo::LeftSide, FlacStereo::RightSide, FlacStereo::MidSide];

    /// The value of ffmpeg's `-ch_mode`.
    pub fn ffmpeg(self) -> &'static str {
        match self {
            FlacStereo::Independent => "0",
            FlacStereo::LeftSide => "1",
            FlacStereo::RightSide => "2",
            FlacStereo::MidSide => "3",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            FlacStereo::Independent => "independent",
            FlacStereo::LeftSide => "leftside",
            FlacStereo::RightSide => "rightside",
            FlacStereo::MidSide => "midside",
        }
    }

    pub fn from_id(id: &str) -> Option<FlacStereo> {
        FlacStereo::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Opus frame sizes in tenths of a millisecond (2.5 ms is 25) - an integer, so it compares and
/// round-trips through JSON without a float.
pub const OPUS_FRAME_TENTHS: [u32; 9] = [25, 50, 100, 200, 400, 600, 800, 1000, 1200];

/// FLAC block sizes ffmpeg's encoder accepts at the usual sample rates.
pub const FLAC_BLOCK_SIZES: [u32; 7] = [576, 1152, 2304, 4096, 4608, 8192, 16384];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EncoderOptions {
    // ── MP3 (LAME) ──
    /// `None` = the encoder decides (joint stereo for stereo sources).
    pub mp3_joint_stereo: Option<bool>,
    /// Low-pass cutoff in Hz; `None` = LAME's own.
    pub mp3_lowpass_hz: Option<u32>,
    /// High-pass cutoff in Hz; `None` = none.
    pub mp3_highpass_hz: Option<u32>,
    /// LAME's algorithm quality 0 (best, slowest) ..= 9 (fastest); `None` = LAME's default.
    pub mp3_algorithm: Option<u8>,
    pub mp3_reservoir: bool,
    pub mp3_copyright: bool,
    pub mp3_original: bool,

    // ── Ogg Vorbis ──
    pub ogg_min_kbps: Option<u32>,
    pub ogg_max_kbps: Option<u32>,

    // ── Opus ──
    pub opus_application: OpusApplication,
    pub opus_vbr: OpusVbr,
    /// Frame size in tenths of a millisecond, one of `OPUS_FRAME_TENTHS`; `None` = 20 ms.
    pub opus_frame_tenths: Option<u32>,
    pub opus_complexity: Option<u8>,
    pub opus_bandwidth: Option<OpusBandwidth>,
    pub opus_phase_inversion: bool,

    // ── FLAC ──
    pub flac_block_size: Option<u32>,
    pub flac_stereo: Option<FlacStereo>,
    pub flac_max_lpc: Option<u8>,
    pub flac_lpc_precision: Option<u8>,
    pub flac_min_partition: Option<u8>,
    pub flac_max_partition: Option<u8>,
    pub flac_exhaustive: bool,

    // ── AAC ──
    /// Bandwidth limit in Hz; `None` = the encoder's own.
    pub aac_cutoff_hz: Option<u32>,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        EncoderOptions {
            mp3_joint_stereo: None,
            mp3_lowpass_hz: None,
            mp3_highpass_hz: None,
            mp3_algorithm: None,
            mp3_reservoir: true,
            mp3_copyright: false,
            mp3_original: true,
            ogg_min_kbps: None,
            ogg_max_kbps: None,
            opus_application: OpusApplication::Audio,
            opus_vbr: OpusVbr::On,
            opus_frame_tenths: None,
            opus_complexity: None,
            opus_bandwidth: None,
            opus_phase_inversion: true,
            flac_block_size: None,
            flac_stereo: None,
            flac_max_lpc: None,
            flac_lpc_precision: None,
            flac_min_partition: None,
            flac_max_partition: None,
            flac_exhaustive: false,
            aac_cutoff_hz: None,
        }
    }
}

impl EncoderOptions {
    /// Every field back to "the encoder decides" - used for the fields of formats a preset is not.
    pub fn reset_mp3(&mut self) {
        let d = EncoderOptions::default();
        self.mp3_joint_stereo = d.mp3_joint_stereo;
        self.mp3_lowpass_hz = d.mp3_lowpass_hz;
        self.mp3_highpass_hz = d.mp3_highpass_hz;
        self.mp3_algorithm = d.mp3_algorithm;
        self.mp3_reservoir = d.mp3_reservoir;
        self.mp3_copyright = d.mp3_copyright;
        self.mp3_original = d.mp3_original;
    }

    pub fn reset_ogg(&mut self) {
        self.ogg_min_kbps = None;
        self.ogg_max_kbps = None;
    }

    pub fn reset_opus(&mut self) {
        let d = EncoderOptions::default();
        self.opus_application = d.opus_application;
        self.opus_vbr = d.opus_vbr;
        self.opus_frame_tenths = d.opus_frame_tenths;
        self.opus_complexity = d.opus_complexity;
        self.opus_bandwidth = d.opus_bandwidth;
        self.opus_phase_inversion = d.opus_phase_inversion;
    }

    pub fn reset_flac(&mut self) {
        let d = EncoderOptions::default();
        self.flac_block_size = d.flac_block_size;
        self.flac_stereo = d.flac_stereo;
        self.flac_max_lpc = d.flac_max_lpc;
        self.flac_lpc_precision = d.flac_lpc_precision;
        self.flac_min_partition = d.flac_min_partition;
        self.flac_max_partition = d.flac_max_partition;
        self.flac_exhaustive = d.flac_exhaustive;
    }

    pub fn reset_aac(&mut self) {
        self.aac_cutoff_hz = None;
    }

    /// Clamp every number into what the encoder accepts.
    pub fn clamp(&mut self) {
        let hz = |v: &mut Option<u32>, lo: u32, hi: u32| {
            if let Some(x) = v {
                *x = (*x).clamp(lo, hi);
            }
        };
        hz(&mut self.mp3_lowpass_hz, 1000, 22050);
        hz(&mut self.mp3_highpass_hz, 10, 10000);
        hz(&mut self.aac_cutoff_hz, 1000, 24000);
        hz(&mut self.ogg_min_kbps, 32, 500);
        hz(&mut self.ogg_max_kbps, 32, 500);
        if let (Some(lo), Some(hi)) = (self.ogg_min_kbps, self.ogg_max_kbps) {
            if lo > hi {
                self.ogg_min_kbps = Some(hi);
            }
        }
        if let Some(a) = &mut self.mp3_algorithm {
            *a = (*a).min(9);
        }
        if let Some(c) = &mut self.opus_complexity {
            *c = (*c).min(10);
        }
        if let Some(t) = self.opus_frame_tenths {
            if !OPUS_FRAME_TENTHS.contains(&t) {
                self.opus_frame_tenths = None;
            }
        }
        if let Some(b) = self.flac_block_size {
            if !FLAC_BLOCK_SIZES.contains(&b) {
                self.flac_block_size = None;
            }
        }
        if let Some(o) = &mut self.flac_max_lpc {
            *o = (*o).clamp(1, 32);
        }
        if let Some(p) = &mut self.flac_lpc_precision {
            *p = (*p).clamp(8, 15);
        }
        for p in [&mut self.flac_min_partition, &mut self.flac_max_partition].into_iter().flatten() {
            *p = (*p).min(8);
        }
        if let (Some(lo), Some(hi)) = (self.flac_min_partition, self.flac_max_partition) {
            if lo > hi {
                self.flac_min_partition = Some(hi);
            }
        }
    }
}
