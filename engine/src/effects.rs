//! Effects: an audio chain applied while converting (see PLAN-effects.md).
//!
//! An [`EffectsEntry`] is a named setting like a tag-cleaning entry, chosen apart from the preset so
//! one preset can be used with several chains. It is a LIST of effects: each item is one effect of a
//! known [`Kind`] with its own parameters. Being in the list means it is on; removing it turns it off.
//! A kind appears at most once, and the items always run in the order of their kind - so one
//! measuring pass measures what every effect will see.
//!
//! The work is split in three. [`Analysis`] is what the measuring passes of ffmpeg report about a
//! file (where the sound starts and stops, its loudness, its peak, the ReplayGain figures);
//! [`window`] decides which part of the source is kept; [`plan`] turns an entry and an analysis into
//! exact filter arguments, the length the output will have, and the tags to write. Nothing here runs
//! ffmpeg: the queue does, and hands the text over.

use serde::{Deserialize, Serialize};

/// The id of the entry a fresh install starts with: an ordinary entry with no effect in it.
pub const STANDARD_ID: &str = "default-effects";

/// The kinds of effect, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    TrimSilence,
    CutRange,
    LimitDuration,
    Channels,
    Repair,
    LowCut,
    Denoise,
    DeEsser,
    Equalizer,
    Tone,
    Dynamics,
    Tempo,
    Volume,
    FadeIn,
    FadeOut,
    SilenceEnd,
    ReplayGain,
}

/// The longest fade, in ms: just under ten seconds. A fade is a few seconds at most, and the field that
/// sets it is four digits wide.
pub const MAX_FADE_MS: u32 = 9_999;

/// What an effect acts on, for the "Add an effect" menu and the help. A grouping of the menu only:
/// the run order stays the fixed one of `Kind::ALL`, whatever group a kind is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Which part is kept, and how long it is.
    Time,
    /// How loud it is, and how that changes.
    Level,
    /// What the sound itself is like.
    Sound,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Time, Group::Level, Group::Sound];
}

impl Kind {
    /// The group the menu lists this kind under.
    pub fn group(self) -> Group {
        match self {
            Kind::TrimSilence | Kind::CutRange | Kind::LimitDuration | Kind::SilenceEnd | Kind::Tempo => Group::Time,
            Kind::Volume | Kind::Dynamics | Kind::FadeIn | Kind::FadeOut | Kind::ReplayGain => Group::Level,
            Kind::Channels | Kind::Repair | Kind::LowCut | Kind::Denoise | Kind::DeEsser | Kind::Equalizer | Kind::Tone => Group::Sound,
        }
    }

    pub const ALL: [Kind; 17] = [
        Kind::TrimSilence,
        Kind::CutRange,
        Kind::LimitDuration,
        Kind::Channels,
        Kind::Repair,
        Kind::LowCut,
        Kind::Denoise,
        Kind::DeEsser,
        Kind::Equalizer,
        Kind::Tone,
        Kind::Dynamics,
        Kind::Tempo,
        Kind::Volume,
        Kind::FadeIn,
        Kind::FadeOut,
        Kind::SilenceEnd,
        Kind::ReplayGain,
    ];
}

/// Trim silence at the ends: the start, the end, or both. Never the middle of the song.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrimSilence {
    pub start: bool,
    pub end: bool,
    /// Below this level (dBFS) the sound counts as silence.
    pub threshold_db: f64,
    /// A little of the silence kept at each cut, so a quiet attack or decay is not clipped.
    pub margin_ms: u32,
}

impl Default for TrimSilence {
    fn default() -> Self {
        TrimSilence { start: true, end: true, threshold_db: -60.0, margin_ms: 50 }
    }
}

/// Keep a part of the song: from `from_s` to `to_s` (0 = to the end), counted on what is left after
/// the effects before it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutRange {
    pub from_s: f64,
    pub to_s: f64,
}

impl Default for CutRange {
    fn default() -> Self {
        CutRange { from_s: 0.0, to_s: 0.0 }
    }
}

/// Keep at most this many seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitDuration {
    pub seconds: f64,
}

impl Default for LimitDuration {
    fn default() -> Self {
        LimitDuration { seconds: 30.0 }
    }
}

/// What is done to the channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelsMode {
    /// Left and right change places.
    Swap,
    /// The left channel on both sides (for a mono source, the one channel on both sides).
    Left,
    /// The right channel on both sides.
    Right,
    /// Both channels mixed into one (a mono file).
    Mono,
}

/// Swap, pick or mix the channels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Channels {
    pub mode: ChannelsMode,
}

impl Default for Channels {
    fn default() -> Self {
        Channels { mode: ChannelsMode::Swap }
    }
}

/// Repair clicks and clipped peaks (real restoration: slow, tuned per file by ffmpeg's defaults).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Repair {
    pub declick: bool,
    pub declip: bool,
}

impl Default for Repair {
    fn default() -> Self {
        Repair { declick: true, declip: true }
    }
}

/// Remove what is below a frequency (rumble, DC offset).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LowCut {
    pub hz: u32,
}

impl Default for LowCut {
    fn default() -> Self {
        LowCut { hz: 30 }
    }
}

/// Reduce steady noise (hiss, hum, fans): `afftdn`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Denoise {
    /// How much the noise is lowered, dB.
    pub reduction_db: u32,
    /// The level (dBFS) the noise floor is assumed to sit at.
    pub floor_db: f64,
}

impl Default for Denoise {
    fn default() -> Self {
        Denoise { reduction_db: 12, floor_db: -50.0 }
    }
}

/// Tame harsh "s" sounds in a voice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeEsser {
    /// 0..=100
    pub intensity_pct: u32,
}

impl Default for DeEsser {
    fn default() -> Self {
        DeEsser { intensity_pct: 50 }
    }
}

/// The centres of the graphic equaliser's bands, Hz.
pub const EQ_BANDS_HZ: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// A ten-band graphic equaliser: a gain (dB) per octave band.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Equalizer {
    pub gains_db: [i32; 10],
}

impl Default for Equalizer {
    fn default() -> Self {
        Equalizer { gains_db: [0; 10] }
    }
}

/// Bass and treble shelves, dB.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tone {
    pub bass_db: i32,
    pub treble_db: i32,
}

/// What the dynamics processor does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsMode {
    /// A compressor: everything above the threshold is lowered by the ratio.
    Compressor,
    /// A dynamic normaliser: the level is brought to one level over the course of the track.
    Normaliser,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dynamics {
    pub mode: DynamicsMode,
    /// Compressor: the level (dBFS) above which it works.
    pub threshold_db: f64,
    /// Compressor: the ratio (3 = 3:1).
    pub ratio: u32,
}

impl Default for Dynamics {
    fn default() -> Self {
        Dynamics { mode: DynamicsMode::Compressor, threshold_db: -20.0, ratio: 3 }
    }
}

