//! Effects: an audio chain applied while converting (see PLAN-effects.md).
//!
//! An [`EffectsEntry`] is a named setting like a tag-cleaning entry, chosen apart from the preset so one preset can be used with several
//! chains. It is a LIST of effects in the order the user put them, the same list as the Video Converter's: each item is one effect (a
//! hand-written effect's id, or `try:` and an ffmpeg filter's name) and the values of its controls. Being in the list means it is on;
//! removing it turns it off.
//!
//! The effects themselves are the catalogue installed with ffmpeg (`effects.json` in the pair's folder, see `download`; a form per effect,
//! read by `ffmpeg_effects`, the module shared with the Video Converter). No copy of it is embedded: without it no effect can be built.
//! Most effects are plain filters and are written in the order listed. A few need the file MEASURED first (where the sound starts and stops,
//! its loudness): they are the *measured* effects. The catalogue says which they are: an effect with a `measure` block names an operation
//! ([`Op`], the fixed list of what this app can do) and which of its controls feeds each input of it; no effect is named in this file. The
//! cuts act on the source before the filters whatever their place in the list; the volume acts where it is listed, on what the filters
//! above it left; the silence at the end comes last.
//!
//! The work is split in three. [`Analysis`] is what the measuring passes of ffmpeg report about a file (where the sound starts and stops,
//! its loudness, its peak, the ReplayGain figures); [`window`] decides which part of the source is kept; [`plan`] turns an entry and an
//! analysis into exact filter arguments, the length the output will have, and the tags to write. Nothing here runs ffmpeg: the queue does,
//! and hands the text over.

use crate::ffmpeg_effects::{self as fx, Catalog, Ctx, Values, V};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, RwLock},
};

/// What the app converts, for the catalogue's menu: the effects that work on it.
pub const WORKS: &str = "audio";

/// The id of the entry a fresh install starts with: an ordinary entry with no effect in it.
pub const STANDARD_ID: &str = "default-effects";

/// What this app can do for a measured effect: the values `measure.op` of the catalogue can take. The effects that use them, their
/// names, their controls and the limits of these are all the catalogue's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// Cut the silence at the start and / or the end of the song.
    CutSilence,
    /// Keep the part between two times.
    CutRange,
    /// Keep at most some seconds.
    CutLength,
    /// Set the volume to a loudness.
    GainLoudness,
    /// Set the volume so the highest sample is at a level.
    GainPeak,
    /// Set the volume to the ReplayGain figure.
    GainReplayGain,
    /// A fixed gain.
    GainFixed,
    /// Cut the song where it stops and leave some silence after it.
    PadSilence,
    /// Write the ReplayGain tags.
    TagsReplayGain,
}

impl Op {
    fn named(name: &str) -> Option<Op> {
        Some(match name {
            "cut-silence" => Op::CutSilence,
            "cut-range" => Op::CutRange,
            "cut-length" => Op::CutLength,
            "gain-loudness" => Op::GainLoudness,
            "gain-peak" => Op::GainPeak,
            "gain-replaygain" => Op::GainReplayGain,
            "gain-fixed" => Op::GainFixed,
            "pad-silence" => Op::PadSilence,
            "tags-replaygain" => Op::TagsReplayGain,
            _ => return None,
        })
    }

    fn is_gain(self) -> bool {
        matches!(self, Op::GainLoudness | Op::GainPeak | Op::GainReplayGain | Op::GainFixed)
    }
}

/// The inputs of a measured effect: the value of the control the catalogue names for each, kept inside that control's own limits.
struct Inputs {
    values: Values,
    roles: BTreeMap<String, String>,
    bounds: BTreeMap<String, (Option<f64>, Option<f64>)>,
}

impl Inputs {
    fn number(&self, role: &str) -> f64 {
        let Some(id) = self.roles.get(role) else { return 0.0 };
        let (min, max) = self.bounds.get(id).copied().unwrap_or((None, None));
        self.values.get(id).map_or(0.0, V::to_number).clamp(min.unwrap_or(f64::MIN), max.unwrap_or(f64::MAX))
    }

    fn switch(&self, role: &str) -> bool {
        self.roles.get(role).and_then(|id| self.values.get(id)).is_some_and(V::truthy)
    }
}

/// The catalogue as read, with the words of every language; `CATALOG` is the copy in the language of the window.
static BASE: RwLock<Option<Arc<Catalog>>> = RwLock::new(None);
static LANG: RwLock<String> = RwLock::new(String::new());
static CATALOG: RwLock<Option<Arc<Catalog>>> = RwLock::new(None);

fn publish() {
    let base = BASE.read().unwrap().clone();
    let lang = LANG.read().unwrap().clone();
    *CATALOG.write().unwrap() = base.map(|b| Arc::new(b.localized(&lang)));
}