/// Play faster or slower with the pitch kept: +25 is 25 % faster. Changes the length.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tempo {
    pub percent: i32,
}

impl Tempo {
    /// The speed ratio: 1.0 = unchanged, inside what one `atempo` accepts.
    pub fn ratio(&self) -> f64 {
        (1.0 + f64::from(self.percent) / 100.0).clamp(0.5, 2.0)
    }
}

/// How the volume is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeMode {
    /// Bring the integrated loudness to a target (LUFS).
    Loudness,
    /// Bring the highest sample to a target level (dBFS).
    Peak,
    /// Apply the ReplayGain track (or album) gain to the audio.
    ReplayGain,
    /// A fixed gain in dB.
    Gain,
}

/// The volume of the track: one of four ways, never stacked. A limiter follows every gain that could
/// clip (all but the peak mode, whose result cannot exceed its target).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Volume {
    pub mode: VolumeMode,
    pub target_lufs: f64,
    pub target_peak_db: f64,
    /// The level (dBFS) the limiter keeps the peaks under.
    pub ceiling_db: f64,
    pub gain_db: f64,
    /// ReplayGain mode: use the album's gain (the files of one folder and one album in the run).
    pub album: bool,
}

impl Default for Volume {
    fn default() -> Self {
        Volume { mode: VolumeMode::Loudness, target_lufs: -16.0, target_peak_db: -1.0, ceiling_db: -1.0, gain_db: 0.0, album: false }
    }
}

/// A fade of this length (milliseconds): in at the start or out at the end of what is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fade {
    pub ms: u32,
}

impl Default for Fade {
    fn default() -> Self {
        Fade { ms: 2000 }
    }
}

/// Silence at the end: the song is cut where it stops and exactly `seconds` of silence follow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SilenceEnd {
    /// How long the silence after the song is, whatever the track's own tail was.
    pub seconds: f64,
    /// Below this level (dBFS) the sound counts as silence.
    pub threshold_db: f64,
    /// A little of the original tail kept, so a quiet decay is not clipped (counted in `seconds`).
    pub margin_ms: u32,
}

impl Default for SilenceEnd {
    fn default() -> Self {
        SilenceEnd { seconds: 2.0, threshold_db: -60.0, margin_ms: 50 }
    }
}

/// Write the ReplayGain tags (the audio is not touched); with `album`, the album's too.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplayGainTags {
    pub album: bool,
}

/// One effect of a chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    TrimSilence(TrimSilence),
    CutRange(CutRange),
    LimitDuration(LimitDuration),
    Channels(Channels),
    Repair(Repair),
    LowCut(LowCut),
    Denoise(Denoise),
    DeEsser(DeEsser),
    Equalizer(Equalizer),
    Tone(Tone),
    Dynamics(Dynamics),
    Tempo(Tempo),
    Volume(Volume),
    FadeIn(Fade),
    FadeOut(Fade),
    SilenceEnd(SilenceEnd),
    ReplayGain(ReplayGainTags),
}

impl Effect {
    /// A new item of this kind, with its default values.
    pub fn new(kind: Kind) -> Self {
        match kind {
            Kind::TrimSilence => Effect::TrimSilence(TrimSilence::default()),
            Kind::CutRange => Effect::CutRange(CutRange::default()),
            Kind::LimitDuration => Effect::LimitDuration(LimitDuration::default()),
            Kind::Channels => Effect::Channels(Channels::default()),
            Kind::Repair => Effect::Repair(Repair::default()),
            Kind::LowCut => Effect::LowCut(LowCut::default()),
            Kind::Denoise => Effect::Denoise(Denoise::default()),
            Kind::DeEsser => Effect::DeEsser(DeEsser::default()),
            Kind::Equalizer => Effect::Equalizer(Equalizer::default()),
            Kind::Tone => Effect::Tone(Tone::default()),
            Kind::Dynamics => Effect::Dynamics(Dynamics::default()),
            Kind::Tempo => Effect::Tempo(Tempo::default()),
            Kind::Volume => Effect::Volume(Volume::default()),
            Kind::FadeIn => Effect::FadeIn(Fade::default()),
            Kind::FadeOut => Effect::FadeOut(Fade::default()),
            Kind::SilenceEnd => Effect::SilenceEnd(SilenceEnd::default()),
            Kind::ReplayGain => Effect::ReplayGain(ReplayGainTags::default()),
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Effect::TrimSilence(_) => Kind::TrimSilence,
            Effect::CutRange(_) => Kind::CutRange,
            Effect::LimitDuration(_) => Kind::LimitDuration,
            Effect::Channels(_) => Kind::Channels,
            Effect::Repair(_) => Kind::Repair,
            Effect::LowCut(_) => Kind::LowCut,
            Effect::Denoise(_) => Kind::Denoise,
            Effect::DeEsser(_) => Kind::DeEsser,
            Effect::Equalizer(_) => Kind::Equalizer,
            Effect::Tone(_) => Kind::Tone,
            Effect::Dynamics(_) => Kind::Dynamics,
            Effect::Tempo(_) => Kind::Tempo,
            Effect::Volume(_) => Kind::Volume,
            Effect::FadeIn(_) => Kind::FadeIn,
            Effect::FadeOut(_) => Kind::FadeOut,
            Effect::SilenceEnd(_) => Kind::SilenceEnd,
            Effect::ReplayGain(_) => Kind::ReplayGain,
        }
    }

    fn normalized(self) -> Self {
        match self {
            Effect::TrimSilence(mut t) => {
                t.threshold_db = t.threshold_db.clamp(-120.0, -10.0);
                t.margin_ms = t.margin_ms.min(5000);
                Effect::TrimSilence(t)
            }
            Effect::CutRange(mut c) => {
                c.from_s = c.from_s.clamp(0.0, 86_400.0);
                c.to_s = c.to_s.clamp(0.0, 86_400.0);
                Effect::CutRange(c)
            }
            Effect::LimitDuration(mut l) => {
                l.seconds = l.seconds.clamp(1.0, 86_400.0);
                Effect::LimitDuration(l)
            }
            Effect::Channels(c) => Effect::Channels(c),
            Effect::Repair(r) => Effect::Repair(r),
            Effect::LowCut(mut l) => {
                l.hz = l.hz.clamp(10, 1000);
                Effect::LowCut(l)
            }
            Effect::Denoise(mut d) => {
                d.reduction_db = d.reduction_db.clamp(1, 60);
                d.floor_db = d.floor_db.clamp(-80.0, -20.0);
                Effect::Denoise(d)
            }
            Effect::DeEsser(mut d) => {
                d.intensity_pct = d.intensity_pct.min(100);
                Effect::DeEsser(d)
            }
            Effect::Equalizer(mut e) => {
                for g in e.gains_db.iter_mut() {
                    *g = (*g).clamp(-12, 12);
                }
                Effect::Equalizer(e)
            }
            Effect::Tone(mut t) => {
                t.bass_db = t.bass_db.clamp(-12, 12);
                t.treble_db = t.treble_db.clamp(-12, 12);
                Effect::Tone(t)
            }
            Effect::Dynamics(mut d) => {
                d.threshold_db = d.threshold_db.clamp(-60.0, -1.0);
                d.ratio = d.ratio.clamp(1, 20);
                Effect::Dynamics(d)
            }
            Effect::Tempo(mut t) => {
                t.percent = t.percent.clamp(-50, 100);
                Effect::Tempo(t)
            }
            Effect::Volume(mut v) => {
                v.target_lufs = v.target_lufs.clamp(-50.0, -5.0);
                v.target_peak_db = v.target_peak_db.clamp(-30.0, 0.0);
                v.ceiling_db = v.ceiling_db.clamp(-20.0, 0.0);
                v.gain_db = v.gain_db.clamp(-30.0, 30.0);
                Effect::Volume(v)
            }
            Effect::FadeIn(mut f) => {
                f.ms = f.ms.min(MAX_FADE_MS);
                Effect::FadeIn(f)
            }
            Effect::FadeOut(mut f) => {
                f.ms = f.ms.min(MAX_FADE_MS);
                Effect::FadeOut(f)
            }
            Effect::SilenceEnd(mut s) => {
                s.seconds = s.seconds.clamp(0.0, 60.0);
                s.threshold_db = s.threshold_db.clamp(-120.0, -10.0);
                s.margin_ms = s.margin_ms.min(5000);
                Effect::SilenceEnd(s)
            }
            Effect::ReplayGain(r) => Effect::ReplayGain(r),
        }
    }
}

/// A named chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectsEntry {
    pub id: String,
    pub name: String,
    /// In the order of their kind, each kind at most once (`normalized` makes it so).
    pub items: Vec<Effect>,
}

impl Default for EffectsEntry {
    fn default() -> Self {
        EffectsEntry { id: STANDARD_ID.to_owned(), name: String::new(), items: Vec::new() }
    }
}

impl EffectsEntry {
    /// The entry a fresh install starts with.
    pub fn standard(name: &str) -> Self {
        EffectsEntry { name: name.to_owned(), ..EffectsEntry::default() }
    }

    pub fn new(name: &str) -> Self {
        EffectsEntry { id: crate::preset::new_id(), name: name.to_owned(), items: Vec::new() }
    }

    pub fn has(&self, kind: Kind) -> bool {
        self.items.iter().any(|e| e.kind() == kind)
    }

    pub fn find(&self, kind: Kind) -> Option<&Effect> {
        self.items.iter().find(|e| e.kind() == kind)
    }

    pub fn find_mut(&mut self, kind: Kind) -> Option<&mut Effect> {
        self.items.iter_mut().find(|e| e.kind() == kind)
    }

    /// The kinds that can still be added.
    pub fn addable(&self) -> Vec<Kind> {
        Kind::ALL.into_iter().filter(|k| !self.has(*k)).collect()
    }

    /// Add an effect of this kind with its default values; nothing if it is already there.
    pub fn add(&mut self, kind: Kind) {
        if !self.has(kind) {
            self.items.push(Effect::new(kind));
            self.sort();
        }
    }

    pub fn remove(&mut self, kind: Kind) {
        self.items.retain(|e| e.kind() != kind);
    }

    fn sort(&mut self) {
        self.items.sort_by_key(|e| Kind::ALL.iter().position(|k| *k == e.kind()));
    }

    pub fn trim_silence(&self) -> Option<&TrimSilence> {
        self.items.iter().find_map(|e| if let Effect::TrimSilence(t) = e { Some(t) } else { None })
    }

    pub fn cut_range(&self) -> Option<&CutRange> {
        self.items.iter().find_map(|e| if let Effect::CutRange(c) = e { Some(c) } else { None })
    }

    pub fn limit_duration(&self) -> Option<&LimitDuration> {
        self.items.iter().find_map(|e| if let Effect::LimitDuration(l) = e { Some(l) } else { None })
    }

    pub fn channels(&self) -> Option<&Channels> {
        self.items.iter().find_map(|e| if let Effect::Channels(c) = e { Some(c) } else { None })
    }

    pub fn repair(&self) -> Option<&Repair> {
        self.items.iter().find_map(|e| if let Effect::Repair(r) = e { Some(r) } else { None })
    }

    pub fn low_cut(&self) -> Option<&LowCut> {
        self.items.iter().find_map(|e| if let Effect::LowCut(l) = e { Some(l) } else { None })
    }

    pub fn denoise(&self) -> Option<&Denoise> {
        self.items.iter().find_map(|e| if let Effect::Denoise(d) = e { Some(d) } else { None })
    }

    pub fn de_esser(&self) -> Option<&DeEsser> {
        self.items.iter().find_map(|e| if let Effect::DeEsser(d) = e { Some(d) } else { None })
    }

    pub fn equalizer(&self) -> Option<&Equalizer> {
        self.items.iter().find_map(|e| if let Effect::Equalizer(q) = e { Some(q) } else { None })
    }

    pub fn tone(&self) -> Option<&Tone> {
        self.items.iter().find_map(|e| if let Effect::Tone(t) = e { Some(t) } else { None })
    }

    pub fn dynamics(&self) -> Option<&Dynamics> {
        self.items.iter().find_map(|e| if let Effect::Dynamics(d) = e { Some(d) } else { None })
    }

    pub fn tempo(&self) -> Option<&Tempo> {
        self.items.iter().find_map(|e| if let Effect::Tempo(t) = e { Some(t) } else { None })
    }

    /// The speed ratio of the tempo effect (1.0 when there is none).
    pub fn tempo_ratio(&self) -> f64 {
        self.tempo().map_or(1.0, Tempo::ratio)
    }