/// The language the catalogue speaks (`fr`, `pt_BR`...): the window calls it with its own, at launch and on every change.
pub fn set_language(lang: &str) {
    {
        let mut l = LANG.write().unwrap();
        if *l == lang {
            return;
        }
        *l = lang.to_owned();
    }
    publish();
}

/// The catalogue installed with ffmpeg, when it is loaded (`load_installed`).
pub fn catalog() -> Option<Arc<Catalog>> {
    #[cfg(test)]
    test_fixture();
    CATALOG.read().unwrap().clone()
}

/// Read the catalogue from its text (the tests give it the fixture).
pub fn set_catalog_text(text: &str) -> Result<(), String> {
    let catalog = Catalog::from_json(text).map_err(|e| format!("the effects catalogue could not be read: {e}"))?;
    *BASE.write().unwrap() = Some(Arc::new(catalog));
    publish();
    Ok(())
}

/// Load the catalogue of the pair installed in `dir`, or none when there is no pair there (or it cannot be read). Called at launch and
/// after every install.
pub fn load_installed(dir: Option<&Path>) -> Result<(), String> {
    let loaded = dir
        .and_then(crate::download::catalogue_path)
        .map(|path| std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|text| set_catalog_text(&text)));
    match loaded {
        Some(result) => {
            if result.is_err() {
                *BASE.write().unwrap() = None;
                publish();
            }
            result
        }
        None => {
            *BASE.write().unwrap() = None;
            publish();
            Ok(())
        }
    }
}

/// The tests' catalogue: a copy of the one the shared folder makes (never part of a build).
#[cfg(test)]
fn test_fixture() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| set_catalog_text(include_str!("../testdata/effects.json")).expect("the fixture reads"));
}

/// One effect of an entry: which (a hand-written effect's id, or `try:` and an ffmpeg filter's name) and the values of its controls.
/// A control with no value has its default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub key: String,
    pub values: BTreeMap<String, Value>,
}

impl Item {
    pub fn new(key: &str) -> Item {
        Item { key: key.to_owned(), values: BTreeMap::new() }
    }

    pub fn with(key: &str, values: &[(&str, Value)]) -> Item {
        Item { key: key.to_owned(), values: values.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect() }
    }

    /// The effect this item is, when the catalogue has it.
    pub fn effect(&self) -> Option<fx::Effect> {
        catalog()?.form(&self.key)
    }

    /// The values of every control: the defaults, then what the user set.
    pub fn all_values(&self) -> Values {
        let (Some(cat), Some(effect)) = (catalog(), self.effect()) else { return Values::new() };
        let mut v = fx::defaults(&effect, &cat.filters).unwrap_or_default();
        for (id, x) in &self.values {
            v.insert(id.clone(), V::from_json(x));
        }
        v
    }

    /// Keep the values as they are after an edit (the whole map: a value equal to its default is kept too).
    pub fn set_values(&mut self, values: &Values) {
        self.values = values.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
    }

    /// The title the user reads.
    pub fn title(&self) -> String {
        let Some(cat) = catalog() else { return self.key.clone() };
        match self.key.strip_prefix("try:") {
            Some(name) => cat.filters.get(name).and_then(|f| f.plain.clone()).unwrap_or_else(|| name.to_owned()),
            None => cat.effects.get(&self.key).and_then(|e| e.plain.clone()).unwrap_or_else(|| self.key.clone()),
        }
    }

    /// Every control the effect asks for that is mandatory has a value; an item that is not is left out of the command.
    pub fn is_ready(&self) -> bool {
        let (Some(cat), Some(effect)) = (catalog(), self.effect()) else { return false };
        let values = self.all_values();
        fx::resolve_controls(&effect, &cat.filters)
            .map(|cs| cs.iter().filter(|c| c.required).all(|c| values.get(&c.id).is_some_and(|v| !matches!(v, V::Null) && !v.js_string().trim().is_empty())))
            .unwrap_or(false)
    }
}

/// Trim silence at the ends: the start, the end, or both. Never the middle of the song.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimSilence {
    pub start: bool,
    pub end: bool,
    /// Below this level (dBFS) the sound counts as silence.
    pub threshold_db: f64,
    /// A little of the silence kept at each cut, so a quiet attack or decay is not clipped.
    pub margin_ms: u32,
}

/// Keep a part of the song: from `from_s` to `to_s` (0 = to the end), counted on what is left after the effects before it.
#[derive(Clone, Debug, PartialEq)]
pub struct CutRange {
    pub from_s: f64,
    pub to_s: f64,
}