    /// What is done to the sound between the cut and the volume, as filters, in the order of the
    /// kinds. These change how loud the track is, so the meters measure through them too.
    pub fn process_filters(&self) -> Vec<String> {
        let mut f: Vec<String> = Vec::new();
        if let Some(c) = self.channels() {
            // Everything starts from a stereo pair, so "left" and "right" exist for a mono source.
            f.push("aformat=channel_layouts=stereo".to_owned());
            f.push(
                match c.mode {
                    ChannelsMode::Swap => "pan=stereo|c0=c1|c1=c0",
                    ChannelsMode::Left => "pan=stereo|c0=c0|c1=c0",
                    ChannelsMode::Right => "pan=stereo|c0=c1|c1=c1",
                    ChannelsMode::Mono => "pan=mono|c0=0.5*c0+0.5*c1",
                }
                .to_owned(),
            );
        }
        if let Some(r) = self.repair() {
            if r.declick {
                f.push("adeclick".to_owned());
            }
            if r.declip {
                f.push("adeclip".to_owned());
            }
        }
        if let Some(l) = self.low_cut() {
            f.push(format!("highpass=f={}", l.hz));
        }
        if let Some(d) = self.denoise() {
            f.push(format!("afftdn=nr={}:nf={}", d.reduction_db, d.floor_db));
        }
        if let Some(d) = self.de_esser() {
            f.push(format!("deesser=i={:.2}:m=0.5:f=0.5", f64::from(d.intensity_pct) / 100.0));
        }
        if let Some(q) = self.equalizer() {
            for (hz, g) in EQ_BANDS_HZ.iter().zip(q.gains_db) {
                if g != 0 {
                    f.push(format!("equalizer=f={hz}:t=o:w=1:g={g}"));
                }
            }
        }
        if let Some(t) = self.tone() {
            if t.bass_db != 0 {
                f.push(format!("bass=g={}", t.bass_db));
            }
            if t.treble_db != 0 {
                f.push(format!("treble=g={}", t.treble_db));
            }
        }
        if let Some(d) = self.dynamics() {
            f.push(match d.mode {
                DynamicsMode::Compressor => format!("acompressor=threshold={}dB:ratio={}:attack=20:release=250", d.threshold_db, d.ratio),
                DynamicsMode::Normaliser => "dynaudnorm".to_owned(),
            });
        }
        if let Some(t) = self.tempo() {
            if t.percent != 0 {
                f.push(format!("atempo={:.4}", t.ratio()));
            }
        }
        f
    }

    pub fn volume(&self) -> Option<&Volume> {
        self.items.iter().find_map(|e| if let Effect::Volume(v) = e { Some(v) } else { None })
    }

    pub fn fade_in(&self) -> Option<&Fade> {
        self.items.iter().find_map(|e| if let Effect::FadeIn(f) = e { Some(f) } else { None })
    }

    pub fn fade_out(&self) -> Option<&Fade> {
        self.items.iter().find_map(|e| if let Effect::FadeOut(f) = e { Some(f) } else { None })
    }

    pub fn silence_end(&self) -> Option<&SilenceEnd> {
        self.items.iter().find_map(|e| if let Effect::SilenceEnd(s) = e { Some(s) } else { None })
    }

    pub fn replaygain(&self) -> Option<&ReplayGainTags> {
        self.items.iter().find_map(|e| if let Effect::ReplayGain(r) = e { Some(r) } else { None })
    }

    /// Does any effect need to know where the sound starts or stops?
    pub fn needs_scan(&self) -> bool {
        self.has(Kind::TrimSilence) || self.has(Kind::SilenceEnd)
    }

    /// Does any effect need the loudness, the peak or the ReplayGain figures of what is kept?
    pub fn needs_meter(&self) -> bool {
        self.replaygain().is_some() || self.volume().is_some_and(|v| v.mode != VolumeMode::Gain)
    }

    /// Does any effect use the album's gain?
    pub fn needs_album(&self) -> bool {
        self.replaygain().is_some_and(|r| r.album)
            || self.volume().is_some_and(|v| v.mode == VolumeMode::ReplayGain && v.album)
    }

    /// Does any effect need the file measured first?
    pub fn needs_analysis(&self) -> bool {
        self.needs_scan() || self.needs_meter()
    }

    /// Would the audio change?
    pub fn changes_audio(&self) -> bool {
        self.items.iter().any(|e| e.kind() != Kind::ReplayGain)
    }

    /// The level (dBFS) below which the scan counts the sound as silence: the trim's own, else the
    /// silence at the end's.
    pub fn scan_threshold_db(&self) -> f64 {
        self.trim_silence()
            .map(|t| t.threshold_db)
            .or_else(|| self.silence_end().map(|s| s.threshold_db))
            .unwrap_or(-60.0)
    }

    /// Put what a hand-edited file can get wrong right: the numbers inside their range, a kind
    /// at most once (the first wins), the items in the order they run.
    pub fn normalized(mut self) -> Self {
        let mut seen: Vec<Kind> = Vec::new();
        self.items = self
            .items
            .into_iter()
            .filter(|e| {
                let fresh = !seen.contains(&e.kind());
                seen.push(e.kind());
                fresh
            })
            .map(Effect::normalized)
            .collect();
        self.sort();
        self
    }
}

/// What the measuring passes found out about one file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub duration_s: f64,
    /// Where the sound starts: the end of a silence that begins at the start of the file. `None`
    /// when the file does not begin in silence.
    pub song_start_s: Option<f64>,
    /// Where the sound stops: the start of the silence that runs to the end. `None` when the file
    /// does not end in silence (the sound runs to the last sample).
    pub song_end_s: Option<f64>,
    /// Integrated loudness, LUFS. `None` for a file with no measurable sound.
    pub loudness_lufs: Option<f64>,
    /// True peak, dBFS.
    pub true_peak_db: Option<f64>,
    /// The highest sample, dBFS (`volumedetect`).
    pub max_volume_db: Option<f64>,
    /// ReplayGain's track gain (dB) and peak (linear) from ffmpeg's `replaygain` filter.
    pub rg_gain_db: Option<f64>,
    pub rg_peak: Option<f64>,
}

/// The album's ReplayGain figures: gain (dB) and the highest peak (linear) of its tracks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Album {
    pub gain_db: f64,
    pub peak: f64,
}

/// The filter of the first pass: where the sound starts and stops.
pub fn scan_filter(threshold_db: f64) -> String {
    format!("silencedetect=n={threshold_db}dB:d=0.1")
}

/// The meters: highest sample, integrated loudness and true peak, ReplayGain.
pub const METER_FILTER: &str = "volumedetect,ebur128=peak=true:framelog=quiet,replaygain";

/// The filter of the second pass: the meters over the part that is kept, after everything that
/// changes how loud it is (the volume is set from what is measured here).
pub fn meter_filter(entry: &EffectsEntry, w: &Window, duration_s: f64) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(w.cut_filter(duration_s));
    parts.extend(entry.process_filters());
    parts.push(METER_FILTER.to_owned());
    parts.join(",")
}