/// Keep at most this many seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitDuration {
    pub seconds: f64,
}

/// How the volume is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The volume of the track: one of four ways. A limiter follows every gain that could clip (all but the peak mode, whose result cannot
/// exceed its target).
#[derive(Clone, Debug, PartialEq)]
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

/// Silence at the end: the song is cut where it stops and exactly `seconds` of silence follow.
#[derive(Clone, Debug, PartialEq)]
pub struct SilenceEnd {
    /// How long the silence after the song is, whatever the track's own tail was.
    pub seconds: f64,
    /// Below this level (dBFS) the sound counts as silence.
    pub threshold_db: f64,
    /// A little of the original tail kept, so a quiet decay is not clipped (counted in `seconds`).
    pub margin_ms: u32,
}

/// Write the ReplayGain tags (the audio is not touched); with `album`, the album's too.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayGainTags {
    pub album: bool,
}

impl Item {
    /// The operation the catalogue gives this effect, when it is a measured effect this app can do.
    fn op(&self) -> Option<Op> {
        Op::named(&catalog()?.effects.get(&self.key)?.measure.as_ref()?.op)
    }

    /// Does the catalogue say the app does this effect itself, from what it measures? (An operation this app lacks still counts: the job
    /// says so, `try_chain`.)
    fn is_measured(&self) -> bool {
        catalog().is_some_and(|c| c.effects.get(&self.key).is_some_and(|e| e.measure.is_some()))
    }

    fn is_volume(&self) -> bool {
        self.op().is_some_and(Op::is_gain)
    }

    /// The operation and its inputs, for the measured effect this item is.
    fn measured(&self) -> Option<(Op, Inputs)> {
        let cat = catalog()?;
        let effect = cat.effects.get(&self.key)?;
        let measure = effect.measure.as_ref()?;
        let bounds = fx::resolve_controls(effect, &cat.filters).ok()?.into_iter().map(|c| (c.id, (c.min, c.max))).collect();
        Some((Op::named(&measure.op)?, Inputs { values: self.all_values(), roles: measure.controls.clone(), bounds }))
    }

    /// How many times faster the effect plays the clip, when it is one that changes the speed (the catalogue names the option: `speed`
    /// of the filter).
    fn speed(&self) -> Option<f64> {
        let cat = catalog()?;
        let option = cat.filters.get(self.key.strip_prefix("try:")?)?.speed.as_ref()?;
        self.all_values().get(option).map(V::to_number)
    }

    fn as_trim_silence(&self) -> Option<TrimSilence> {
        let (op, r) = self.measured()?;
        (op == Op::CutSilence).then(|| TrimSilence { start: r.switch("start"), end: r.switch("end"), threshold_db: r.number("threshold"), margin_ms: r.number("margin") as u32 })
    }

    fn as_cut_range(&self) -> Option<CutRange> {
        let (op, r) = self.measured()?;
        (op == Op::CutRange).then(|| CutRange { from_s: r.number("from"), to_s: r.number("to") })
    }

    fn as_limit_duration(&self) -> Option<LimitDuration> {
        let (op, r) = self.measured()?;
        (op == Op::CutLength).then(|| LimitDuration { seconds: r.number("seconds") })
    }

    fn as_volume(&self) -> Option<Volume> {
        let (op, r) = self.measured()?;
        let mode = match op {
            Op::GainLoudness => VolumeMode::Loudness,
            Op::GainPeak => VolumeMode::Peak,
            Op::GainReplayGain => VolumeMode::ReplayGain,
            Op::GainFixed => VolumeMode::Gain,
            _ => return None,
        };
        let mut v = Volume { mode, ..Volume::default() };
        match mode {
            VolumeMode::Loudness => v.target_lufs = r.number("target"),
            VolumeMode::Peak => v.target_peak_db = r.number("target"),
            VolumeMode::ReplayGain => v.album = r.switch("album"),
            VolumeMode::Gain => v.gain_db = r.number("gain"),
        }
        if mode != VolumeMode::Peak {
            v.ceiling_db = r.number("ceiling");
        }
        Some(v)
    }

    fn as_silence_end(&self) -> Option<SilenceEnd> {
        let (op, r) = self.measured()?;
        (op == Op::PadSilence).then(|| SilenceEnd { seconds: r.number("seconds"), threshold_db: r.number("threshold"), margin_ms: r.number("margin") as u32 })
    }

    fn as_replaygain(&self) -> Option<ReplayGainTags> {
        let (op, r) = self.measured()?;
        (op == Op::TagsReplayGain).then(|| ReplayGainTags { album: r.switch("album") })
    }
}

/// A named chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectsEntry {
    pub id: String,
    pub name: String,
    /// In the order the user put them.
    pub items: Vec<Item>,
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

    /// The same entry without an item that names no effect (what a file of an older build holds in place of one). An effect the catalogue lacks stays: another
    /// ffmpeg may bring it back, and the job says it is not available.
    pub fn normalized(mut self) -> Self {
        self.items.retain(|i| !i.key.is_empty());
        self
    }

    fn first<T>(&self, read: impl Fn(&Item) -> Option<T>) -> Option<T> {
        self.items.iter().find_map(read)
    }

    pub fn trim_silence(&self) -> Option<TrimSilence> {
        self.first(Item::as_trim_silence)
    }

    pub fn cut_range(&self) -> Option<CutRange> {
        self.first(Item::as_cut_range)
    }

    pub fn limit_duration(&self) -> Option<LimitDuration> {
        self.first(Item::as_limit_duration)
    }

    pub fn volume(&self) -> Option<Volume> {
        self.first(Item::as_volume)
    }

    pub fn silence_end(&self) -> Option<SilenceEnd> {
        self.first(Item::as_silence_end)
    }

    pub fn replaygain(&self) -> Option<ReplayGainTags> {
        self.first(Item::as_replaygain)
    }

    /// The speed ratio of all the tempo effects listed (1.0 when there is none).
    pub fn tempo_ratio(&self) -> f64 {
        self.ratio_before(self.items.len())
    }

    /// The speed ratio of the tempo effects listed before the item at `end`.
    fn ratio_before(&self, end: usize) -> f64 {
        self.items.iter().take(end).filter_map(Item::speed).filter(|r| *r > 0.0).product()
    }

    /// What is wrong with the order of the effects: one that runs before another which changes what it works on.
    pub fn order_problems(&self) -> Vec<String> {
        let Some(cat) = catalog() else { return Vec::new() };
        cat.order(&self.items.iter().filter(|i| i.is_ready()).map(|i| i.key.clone()).collect::<Vec<_>>()).problems
    }

    /// Does any effect need to know where the sound starts or stops?
    pub fn needs_scan(&self) -> bool {
        self.trim_silence().is_some() || self.silence_end().is_some()
    }

    /// Does any effect need the loudness, the peak or the ReplayGain figures of what is kept?
    pub fn needs_meter(&self) -> bool {
        self.replaygain().is_some() || self.volume().is_some_and(|v| v.mode != VolumeMode::Gain)
    }

    /// Does any effect use the album's gain?
    pub fn needs_album(&self) -> bool {
        self.replaygain().is_some_and(|r| r.album) || self.volume().is_some_and(|v| v.mode == VolumeMode::ReplayGain && v.album)
    }

    /// Does any effect need the file measured first?
    pub fn needs_analysis(&self) -> bool {
        self.needs_scan() || self.needs_meter()
    }

    /// Would the audio change?
    pub fn changes_audio(&self) -> bool {
        self.items.iter().any(|i| i.op() != Some(Op::TagsReplayGain))
    }

    /// The level (dBFS) below which the scan counts the sound as silence: the trim's own, else the silence at the end's.
    pub fn scan_threshold_db(&self) -> f64 {
        self.trim_silence().map(|t| t.threshold_db).or_else(|| self.silence_end().map(|s| s.threshold_db)).unwrap_or(-60.0)
    }

    /// The filters of the listed effects that are not measured, in the order listed, for the items in `range`. `length_s` is the length of what is kept before any tempo change: the fades are placed on it, taking the tempo
    /// effects listed above them into account. An effect that cannot be written is left out here (the job says why first, `try_chain`).
    pub fn chain(&self, range: std::ops::Range<usize>, length_s: f64) -> Vec<String> {
        self.try_chain(range, length_s).unwrap_or_default()
    }

    /// `chain`, or why an effect cannot be written.
    pub fn try_chain(&self, range: std::ops::Range<usize>, length_s: f64) -> Result<Vec<String>, String> {
        let Some(cat) = catalog() else {
            return if self.items.iter().any(|i| !i.is_measured()) { Err("the effects are not installed yet: they come with ffmpeg".to_owned()) } else { Ok(Vec::new()) };
        };
        let mut out = Vec::new();
        for (at, item) in self.items.iter().enumerate().take(range.end).skip(range.start) {
            if item.is_measured() {
                if item.op().is_none() {
                    return Err(format!("{}: this version of the app cannot do it", item.title()));
                }
                continue;
            }
            let effect = item.effect().ok_or_else(|| format!("{}: not available with this ffmpeg", item.title()))?;
            if !item.is_ready() {
                continue;
            }
            let ctx: Ctx = [("duration".to_owned(), format!("{}", length_s / self.ratio_before(at).max(1e-9)))].into_iter().collect();
            let built = fx::build(&effect, &cat.filters, &item.all_values(), &ctx).map_err(|e| format!("{}: {e}", item.title()))?;
            out.extend(built.audio.filter(|a| !a.is_empty()));
        }
        Ok(out)
    }

    /// Check every effect can be written (the job fails with the reason before anything runs).
    pub fn check(&self) -> Result<(), String> {
        self.try_chain(0..self.items.len(), 1.0).map(|_| ())
    }

    /// The place of the first volume effect in the list (the end of the list when there is none).
    fn volume_at(&self) -> usize {
        self.items.iter().position(Item::is_volume).unwrap_or(self.items.len())
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
    parts.extend(entry.chain(0..entry.volume_at(), w.length()));
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
/// The order: what is kept is cut out, then the effects listed above the volume, the volume (a gain, with a limiter where it could
/// clip), the effects listed below it, and last the silence - so the gain was computed from the sound the effects above it leave, and
/// the silence is untouched.
pub fn plan(entry: &EffectsEntry, a: &Analysis, album: Option<&Album>) -> EffectsPlan {
    let mut p = EffectsPlan::default();
    let w = window(entry, a);
    // A tempo change moves every later time: the fades and the silence are placed on the new length.
    let ratio = entry.tempo_ratio();
    let length = w.length() / ratio;

    if let Some(cut) = w.cut_filter(a.duration_s) {
        p.filters.extend(cut.split(',').map(str::to_owned));
    }
    // What is listed above the volume, then the volume, then the rest.
    let split = entry.volume_at();
    p.filters.extend(entry.chain(0..split, w.length()));

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

    p.filters.extend(entry.chain(split..entry.items.len(), w.length()));

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
    use super::*;
    use serde_json::json;

    #[test]
    fn a_language_without_translations_stays_english_and_one_with_them_changes_the_words() {
        let en = Catalog::from_json(include_str!("../testdata/effects.json")).unwrap();
        let same = en.localized("xx");
        assert_eq!(same.categories[0].title, en.categories[0].title);
        let Some((lang, t)) = en.i18n.iter().find(|(_, t)| !t.categories.is_empty()) else { return };
        let (id, text) = t.categories.iter().next().unwrap();
        let Some(title) = &text.title else { return };
        let fr = en.localized(lang);
        let cat = fr.categories.iter().find(|c| &c.id == id).unwrap();
        assert_eq!(&cat.title, title);
        assert!(fr.i18n.is_empty());
    }

    const TRIM_SILENCE: &str = "trim-silence";
    const KEEP_RANGE: &str = "keep-range";
    const LIMIT_DURATION: &str = "limit-duration";
    const VOLUME_LOUDNESS: &str = "volume-loudness";
    const VOLUME_PEAK: &str = "volume-peak";
    const VOLUME_REPLAYGAIN: &str = "volume-replaygain";
    const VOLUME_GAIN: &str = "volume-gain";
    const SILENCE_END: &str = "silence-end";
    const REPLAYGAIN_TAGS: &str = "replaygain-tags";
    const TEMPO: &str = "try:atempo";

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

    fn entry(items: Vec<Item>) -> EffectsEntry {
        EffectsEntry { items, ..EffectsEntry::new("t") }
    }

    fn song(duration_s: f64, start: Option<f64>, end: Option<f64>) -> Analysis {
        Analysis { duration_s, song_start_s: start, song_end_s: end, ..Analysis::default() }
    }

    fn silence_end(extra: &[(&str, Value)]) -> Item {
        Item::with(SILENCE_END, extra)
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
    fn the_list_keeps_the_order_the_user_gave_and_survives_json() {
        let e = entry(vec![Item::new(REPLAYGAIN_TAGS), Item::with("fade-out", &[("length", json!(1234))]), Item::new(TRIM_SILENCE), Item::new("")]);
        let e = e.normalized();
        let keys: Vec<&str> = e.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, [REPLAYGAIN_TAGS, "fade-out", TRIM_SILENCE], "an item naming no effect is dropped, the rest is not moved");
        let back: EffectsEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn numbers_are_read_inside_their_range() {
        let e = entry(vec![silence_end(&[("seconds", json!(9999))]), Item::with(VOLUME_GAIN, &[("gain", json!(400))])]);
        assert_eq!(e.silence_end().unwrap().seconds, 60.0);
        assert_eq!(e.volume().unwrap().gain_db, 30.0);
        let twice = entry(vec![silence_end(&[("seconds", json!(3))]), silence_end(&[("seconds", json!(9))])]);
        assert_eq!(twice.silence_end().unwrap().seconds, 3.0, "the first of a kind wins");
    }

    #[test]
    fn a_file_saved_with_the_old_kinds_loses_them_and_keeps_the_rest() {
        let old = r#"{ "id": "x", "name": "Old", "items": [
            { "trim_silence": { "start": true } },
            { "key": "fade-out", "values": { "length": 1500 } } ] }"#;
        let e: EffectsEntry = serde_json::from_str(old).unwrap();
        let e = e.normalized();
        let keys: Vec<&str> = e.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["fade-out"], "an effect of the old shape names no effect now");
        assert_eq!((e.id.as_str(), e.name.as_str()), ("x", "Old"));
    }

    #[test]
    fn every_measured_effect_of_the_catalogue_names_an_operation_the_app_has() {
        let cat = catalog().unwrap();
        let measured: Vec<_> = cat.effects.iter().filter(|(_, e)| e.measure.is_some()).collect();
        assert!(!measured.is_empty());
        for (id, e) in measured {
            let m = e.measure.as_ref().unwrap();
            assert!(Op::named(&m.op).is_some(), "{id}: the app has no operation {}", m.op);
            for control in m.controls.values() {
                assert!(e.controls.iter().any(|c| &c.id == control), "{id}: no control {control}");
            }
        }
    }

    #[test]
    fn the_speed_comes_from_the_filter_the_catalogue_names() {
        let e = entry(vec![Item::with("try:atempo", &[("tempo", json!(2))]), Item::new("fade-out")]);
        assert!((e.tempo_ratio() - 2.0).abs() < 1e-9);
        assert!((entry(vec![Item::new("fade-out")]).tempo_ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_silence_at_the_end_is_exactly_the_wanted_length() {
        let e = entry(vec![silence_end(&[])]);
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
        let e = entry(vec![silence_end(&[("margin", json!(500))])]);
        let p = plan(&e, &song(3.2, None, Some(3.0)), None);
        assert!(p.filters.iter().all(|f| !f.starts_with("atrim")), "{:?}", p.filters);
        assert!((p.expected_s.unwrap() - (3.2 + 1.8)).abs() < 1e-6);
    }

    #[test]
    fn trimming_the_ends_keeps_the_sound_and_a_margin() {
        let e = entry(vec![Item::new(TRIM_SILENCE)]);
        let w = window(&e, &song(10.0, Some(1.5), Some(8.0)));
        assert!((w.start - 1.45).abs() < 1e-9 && (w.end - 8.05).abs() < 1e-9, "{w:?}");
        // Only the start, only the end.
        let only_start = entry(vec![Item::with(TRIM_SILENCE, &[("end", json!(false))])]);
        let w = window(&only_start, &song(10.0, Some(1.5), Some(8.0)));
        assert!((w.end - 10.0).abs() < 1e-9 && (w.start - 1.45).abs() < 1e-9);
        // A file that neither begins nor ends in silence is kept whole.
        let p = plan(&e, &song(10.0, None, None), None);
        assert!(p.af().is_none() && p.expected_s.is_none());
    }

    #[test]
    fn keep_a_range_and_the_limit_work_on_what_is_left_by_the_effects_before_them() {
        let trim = Item::with(TRIM_SILENCE, &[("margin", json!(0))]);
        let range = Item::with(KEEP_RANGE, &[("from", json!(2)), ("to", json!(10))]);
        let mut e = entry(vec![trim, range]);
        // Sound from 1 to 20: the range counts from 1 -> 3 to 11.
        let w = window(&e, &song(25.0, Some(1.0), Some(20.0)));
        assert_eq!((w.start, w.end), (3.0, 11.0));
        e.items.push(Item::with(LIMIT_DURATION, &[("seconds", json!(5))]));
        let w = window(&e, &song(25.0, Some(1.0), Some(20.0)));
        assert_eq!((w.start, w.end), (3.0, 8.0));
        // A range that falls outside what is left changes nothing.
        let out = entry(vec![Item::with(KEEP_RANGE, &[("from", json!(50)), ("to", json!(60))])]);
        let w = window(&out, &song(10.0, None, None));
        assert_eq!((w.start, w.end), (0.0, 10.0));
    }

    #[test]
    fn the_limit_shortens_and_the_silence_is_added_after_it() {
        let e = entry(vec![Item::new(LIMIT_DURATION), silence_end(&[])]);
        // A 4-minute song: 30 s kept, then the 2 s of silence.
        let p = plan(&e, &song(247.0, None, Some(240.0)), None);
        assert!((p.expected_s.unwrap() - 32.0).abs() < 1e-6, "{p:?}");
        assert!(p.filters[0].starts_with("atrim=start=0.000000:end=30.000000"));
    }

    #[test]
    fn fades_ride_on_the_kept_part() {
        let e = entry(vec![Item::new(LIMIT_DURATION), Item::new("fade-in"), Item::new("fade-out")]);
        let af = plan(&e, &song(200.0, None, None), None).af().unwrap();
        assert!(af.contains("afade=type=in:start_time=0:duration=2"), "{af}");
        assert!(af.contains("afade=type=out:start_time=28:duration=2"), "{af}");
        // A fade longer than the song is the whole song.
        let short = entry(vec![Item::with("fade-out", &[("length", json!(9000))])]);
        let af = plan(&short, &song(3.0, None, None), None).af().unwrap();
        assert!(af.contains("afade=type=out:start_time=0:duration=3"), "{af}");
    }

    #[test]
    fn each_volume_mode_computes_its_gain_and_only_the_peak_mode_has_no_limiter() {
        let a = Analysis { duration_s: 6.5, loudness_lufs: Some(-22.0), max_volume_db: Some(-12.0), rg_gain_db: Some(3.7), ..Analysis::default() };
        let p = plan(&entry(vec![Item::new(VOLUME_LOUDNESS)]), &a, None);
        assert_eq!(p.applied_gain_db, Some(6.0));
        assert!(p.af().unwrap().contains("volume=6.000dB,alimiter=limit=0.891251:level=0"));
        let p = plan(&entry(vec![Item::new(VOLUME_PEAK)]), &a, None);
        assert_eq!(p.applied_gain_db, Some(11.0));
        assert_eq!(p.af().unwrap(), "volume=11.000dB", "a peak target cannot clip, no limiter");
        let p = plan(&entry(vec![Item::new(VOLUME_REPLAYGAIN)]), &a, None);
        assert_eq!(p.applied_gain_db, Some(3.7));
        let album = Item::with(VOLUME_REPLAYGAIN, &[("album", json!(true))]);
        let p = plan(&entry(vec![album.clone()]), &a, Some(&Album { gain_db: -1.5, peak: 0.9 }));
        assert_eq!(p.applied_gain_db, Some(-1.5), "the album's gain when the album is asked for");
        let p = plan(&entry(vec![album]), &a, None);
        assert_eq!(p.applied_gain_db, Some(3.7), "no album found: the track's own");
        let p = plan(&entry(vec![Item::with(VOLUME_GAIN, &[("gain", json!(-4))])]), &a, None);
        assert_eq!(p.applied_gain_db, Some(-4.0));
    }

    #[test]
    fn the_volume_acts_where_it_is_listed() {
        let a = Analysis { duration_s: 6.5, max_volume_db: Some(-12.0), ..Analysis::default() };
        let above = entry(vec![Item::with("try:highpass", &[("frequency", json!(80))]), Item::new(VOLUME_PEAK)]);
        let below = entry(vec![Item::new(VOLUME_PEAK), Item::with("try:highpass", &[("frequency", json!(80))])]);
        let (up, down) = (plan(&above, &a, None).af().unwrap(), plan(&below, &a, None).af().unwrap());
        assert!(up.starts_with("highpass=frequency=80,volume="), "{up}");
        assert!(down.starts_with("volume=") && down.ends_with("highpass=frequency=80"), "{down}");
    }

    #[test]
    fn replaygain_tags_describe_the_output_file() {
        let mut e = entry(vec![Item::new(REPLAYGAIN_TAGS)]);
        let a = Analysis { duration_s: 6.5, rg_gain_db: Some(6.28), rg_peak: Some(0.088367), ..Analysis::default() };
        let p = plan(&e, &a, None);
        assert_eq!(p.tags[0], ("REPLAYGAIN_TRACK_GAIN".to_owned(), "+6.28 dB".to_owned()));
        assert_eq!(p.tags[1], ("REPLAYGAIN_TRACK_PEAK".to_owned(), "0.088367".to_owned()));
        assert_eq!(p.tags.len(), 2, "no album tags unless asked for");
        // With a gain applied the tag says what is LEFT to do, and the peak moved with it.
        e.items.push(Item::new(VOLUME_LOUDNESS));
        let a = Analysis { loudness_lufs: Some(-22.0), ..a };
        let p = plan(&e, &a, None);
        assert_eq!(p.tags[0].1, "+0.28 dB");
        let peak: f64 = p.tags[1].1.parse().unwrap();
        assert!((peak - 0.088367 * db_to_linear(6.0)).abs() < 1e-5);
        // The album's figures are written beside the track's, with the same gain taken off.
        e.items[0] = Item::with(REPLAYGAIN_TAGS, &[("album", json!(true))]);
        let p = plan(&e, &a, Some(&Album { gain_db: 5.0, peak: 0.2 }));
        assert_eq!(p.tags.len(), 4);
        assert_eq!(p.tags[2], ("REPLAYGAIN_ALBUM_GAIN".to_owned(), "-1.00 dB".to_owned()));
    }

    #[test]
    fn the_meter_runs_over_the_kept_part_only() {
        let e = entry(vec![Item::new(TRIM_SILENCE), Item::new(VOLUME_LOUDNESS)]);
        let a = song(10.0, Some(2.0), Some(8.0));
        let w = window(&e, &a);
        let f = meter_filter(&e, &w, 10.0);
        assert!(f.starts_with("atrim=start=1.950000:end=8.050000,asetpts=PTS-STARTPTS,volumedetect"), "{f}");
        let whole = entry(vec![Item::new(VOLUME_LOUDNESS)]);
        assert_eq!(meter_filter(&whole, &window(&whole, &a), 10.0), METER_FILTER);
        // The meters see what the volume will be set on: through the effects above it, not the raw cut, and not through those below.
        let p = entry(vec![Item::with("try:highpass", &[("frequency", json!(80))]), Item::new(VOLUME_LOUDNESS), Item::with("try:highpass", &[("frequency", json!(90))])]);
        assert_eq!(meter_filter(&p, &window(&p, &a), 10.0), format!("highpass=frequency=80,{METER_FILTER}"));
    }

    #[test]
    fn the_filters_are_written_in_the_order_listed() {
        let mut gains = serde_json::Map::new();
        gains.insert("g31".into(), json!(3));
        gains.insert("g1k".into(), json!(-4));
        let e = entry(vec![
            Item::with("channels", &[("mode", json!("swap"))]),
            Item::new("try:adeclick"),
            Item::with("try:highpass", &[("frequency", json!(40))]),
            Item { key: "equalizer".into(), values: gains.into_iter().collect() },
            Item::with("compressor", &[("threshold", json!(-18)), ("ratio", json!(4))]),
            Item::with(TEMPO, &[("tempo", json!(1.25))]),
        ]);
        let chain = e.try_chain(0..e.items.len(), 100.0).unwrap();
        let text = chain.join(" | ");
        let at = |n: &str| text.find(n).unwrap_or_else(|| panic!("{n} in {text}"));
        assert!(at("pan=") < at("adeclick") && at("adeclick") < at("highpass=frequency=40") && at("highpass") < at("equalizer") && at("equalizer") < at("acompressor") && at("acompressor") < at("atempo"), "{text}");
        assert!(text.contains("pan=args='stereo|c0=c1|c1=c0'"), "{text}");
        assert!(text.contains("threshold=-18dB") && text.contains("ratio=4"), "{text}");
        let reversed: Vec<Item> = e.items.iter().rev().cloned().collect();
        assert_ne!(chain, entry(reversed).try_chain(0..6, 100.0).unwrap(), "the order of the list is the order of the filters");
    }

    #[test]
    fn a_tempo_change_moves_the_length_the_fades_and_the_silence_follow() {
        let e = entry(vec![Item::with(TEMPO, &[("tempo", json!(2))]), Item::with("fade-out", &[("length", json!(1000))]), silence_end(&[])]);
        // 20 s of sound twice as fast = 10 s; the fade-out is the last second of THOSE 10, then 2 s.
        let p = plan(&e, &song(20.0, None, None), None);
        let af = p.af().unwrap();
        assert!(af.contains("atempo=tempo=2"), "{af}");
        assert!(af.contains("afade=type=out:start_time=9:duration=1"), "{af}");
        assert!((p.expected_s.unwrap() - 12.0).abs() < 1e-6, "{p:?}");
    }

    #[test]
    fn an_effect_the_catalogue_lacks_is_named_and_a_missing_catalogue_is_not_a_crash() {
        let e = entry(vec![Item::new("gone")]);
        assert!(e.check().unwrap_err().contains("not available with this ffmpeg"));
        assert!(entry(vec![Item::new(VOLUME_GAIN)]).check().is_ok(), "a measured effect needs no filter of the catalogue");
    }

    #[test]
    fn an_empty_list_changes_nothing_and_needs_no_analysis() {
        let e = EffectsEntry::standard("No effects");
        assert!(!e.needs_analysis() && !e.changes_audio());
        let p = plan(&e, &Analysis { duration_s: 10.0, ..Analysis::default() }, None);
        assert!(p.af().is_none() && p.tags.is_empty() && p.expected_s.is_none());
        // A fixed gain and fades need no measuring at all.
        let g = entry(vec![Item::new("fade-in"), Item::new(VOLUME_GAIN)]);
        assert!(!g.needs_analysis() && g.changes_audio());
    }
}