fn number_after(line: &str, key: &str) -> Option<f64> {
    let rest = line.split(key).nth(1)?;
    let token: String = rest
        .trim_start_matches([' ', '=', ':'])
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
        .collect();
    token.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Read what a measuring pass printed (its stderr) into an [`Analysis`]. `duration_s` is the
/// length of what was measured.
pub fn parse_analysis(stderr: &str, duration_s: f64) -> Analysis {
    let mut a = Analysis { duration_s, ..Analysis::default() };
    let mut last_start: Option<f64> = None;
    let mut in_true_peak = false;
    for raw in stderr.lines() {
        let line = raw.trim();
        if let Some(v) = line.split("silence_start:").nth(1) {
            last_start = v.trim().parse::<f64>().ok();
        } else if line.contains("silence_end:") {
            let end = number_after(line, "silence_end:");
            if let (Some(start), Some(end)) = (last_start, end) {
                // Closed at the end of the file: the sound stops where this silence began.
                if duration_s > 0.0 && (duration_s - end).abs() < 0.05 {
                    a.song_end_s = Some(start.max(0.0));
                }
                // Begun at the start of the file: the sound starts where this silence ends.
                if start <= 0.05 && a.song_start_s.is_none() {
                    a.song_start_s = Some(end.max(0.0));
                }
            }
            last_start = None;
        } else if line.starts_with("I:") && line.contains("LUFS") {
            a.loudness_lufs = number_after(line, "I:");
        } else if line.starts_with("True peak:") {
            in_true_peak = true;
        } else if in_true_peak && line.starts_with("Peak:") {
            a.true_peak_db = number_after(line, "Peak:");
            in_true_peak = false;
        } else if line.contains("max_volume:") {
            a.max_volume_db = number_after(line, "max_volume:");
        } else if line.contains("track_gain") {
            a.rg_gain_db = number_after(line, "track_gain =");
        } else if line.contains("track_peak") {
            a.rg_peak = number_after(line, "track_peak =");
        }
    }
    // A silence that was opened and never closed (ffmpeg printed no end) also runs to the end.
    if a.song_end_s.is_none() {
        if let Some(s) = last_start {
            a.song_end_s = Some(s.max(0.0));
        }
    }
    a
}

/// The part of the source that is kept, and what follows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub start: f64,
    pub end: f64,
    /// Silence appended after the kept part.
    pub pad: f64,
}

impl Window {
    pub fn length(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// The filter that keeps only the window, or `None` when the whole file is kept.
    pub fn cut_filter(&self, duration_s: f64) -> Option<String> {
        if self.start > 0.001 || self.end < duration_s - 0.001 {
            Some(format!("atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS", self.start, self.end))
        } else {
            None
        }
    }
}

/// Which part of the source is kept, from the effects that cut: trim silence, cut range, limit
/// duration and the cut of silence at the end - in that order, each working on what is left by the
/// one before it.
pub fn window(entry: &EffectsEntry, a: &Analysis) -> Window {
    let dur = a.duration_s.max(0.0);
    let (mut start, mut end) = (0.0_f64, dur);
    let song_start = a.song_start_s.unwrap_or(0.0).clamp(0.0, dur);
    let song_end = a.song_end_s.unwrap_or(dur).clamp(0.0, dur);

    if let Some(t) = entry.trim_silence() {
        let margin = f64::from(t.margin_ms) / 1000.0;
        if t.start {
            start = (song_start - margin).max(0.0);
        }
        if t.end {
            end = (song_end + margin).min(dur);
        }
    }
    if let Some(c) = entry.cut_range() {
        let from = start + c.from_s;
        let to = if c.to_s > 0.0 { (start + c.to_s).min(end) } else { end };
        // A range that falls outside what is left keeps what is left, rather than nothing.
        if from < to {
            start = from;
            end = to;
        }
    }
    if let Some(l) = entry.limit_duration() {
        end = end.min(start + l.seconds);
    }
    let mut pad = 0.0;
    if let Some(s) = entry.silence_end() {
        let music_end = song_end.clamp(start, end);
        let existing = (end - music_end).max(0.0);
        let want = s.seconds.max(0.0);
        let margin = (f64::from(s.margin_ms) / 1000.0).min(want);
        // Keep a little of the original tail (never more than there is, never more than wanted); the
        // rest of the gap is appended, so the silence after the song is exactly `want`.
        let kept = existing.min(margin);
        end = music_end + kept;
        pad = (want - kept).max(0.0);
    }
    Window { start, end, pad }
}

/// Exact instructions for one file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectsPlan {
    /// Audio filters, in order, joined by a comma into one `-af`. Empty = the audio is not changed.
    pub filters: Vec<String>,
    /// How long the output will be, when the chain changes it.
    pub expected_s: Option<f64>,
    /// Tags to write after the cleaning has been applied: they describe the OUTPUT file.
    pub tags: Vec<(String, String)>,
    /// The gain applied, dB (for reporting and tests).
    pub applied_gain_db: Option<f64>,
}

impl EffectsPlan {
    pub fn af(&self) -> Option<String> {
        (!self.filters.is_empty()).then(|| self.filters.join(","))
    }
}

fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Turn an entry, a file's analysis and (when an effect asked for it) its album into filter
/// arguments, the output length and the tags.
///
/// The order is the one of the kinds: what is kept is cut out, then the volume (a gain, with a
/// limiter where it could clip), the fades, and last the silence - so the gain was computed from the
/// sound that is kept, the fades ride on the gained sound, and the silence is untouched.
pub fn plan(entry: &EffectsEntry, a: &Analysis, album: Option<&Album>) -> EffectsPlan {
    let mut p = EffectsPlan::default();
    let w = window(entry, a);
    // A tempo change moves every later time: the fades and the silence are placed on the new length.
    let ratio = entry.tempo_ratio();
    let length = w.length() / ratio;

    if let Some(cut) = w.cut_filter(a.duration_s) {
        p.filters.extend(cut.split(',').map(str::to_owned));
    }
    p.filters.extend(entry.process_filters());

    // The volume.
    let mut limit_to: Option<f64> = None;
    if let Some(v) = entry.volume() {
        let gain = match v.mode {
            VolumeMode::Loudness => a.loudness_lufs.map(|i| v.target_lufs - i),
            VolumeMode::Peak => a.max_volume_db.map(|m| v.target_peak_db - m),
            VolumeMode::ReplayGain => match (v.album, album) {
                (true, Some(al)) => Some(al.gain_db),
                _ => a.rg_gain_db,
            },
            VolumeMode::Gain => Some(v.gain_db),
        };
        if let Some(g) = gain {
            p.filters.push(format!("volume={g:.3}dB"));
            if v.mode != VolumeMode::Peak {
                let ceiling = db_to_linear(v.ceiling_db);
                p.filters.push(format!("alimiter=limit={ceiling:.6}:level=0"));
                limit_to = Some(ceiling);
            }
            p.applied_gain_db = Some(g);
        }
    }

    // The fades ride on the kept part.
    if let Some(f) = entry.fade_in() {
        let d = (f64::from(f.ms) / 1000.0).min(length);
        if d > 0.0 {
            p.filters.push(format!("afade=t=in:st=0:d={d:.3}"));
        }
    }
    if let Some(f) = entry.fade_out() {
        let d = (f64::from(f.ms) / 1000.0).min(length);
        if d > 0.0 {
            p.filters.push(format!("afade=t=out:st={:.3}:d={d:.3}", (length - d).max(0.0)));
        }
    }

    if entry.silence_end().is_some() {
        p.filters.push(format!("apad=pad_dur={:.6}", w.pad));
    }

    let changed_length =
        w.cut_filter(a.duration_s).is_some() || entry.silence_end().is_some() || (ratio - 1.0).abs() > 1e-9;
    if changed_length {
        p.expected_s = Some(length + w.pad);
    }

    if let Some(r) = entry.replaygain() {
        if let (Some(g), Some(peak)) = (a.rg_gain_db, a.rg_peak) {
            // The tags describe the file that is written. If a gain was applied the remaining
            // adjustment is smaller by that gain, and the peak has moved with it (never past the
            // limiter's ceiling).
            let applied = p.applied_gain_db.unwrap_or(0.0);
            let moved = |peak: f64| {
                let moved = peak * db_to_linear(applied);
                limit_to.map_or(moved, |c| moved.min(c))
            };
            p.tags.push(("REPLAYGAIN_TRACK_GAIN".to_owned(), format!("{:+.2} dB", g - applied)));
            p.tags.push(("REPLAYGAIN_TRACK_PEAK".to_owned(), format!("{:.6}", moved(peak))));
            if let (true, Some(al)) = (r.album, album) {
                p.tags.push(("REPLAYGAIN_ALBUM_GAIN".to_owned(), format!("{:+.2} dB", al.gain_db - applied)));
                p.tags.push(("REPLAYGAIN_ALBUM_PEAK".to_owned(), format!("{:.6}", moved(al.peak))));
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_fade_is_never_longer_than_the_field_can_show() {
        let mut entry = EffectsEntry::new("t");
        entry.items.push(Effect::FadeIn(Fade { ms: 60_000 }));
        entry.items.push(Effect::FadeOut(Fade { ms: 12_345 }));
        let entry = entry.normalized();
        assert_eq!(entry.fade_in().unwrap().ms, MAX_FADE_MS);
        assert_eq!(entry.fade_out().unwrap().ms, MAX_FADE_MS);
    }

    #[test]
    fn every_kind_is_listed_under_one_group_and_every_group_has_kinds() {
        for g in Group::ALL {
            assert!(Kind::ALL.iter().any(|k| k.group() == g), "{g:?} is empty");
        }
        // Three groups cover all seventeen kinds, none twice.
        let total: usize = Group::ALL.iter().map(|g| Kind::ALL.iter().filter(|k| k.group() == *g).count()).sum();
        assert_eq!(total, Kind::ALL.len());
    }

    use super::*;

    const OUT: &str = "\
[Parsed_silencedetect_0 @ 0000] silence_start: 3
[Parsed_replaygain_2 @ 0000] track_gain = +6.28 dB
[Parsed_replaygain_2 @ 0000] track_peak = 0.088367
[Parsed_silencedetect_0 @ 0000] silence_end: 6.5 | silence_duration: 3.5
[Parsed_volumedetect_1 @ 0000] max_volume: -18.1 dB
[Parsed_ebur128_1 @ 0000] Summary:

  Integrated loudness:
    I:         -22.0 LUFS
    Threshold: -32.1 LUFS

  Loudness range:
    LRA:         9.9 LU

  True peak:
    Peak:      -18.1 dBFS
";

    fn entry(kinds: &[Kind]) -> EffectsEntry {
        let mut e = EffectsEntry::new("t");
        for k in kinds {
            e.add(*k);
        }
        e
    }

    fn set(e: &mut EffectsEntry, effect: Effect) {
        e.items.retain(|x| x.kind() != effect.kind());
        e.items.push(effect);
        e.items.sort_by_key(|x| Kind::ALL.iter().position(|k| *k == x.kind()));
    }

    fn song(duration_s: f64, start: Option<f64>, end: Option<f64>) -> Analysis {
        Analysis { duration_s, song_start_s: start, song_end_s: end, ..Analysis::default() }
    }

    #[test]
    fn the_measuring_pass_is_read_into_numbers() {
        let a = parse_analysis(OUT, 6.5);
        assert_eq!(a.song_end_s, Some(3.0));
        assert_eq!(a.song_start_s, None);
        assert_eq!(a.loudness_lufs, Some(-22.0));
        assert_eq!(a.true_peak_db, Some(-18.1));
        assert_eq!(a.max_volume_db, Some(-18.1));
        assert_eq!(a.rg_gain_db, Some(6.28));
        assert_eq!(a.rg_peak, Some(0.088367));
    }

    #[test]
    fn a_silence_at_the_start_gives_the_start_of_the_sound() {
        let out = "silence_start: 0\nsilence_end: 1.5 | silence_duration: 1.5\nsilence_start: 4\nsilence_end: 5 | silence_duration: 1\n";
        let a = parse_analysis(out, 5.0);
        assert_eq!(a.song_start_s, Some(1.5));
        assert_eq!(a.song_end_s, Some(4.0));
    }

    #[test]
    fn a_silence_in_the_middle_is_not_the_end_of_the_song() {
        let out = "silence_start: 2\nsilence_end: 4 | silence_duration: 2\n";
        let a = parse_analysis(out, 10.0);
        assert_eq!(a.song_end_s, None, "silence that ends before the file does is part of the song");
        assert_eq!(a.song_start_s, None);
    }

    #[test]
    fn a_silence_that_is_never_closed_runs_to_the_end() {
        let a = parse_analysis("silence_start: 7.25\n", 9.0);
        assert_eq!(a.song_end_s, Some(7.25));
    }

    #[test]
    fn the_list_keeps_each_kind_once_in_the_order_they_run() {
        let mut e = entry(&[]);
        e.add(Kind::ReplayGain);
        e.add(Kind::FadeOut);
        e.add(Kind::SilenceEnd);
        e.add(Kind::ReplayGain);
        e.add(Kind::Volume);
        e.add(Kind::TrimSilence);
        let kinds: Vec<Kind> = e.items.iter().map(Effect::kind).collect();
        assert_eq!(kinds, [Kind::TrimSilence, Kind::Volume, Kind::FadeOut, Kind::SilenceEnd, Kind::ReplayGain]);
        e.remove(Kind::Volume);
        assert!(e.addable().contains(&Kind::Volume));
        for k in Kind::ALL {
            e.add(k);
        }
        assert!(e.addable().is_empty());
        // A hand-edited file with a duplicate and a wrong order is put right, and numbers are clamped.
        let messy = EffectsEntry {
            items: vec![
                Effect::ReplayGain(ReplayGainTags::default()),
                Effect::SilenceEnd(SilenceEnd { seconds: 9999.0, ..SilenceEnd::default() }),
                Effect::SilenceEnd(SilenceEnd { seconds: 3.0, ..SilenceEnd::default() }),
                Effect::Volume(Volume { gain_db: 400.0, ..Volume::default() }),
            ],
            ..EffectsEntry::new("m")
        }
        .normalized();
        assert_eq!(messy.items.len(), 3);
        assert_eq!(messy.silence_end().unwrap().seconds, 60.0, "the first of a kind wins");
        assert_eq!(messy.volume().unwrap().gain_db, 30.0);
        assert_eq!(messy.items[2].kind(), Kind::ReplayGain);
    }

    #[test]
    fn the_list_survives_json() {
        let mut e = entry(&[Kind::TrimSilence, Kind::Volume, Kind::FadeOut, Kind::ReplayGain]);
        set(&mut e, Effect::FadeOut(Fade { ms: 1234 }));
        let back: EffectsEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn the_silence_at_the_end_is_exactly_the_wanted_length() {
        let e = entry(&[Kind::SilenceEnd]);
        // Song to 3 s, a 3.5 s tail: cut at 3.05, then 1.95 s appended -> 3 s + 2 s.
        let p = plan(&e, &song(6.5, None, Some(3.0)), None);
        assert_eq!(p.filters[0], "atrim=start=0.000000:end=3.050000");
        assert_eq!(p.filters[2], "apad=pad_dur=1.950000");
        assert!((p.expected_s.unwrap() - 5.0).abs() < 1e-6);
        // A tail of 0.3 s is cut to the margin too.
        assert!((plan(&e, &song(3.3, None, Some(3.0)), None).expected_s.unwrap() - 5.0).abs() < 1e-6);
        // A file with no trailing silence: nothing to cut, the whole gap appended.
        let p = plan(&e, &song(3.0, None, None), None);
        assert_eq!(p.af().unwrap(), "apad=pad_dur=2.000000");
        assert!((p.expected_s.unwrap() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_tail_shorter_than_the_margin_is_kept_whole() {
        let mut e = entry(&[]);
        set(&mut e, Effect::SilenceEnd(SilenceEnd { margin_ms: 500, ..SilenceEnd::default() }));
        let p = plan(&e, &song(3.2, None, Some(3.0)), None);
        assert!(p.filters.iter().all(|f| !f.starts_with("atrim")), "{:?}", p.filters);
        assert!((p.expected_s.unwrap() - (3.2 + 1.8)).abs() < 1e-6);
    }

    #[test]
    fn trimming_the_ends_keeps_the_sound_and_a_margin() {
        let e = entry(&[Kind::TrimSilence]);
        let w = window(&e, &song(10.0, Some(1.5), Some(8.0)));
        assert!((w.start - 1.45).abs() < 1e-9 && (w.end - 8.05).abs() < 1e-9, "{w:?}");
        // Only the start, only the end.
        let mut only_start = entry(&[]);
        set(&mut only_start, Effect::TrimSilence(TrimSilence { end: false, ..TrimSilence::default() }));
        let w = window(&only_start, &song(10.0, Some(1.5), Some(8.0)));
        assert!((w.end - 10.0).abs() < 1e-9 && (w.start - 1.45).abs() < 1e-9);
        // A file that neither begins nor ends in silence is kept whole.
        let p = plan(&e, &song(10.0, None, None), None);
        assert!(p.af().is_none() && p.expected_s.is_none());
    }

    #[test]
    fn cut_range_and_limit_work_on_what_is_left_by_the_effects_before_them() {
        let mut e = entry(&[]);
        set(&mut e, Effect::TrimSilence(TrimSilence { margin_ms: 0, ..TrimSilence::default() }));
        set(&mut e, Effect::CutRange(CutRange { from_s: 2.0, to_s: 10.0 }));
        // Sound from 1 to 20: the range counts from 1 -> 3 to 11.
        let w = window(&e, &song(25.0, Some(1.0), Some(20.0)));
        assert_eq!((w.start, w.end), (3.0, 11.0));
        set(&mut e, Effect::LimitDuration(LimitDuration { seconds: 5.0 }));
        let w = window(&e, &song(25.0, Some(1.0), Some(20.0)));
        assert_eq!((w.start, w.end), (3.0, 8.0));
        // A range that falls outside what is left changes nothing.
        let mut out = entry(&[]);
        set(&mut out, Effect::CutRange(CutRange { from_s: 50.0, to_s: 60.0 }));
        let w = window(&out, &song(10.0, None, None));
        assert_eq!((w.start, w.end), (0.0, 10.0));
    }

    #[test]
    fn the_limit_shortens_and_the_silence_is_added_after_it() {
        let mut e = entry(&[Kind::SilenceEnd]);
        set(&mut e, Effect::LimitDuration(LimitDuration { seconds: 30.0 }));
        // A 4-minute song: 30 s kept, then the 2 s of silence.
        let p = plan(&e, &song(247.0, None, Some(240.0)), None);
        assert!((p.expected_s.unwrap() - 32.0).abs() < 1e-6, "{p:?}");
        assert!(p.filters[0].starts_with("atrim=start=0.000000:end=30.000000"));
    }

    #[test]
    fn fades_ride_on_the_kept_part() {
        let mut e = entry(&[Kind::FadeIn, Kind::FadeOut]);
        set(&mut e, Effect::LimitDuration(LimitDuration { seconds: 30.0 }));
        let p = plan(&e, &song(200.0, None, None), None);
        let af = p.af().unwrap();
        assert!(af.contains("afade=t=in:st=0:d=2.000"), "{af}");
        assert!(af.contains("afade=t=out:st=28.000:d=2.000"), "{af}");
        // A fade longer than the song is the whole song.
        let mut short = entry(&[Kind::FadeOut]);
        set(&mut short, Effect::FadeOut(Fade { ms: 9000 }));
        let af = plan(&short, &song(3.0, None, None), None).af().unwrap();
        assert!(af.contains("afade=t=out:st=0.000:d=3.000"), "{af}");
    }

    #[test]
    fn each_volume_mode_computes_its_gain_and_only_the_peak_mode_has_no_limiter() {
        let a = Analysis {
            duration_s: 6.5,
            loudness_lufs: Some(-22.0),
            max_volume_db: Some(-12.0),
            rg_gain_db: Some(3.7),
            ..Analysis::default()
        };
        let mut e = entry(&[]);
        let mut gain_of = |mode, album: bool, al: Option<Album>| {
            set(&mut e, Effect::Volume(Volume { mode, album, gain_db: -4.0, ..Volume::default() }));
            plan(&e, &a, al.as_ref())
        };
        let p = gain_of(VolumeMode::Loudness, false, None);
        assert_eq!(p.applied_gain_db, Some(6.0));
        assert!(p.af().unwrap().contains("volume=6.000dB,alimiter=limit=0.891251:level=0"));
        let p = gain_of(VolumeMode::Peak, false, None);
        assert_eq!(p.applied_gain_db, Some(11.0));
        assert_eq!(p.af().unwrap(), "volume=11.000dB", "a peak target cannot clip, no limiter");
        let p = gain_of(VolumeMode::ReplayGain, false, None);
        assert_eq!(p.applied_gain_db, Some(3.7));
        let p = gain_of(VolumeMode::ReplayGain, true, Some(Album { gain_db: -1.5, peak: 0.9 }));
        assert_eq!(p.applied_gain_db, Some(-1.5), "the album's gain when the album is asked for");
        let p = gain_of(VolumeMode::ReplayGain, true, None);
        assert_eq!(p.applied_gain_db, Some(3.7), "no album found: the track's own");
        let p = gain_of(VolumeMode::Gain, false, None);
        assert_eq!(p.applied_gain_db, Some(-4.0));
    }

    #[test]
    fn replaygain_tags_describe_the_output_file() {
        let mut e = entry(&[Kind::ReplayGain]);
        let a = Analysis { duration_s: 6.5, rg_gain_db: Some(6.28), rg_peak: Some(0.088367), ..Analysis::default() };
        let p = plan(&e, &a, None);
        assert_eq!(p.tags[0], ("REPLAYGAIN_TRACK_GAIN".to_owned(), "+6.28 dB".to_owned()));
        assert_eq!(p.tags[1], ("REPLAYGAIN_TRACK_PEAK".to_owned(), "0.088367".to_owned()));
        assert_eq!(p.tags.len(), 2, "no album tags unless asked for");
        // With a gain applied the tag says what is LEFT to do, and the peak moved with it.
        e.add(Kind::Volume);
        let a = Analysis { loudness_lufs: Some(-22.0), ..a };
        let p = plan(&e, &a, None);
        assert_eq!(p.tags[0].1, "+0.28 dB");
        let peak: f64 = p.tags[1].1.parse().unwrap();
        assert!((peak - 0.088367 * db_to_linear(6.0)).abs() < 1e-5);
        // The album's figures are written beside the track's, with the same gain taken off.
        set(&mut e, Effect::ReplayGain(ReplayGainTags { album: true }));
        let p = plan(&e, &a, Some(&Album { gain_db: 5.0, peak: 0.2 }));
        assert_eq!(p.tags.len(), 4);
        assert_eq!(p.tags[2], ("REPLAYGAIN_ALBUM_GAIN".to_owned(), "-1.00 dB".to_owned()));
    }

    #[test]
    fn the_meter_runs_over_the_kept_part_only() {
        let e = entry(&[Kind::TrimSilence, Kind::Volume]);
        let a = song(10.0, Some(2.0), Some(8.0));
        let w = window(&e, &a);
        let f = meter_filter(&e, &w, 10.0);
        assert!(f.starts_with("atrim=start=1.950000:end=8.050000,asetpts=PTS-STARTPTS,volumedetect"), "{f}");
        let whole = entry(&[Kind::Volume]);
        assert_eq!(meter_filter(&whole, &window(&whole, &a), 10.0), METER_FILTER);
        // The meters see what the volume will be set on: through the processing, not the raw cut.
        let mut p = entry(&[Kind::Volume]);
        set(&mut p, Effect::LowCut(LowCut { hz: 80 }));
        assert_eq!(meter_filter(&p, &window(&p, &a), 10.0), format!("highpass=f=80,{METER_FILTER}"));
    }

    #[test]
    fn the_processing_effects_make_their_filters_in_the_order_of_the_kinds() {
        let mut e = entry(&[]);
        set(&mut e, Effect::Tempo(Tempo { percent: 25 }));
        set(&mut e, Effect::Tone(Tone { bass_db: 3, treble_db: -2 }));
        set(&mut e, Effect::LowCut(LowCut { hz: 40 }));
        set(&mut e, Effect::Channels(Channels { mode: ChannelsMode::Swap }));
        set(&mut e, Effect::Repair(Repair { declick: true, declip: false }));
        set(&mut e, Effect::Denoise(Denoise::default()));
        set(&mut e, Effect::DeEsser(DeEsser { intensity_pct: 30 }));
        let mut gains = [0; 10];
        gains[0] = 3;
        gains[5] = -4;
        set(&mut e, Effect::Equalizer(Equalizer { gains_db: gains }));
        set(&mut e, Effect::Dynamics(Dynamics { mode: DynamicsMode::Compressor, threshold_db: -18.0, ratio: 4 }));
        assert_eq!(
            e.process_filters(),
            [
                "aformat=channel_layouts=stereo",
                "pan=stereo|c0=c1|c1=c0",
                "adeclick",
                "highpass=f=40",
                "afftdn=nr=12:nf=-50",
                "deesser=i=0.30:m=0.5:f=0.5",
                "equalizer=f=31:t=o:w=1:g=3",
                "equalizer=f=1000:t=o:w=1:g=-4",
                "bass=g=3",
                "treble=g=-2",
                "acompressor=threshold=-18dB:ratio=4:attack=20:release=250",
                "atempo=1.2500",
            ]
        );
        // A zero band, zero shelves and no tempo make no filter.
        let flat = entry(&[Kind::Equalizer, Kind::Tone, Kind::Tempo]);
        assert!(flat.process_filters().is_empty());
    }

    #[test]
    fn a_tempo_change_moves_the_length_the_fades_and_the_silence_follow() {
        let mut e = entry(&[Kind::FadeOut, Kind::SilenceEnd]);
        set(&mut e, Effect::Tempo(Tempo { percent: 100 }));
        set(&mut e, Effect::FadeOut(Fade { ms: 1000 }));
        // 20 s of sound twice as fast = 10 s; the fade-out is the last second of THOSE 10, then 2 s.
        let p = plan(&e, &song(20.0, None, None), None);
        let af = p.af().unwrap();
        assert!(af.contains("atempo=2.0000"), "{af}");
        assert!(af.contains("afade=t=out:st=9.000:d=1.000"), "{af}");
        assert!((p.expected_s.unwrap() - 12.0).abs() < 1e-6, "{p:?}");
        assert!((Tempo { percent: 400 }.ratio() - 2.0).abs() < 1e-9, "one atempo goes to 2x");
    }

    #[test]
    fn an_empty_list_changes_nothing_and_needs_no_analysis() {
        let e = EffectsEntry::standard("No effects");
        assert!(!e.needs_analysis() && !e.changes_audio());
        let p = plan(&e, &Analysis { duration_s: 10.0, ..Analysis::default() }, None);
        assert!(p.af().is_none() && p.tags.is_empty() && p.expected_s.is_none());
        // A fixed gain and fades need no measuring at all.
        let g = {
            let mut g = entry(&[Kind::FadeIn]);
            set(&mut g, Effect::Volume(Volume { mode: VolumeMode::Gain, ..Volume::default() }));
            g
        };
        assert!(!g.needs_analysis() && g.changes_audio());
    }
}
