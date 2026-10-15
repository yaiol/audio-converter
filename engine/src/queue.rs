//! The job queue: N conversions at once, each with its own progress and its own cancel.
//!
//! std threads and a mutex - the window polls `snapshot()`, so the engine needs no async runtime
//! and no callback into the UI. One worker thread takes the next queued job, probes the file,
//! resolves the output path, runs ffmpeg and reads its progress.

use crate::{
    cleanup::TagCleanup,
    destination::{self, Destination, Overwrite, Resolved},
    ffmpeg,
    preset::Preset,
};
use std::{
    io::{BufRead, BufReader},
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[derive(Clone, Debug)]
pub struct JobSpec {
    pub id: u64,
    pub input: PathBuf,
    /// The folder `input` was added through, if any - what `keep_structure` is relative to.
    pub source_root: Option<PathBuf>,
    pub preset: Preset,
    /// What to strip from the output - chosen apart from the preset (see `cleanup`).
    pub cleanup: TagCleanup,
    pub destination: Destination,
    pub overwrite: Overwrite,
    /// The effects chain - chosen apart from the preset like the tag cleaning.
    pub effects: crate::effects::EffectsEntry,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobStatus {
    Queued,
    Probing,
    /// The measuring pass an effect needs, before the conversion. 0.0 ..= 100.0
    Analysing { percent: f32 },
    /// 0.0 ..= 100.0
    Running { percent: f32 },
    Done { output: PathBuf },
    Skipped { output: PathBuf },
    Failed { reason: String },
    Cancelled,
}

impl JobStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, JobStatus::Queued | JobStatus::Probing | JobStatus::Analysing { .. } | JobStatus::Running { .. })
    }
}

struct Slot {
    spec: JobSpec,
    status: JobStatus,
    cancel: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

struct State {
    slots: Vec<Slot>,
    workers: usize,
    limit: usize,
}

struct Inner {
    ffmpeg: PathBuf,
    state: Mutex<State>,
    /// The ReplayGain figures of an album, by folder and album name: computed by the first job that
    /// needs them, from every file of the run that belongs to it, and kept for the rest. Forgotten
    /// when a new run starts, because the files may have changed.
    albums: Mutex<HashMap<String, Option<crate::effects::Album>>>,
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

impl Runner {
    pub fn new(ffmpeg: PathBuf) -> Runner {
        Runner {
            inner: Arc::new(Inner {
                ffmpeg,
                state: Mutex::new(State { slots: Vec::new(), workers: 0, limit: 1 }),
                albums: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Queue jobs and make sure up to `parallel` workers are running. A job whose id is already
    /// known is replaced - unless it is still active, in which case it is left as it is.
    pub fn start(&self, jobs: Vec<JobSpec>, parallel: usize) {
        let mut spawn = 0;
        self.inner.albums.lock().unwrap().clear();
        {
            let mut st = self.inner.state.lock().unwrap();
            st.limit = parallel.max(1);
            for spec in jobs {
                let slot = Slot {
                    spec: spec.clone(),
                    status: JobStatus::Queued,
                    cancel: Arc::new(AtomicBool::new(false)),
                    child: Arc::new(Mutex::new(None)),
                };
                match st.slots.iter().position(|s| s.spec.id == spec.id) {
                    Some(i) if st.slots[i].status.is_active() => {}
                    Some(i) => st.slots[i] = slot,
                    None => st.slots.push(slot),
                }
            }
            let queued = st.slots.iter().filter(|s| s.status == JobStatus::Queued).count();
            while st.workers < st.limit && spawn < queued {
                st.workers += 1;
                spawn += 1;
            }
        }
        for _ in 0..spawn {
            let inner = Arc::clone(&self.inner);
            thread::spawn(move || worker(inner));
        }
    }

    pub fn cancel(&self, id: u64) {
        let mut st = self.inner.state.lock().unwrap();
        if let Some(s) = st.slots.iter_mut().find(|s| s.spec.id == id) {
            cancel_slot(s);
        }
    }

    pub fn cancel_all(&self) {
        let mut st = self.inner.state.lock().unwrap();
        for s in st.slots.iter_mut() {
            cancel_slot(s);
        }
    }

    /// Forget a finished job (the window removed its row).
    pub fn remove(&self, id: u64) {
        let mut st = self.inner.state.lock().unwrap();
        st.slots.retain(|s| s.spec.id != id || s.status.is_active());
    }

    pub fn snapshot(&self) -> Vec<(u64, JobStatus)> {
        let st = self.inner.state.lock().unwrap();
        st.slots.iter().map(|s| (s.spec.id, s.status.clone())).collect()
    }

    pub fn busy(&self) -> bool {
        self.inner.state.lock().unwrap().slots.iter().any(|s| s.status.is_active())
    }
}

fn cancel_slot(s: &mut Slot) {
    match s.status {
        JobStatus::Queued => s.status = JobStatus::Cancelled,
        JobStatus::Probing | JobStatus::Analysing { .. } | JobStatus::Running { .. } => {
            s.cancel.store(true, Ordering::SeqCst);
            if let Some(c) = s.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        }
        _ => {}
    }
}

fn set_status(inner: &Inner, id: u64, status: JobStatus) {
    let mut st = inner.state.lock().unwrap();
    if let Some(s) = st.slots.iter_mut().find(|s| s.spec.id == id) {
        s.status = status;
    }
}

fn worker(inner: Arc<Inner>) {
    loop {
        let job = {
            let mut st = inner.state.lock().unwrap();
            // A shrunk limit lets surplus workers retire as soon as they finish a job.
            let idx = if st.workers > st.limit {
                None
            } else {
                st.slots.iter().position(|s| s.status == JobStatus::Queued)
            };
            match idx {
                Some(i) => {
                    let s = &mut st.slots[i];
                    s.status = JobStatus::Probing;
                    Some((s.spec.clone(), Arc::clone(&s.cancel), Arc::clone(&s.child)))
                }
                None => {
                    st.workers -= 1;
                    None
                }
            }
        };
        let Some((spec, cancel, child)) = job else { return };
        let status = run_job(&inner, &spec, &cancel, &child);
        set_status(&inner, spec.id, status);
    }
}

fn run_job(inner: &Inner, spec: &JobSpec, cancel: &AtomicBool, child_slot: &Mutex<Option<Child>>) -> JobStatus {
    let probe = match ffmpeg::probe(&inner.ffmpeg, &spec.input) {
        Ok(p) => p,
        Err(reason) => return JobStatus::Failed { reason },
    };
    if cancel.load(Ordering::SeqCst) {
        return JobStatus::Cancelled;
    }

    // "(Original)" is a rule, not a preset: this input's own format, at the bitrate it had.
    let preset = if spec.preset.is_original() {
        let ext = spec.input.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        match crate::format::Format::keeping(&probe.codec, &ext) {
            Some(f) => Preset::reproducing(f, probe.bitrate_kbps),
            None => return JobStatus::Failed { reason: format!("no output format keeps the codec \"{}\"", probe.codec) },
        }
    } else {
        spec.preset.clone()
    };
    let ext = preset.format.extension();
    let output = match destination::resolve_output(&spec.destination, &spec.input, spec.source_root.as_deref(), &probe.tags, ext, spec.overwrite) {
        Resolved::Skip(output) => return JobStatus::Skipped { output },
        Resolved::Write(p) => p,
    };
    if let Some(dir) = output.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return JobStatus::Failed { reason: format!("cannot create {}: {e}", dir.display()) };
        }
    }

    let name = spec.input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let with_cover = crate::is_audio_file(&name);
    // The stored list is the user's text; names are split and tidied only here, where they are used.
    let cleanup = spec.cleanup.clone().normalized();
    // The INFO ids of a WAV that ffmpeg could not map are not fields in the reference tagger either,
    // so they are not carried over.
    let ignored = crate::riff::unmapped_info_keys(&spec.input, &probe.tags);
    // An M4A holds a few fields in atoms ffmpeg's demuxer does not expose; they are carried by the tag
    // layer, after the cleaning, so what the entry removes is not brought back.
    let mut carried: Vec<(String, String)> = crate::tags::source_only_fields(&spec.input, &probe.tags);
    carried.retain(|(k, _)| !cleanup.drops(&crate::mapping::spellings(k)));

    // The effects that need numbers get them from measuring passes over the source, before the
    // conversion; what they find is turned into exact arguments (see `effects::plan`).
    let effects = spec.effects.clone().normalized();
    let mut plan = crate::effects::EffectsPlan::default();
    if !effects.items.is_empty() {
        match analysis_plan(inner, spec, &effects, &probe, cancel, child_slot) {
            Ok(Some(p)) => plan = p,
            Ok(None) => return JobStatus::Cancelled,
            Err(reason) => return JobStatus::Failed { reason },
        }
    }
    // The percentage is against what will be written, not what was read.
    let total_us = plan.expected_s.map_or(probe.duration_us, |s| (s * 1_000_000.0) as u64);
    // What ffmpeg's listing leaves out goes with the effects' tags: after the cleaning in the formats the
    // tag layer writes, as an ordinary tag argument in those only ffmpeg writes (WavPack, TTA...).
    plan.tags.splice(0..0, carried.iter().cloned());
    let args = ffmpeg::build_args(&spec.input, &output, &preset, &cleanup, &probe.tags, &ignored, with_cover, probe.source, &plan);

    set_status(inner, spec.id, JobStatus::Running { percent: 0.0 });
    let mut child = match ffmpeg::command(&inner.ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return JobStatus::Failed { reason: format!("cannot run ffmpeg: {e}") },
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *child_slot.lock().unwrap() = Some(child);
    // Cancelled between the check above and the spawn: the kill found no child, so do it now.
    if cancel.load(Ordering::SeqCst) {
        if let Some(c) = child_slot.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    // stderr must be drained or ffmpeg blocks on a full pipe; keep its tail for the error message.
    let errors = thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        if let Some(e) = stderr {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                let line = line.trim().to_owned();
                if !line.is_empty() {
                    tail.push(line);
                    if tail.len() > 12 {
                        tail.remove(0);
                    }
                }
            }
        }
        tail
    });

    if let Some(out) = stdout {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(us) = ffmpeg::parse_progress_line(&line) {
                if total_us > 0 {
                    let pct = (us as f64 / total_us as f64 * 100.0).clamp(0.0, 99.9) as f32;
                    set_status(inner, spec.id, JobStatus::Running { percent: pct });
                }
            }
        }
    }

    let exit = child_slot.lock().unwrap().take().map(|mut c| c.wait());
    let tail = errors.join().unwrap_or_default();

    if cancel.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&output);
        return JobStatus::Cancelled;
    }
    match exit {
        Some(Ok(s)) if s.success() => {
            // ffmpeg joins a repeated field into one value; the tag library writes them back as
            // separate values. A failure here keeps ffmpeg's tags: the file is still good.
            let _ = crate::tags::restore_repeated_fields(&inner.ffmpeg, &spec.input, &output, &preset, &cleanup, &probe.tags, &ignored, probe.source, &plan.tags);
            JobStatus::Done { output }
        }
        _ => {
            let _ = std::fs::remove_file(&output);
            let reason = tail.last().cloned().unwrap_or_else(|| "ffmpeg stopped without a message".to_owned());
            JobStatus::Failed { reason }
        }
    }
}

/// One measuring pass: ffmpeg runs `args` (everything between the program and the progress options)
/// to nowhere, and what it printed is returned. `duration_us` is how long the measured audio is, so
/// the progress can be read from `-progress` and mapped onto `range` of the Analysing percentage.
/// `Ok(None)` = cancelled while it ran.
fn run_pass(
    inner: &Inner,
    id: u64,
    args: &[std::ffi::OsString],
    duration_us: u64,
    range: (f32, f32),
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<Option<String>, String> {
    let mut child = ffmpeg::command(&inner.ffmpeg)
        .args(["-hide_banner", "-nostdin", "-nostats", "-loglevel", "info"])
        .args(args)
        .args(["-f", "null", "-progress", "pipe:1", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *child_slot.lock().unwrap() = Some(child);
    if cancel.load(Ordering::SeqCst) {
        if let Some(c) = child_slot.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
    // The meters' report comes at the end, on stderr: it is all kept, the progress is on stdout.
    let report = thread::spawn(move || {
        let mut text = String::new();
        if let Some(e) = stderr {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    });
    if let Some(out) = stdout {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(us) = ffmpeg::parse_progress_line(&line) {
                if duration_us > 0 {
                    let part = (us as f64 / duration_us as f64).clamp(0.0, 1.0) as f32;
                    let pct = (range.0 + (range.1 - range.0) * part).min(99.9);
                    set_status(inner, id, JobStatus::Analysing { percent: pct });
                }
            }
        }
    }
    let exit = child_slot.lock().unwrap().take().map(|mut c| c.wait());
    let text = report.join().unwrap_or_default();
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    match exit {
        Some(Ok(s)) if s.success() => Ok(Some(text)),
        _ => Err(text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("the measuring pass failed").trim().to_owned()),
    }
}

fn os(s: impl Into<std::ffi::OsString>) -> std::ffi::OsString {
    s.into()
}

/// Everything the effects need to know about one file, then the plan they make from it.
///
/// At most two passes: the SCAN (where the sound starts and stops) when a trim or the silence at the
/// end asks for it, and the METERS (loudness, peak, ReplayGain) when the volume or the tags ask for
/// them - run over the part that is kept, so a track cut to 30 s is leveled for those 30 s. The
/// album's figures, when an effect uses them, come from the files of its folder and album together.
/// `Ok(None)` = cancelled.
fn analysis_plan(
    inner: &Inner,
    spec: &JobSpec,
    effects: &crate::effects::EffectsEntry,
    probe: &ffmpeg::Probe,
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<Option<crate::effects::EffectsPlan>, String> {
    use crate::effects as fx;
    let duration_s = probe.duration_us as f64 / 1_000_000.0;
    let (scan, meter) = (effects.needs_scan(), effects.needs_meter());
    let mut a = fx::Analysis { duration_s, ..fx::Analysis::default() };
    let both = scan && meter;
    if scan || meter || effects.needs_album() {
        set_status(inner, spec.id, JobStatus::Analysing { percent: 0.0 });
    }
    if scan {
        let args = [
            os("-i"),
            os(&spec.input),
            os("-map"),
            os("0:a:0"),
            os("-vn"),
            os("-af"),
            os(fx::scan_filter(effects.scan_threshold_db())),
        ];
        let range = (0.0, if both { 50.0 } else { 100.0 });
        let Some(text) = run_pass(inner, spec.id, &args, probe.duration_us, range, cancel, child_slot)? else {
            return Ok(None);
        };
        a = fx::parse_analysis(&text, duration_s);
    }
    if meter {
        let w = fx::window(effects, &a);
        let args = [
            os("-i"),
            os(&spec.input),
            os("-map"),
            os("0:a:0"),
            os("-vn"),
            os("-af"),
            os(fx::meter_filter(effects, &w, duration_s)),
        ];
        let range = (if both { 50.0 } else { 0.0 }, 100.0);
        // What the meter pass writes is the kept part after the tempo change.
        let kept_us = (w.length() / effects.tempo_ratio() * 1_000_000.0) as u64;
        let Some(text) = run_pass(inner, spec.id, &args, kept_us, range, cancel, child_slot)? else {
            return Ok(None);
        };
        let m = fx::parse_analysis(&text, w.length() / effects.tempo_ratio());
        a.loudness_lufs = m.loudness_lufs;
        a.true_peak_db = m.true_peak_db;
        a.max_volume_db = m.max_volume_db;
        a.rg_gain_db = m.rg_gain_db;
        a.rg_peak = m.rg_peak;
    }
    let album = if effects.needs_album() {
        let found = album_gain(inner, spec, probe, cancel, child_slot)?;
        if cancel.load(Ordering::SeqCst) {
            return Ok(None);
        }
        found
    } else {
        None
    };
    Ok(Some(fx::plan(effects, &a, album.as_ref())))
}

/// The ReplayGain gain and peak of the album this file belongs to: the files of the run that sit in
/// the same folder and carry the same album tag, measured as one continuous program (the `concat`
/// filter, each file brought to 44.1 kHz stereo first). `None` when the file has no album tag - there
/// is no album to speak of - or the measuring fails. Computed once per album and kept.
fn album_gain(
    inner: &Inner,
    spec: &JobSpec,
    probe: &ffmpeg::Probe,
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<Option<crate::effects::Album>, String> {
    let album = probe.tags.get("album").map(|a| a.trim().to_lowercase()).unwrap_or_default();
    if album.is_empty() {
        return Ok(None);
    }
    let folder = spec.input.parent().map(Path::to_path_buf).unwrap_or_default();
    let key = format!("{}\u{1}{}", folder.display(), album);
    // Held while the album is measured: the other jobs of the album need the same answer.
    let mut cache = inner.albums.lock().unwrap();
    if let Some(found) = cache.get(&key) {
        return Ok(*found);
    }
    let peers: Vec<PathBuf> = {
        let st = inner.state.lock().unwrap();
        st.slots.iter().filter(|s| s.spec.input.parent() == Some(folder.as_path())).map(|s| s.spec.input.clone()).collect()
    };
    let mut members: Vec<(PathBuf, u64)> = Vec::new();
    for p in peers {
        if members.iter().any(|(m, _)| *m == p) {
            continue;
        }
        let same = if p == spec.input {
            Some(probe.duration_us)
        } else {
            ffmpeg::probe(&inner.ffmpeg, &p)
                .ok()
                .filter(|q| q.tags.get("album").map(|a| a.trim().to_lowercase()).as_deref() == Some(album.as_str()))
                .map(|q| q.duration_us)
        };
        if let Some(d) = same {
            members.push((p, d));
        }
    }
    members.sort();
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    let mut graph = String::new();
    for (i, (p, _)) in members.iter().enumerate() {
        args.push(os("-i"));
        args.push(os(p));
        graph.push_str(&format!("[{i}:a:0]aresample=44100,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];"));
    }
    for i in 0..members.len() {
        graph.push_str(&format!("[a{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=0:a=1,replaygain", members.len()));
    args.extend([os("-filter_complex"), os(graph)]);
    let total_us: u64 = members.iter().map(|(_, d)| *d).sum();
    let found = match run_pass(inner, spec.id, &args, total_us, (0.0, 99.0), cancel, child_slot)? {
        Some(text) => {
            let m = crate::effects::parse_analysis(&text, 0.0);
            match (m.rg_gain_db, m.rg_peak) {
                (Some(gain_db), Some(peak)) => Some(crate::effects::Album { gain_db, peak }),
                _ => None,
            }
        }
        None => return Ok(None),
    };
    cache.insert(key, found);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::preset::Id3Version;
    use crate::cleanup::CleanupMode;
    use std::{
        collections::HashMap,
        path::Path,
        time::{Duration, Instant},
    };

    /// These run the REAL ffmpeg when there is one and are skipped quietly when there is not, so a
    /// machine without it still passes `cargo test`.
    fn ffmpeg() -> Option<PathBuf> {
        let f = ffmpeg::locate();
        if f.is_none() {
            eprintln!("ffmpeg not found - integration test skipped");
        }
        f
    }

    fn tone(ffmpeg: &Path, dir: &Path, name: &str, seconds: u32) -> PathBuf {
        let out = dir.join(name);
        let ok = ffmpeg::command(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:duration={seconds}"))
            .args(["-metadata", "title=Test Tone", "-metadata", "artist=Tester", "-metadata", "track=2"])
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok, "could not generate the test tone");
        out
    }

    fn wait_idle(r: &Runner) {
        let t = Instant::now();
        while r.busy() {
            assert!(t.elapsed() < Duration::from_secs(60), "queue did not finish");
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yaiol-queue-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_format_converts_with_a_real_ffmpeg() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("formats");
        let src = tone(&ff, &dir, "tone.wav", 2);
        let out = dir.join("out");
        let dest = Destination { folder: out.to_string_lossy().into_owned(), pattern: "{artist} - {title}".into(), ..Destination::new("t", "") };
        let r = Runner::new(ff);
        let jobs: Vec<JobSpec> = Format::ALL
            .iter()
            .enumerate()
            .map(|(i, f)| JobSpec {
                id: i as u64,
                input: src.clone(),
                source_root: None,
                preset: Preset::new("t", *f),
                cleanup: TagCleanup::default(),
                destination: dest.clone(),
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            })
            .collect();
        r.start(jobs, 3);
        wait_idle(&r);
        for (id, status) in r.snapshot() {
            let f = Format::ALL[id as usize];
            match status {
                JobStatus::Done { output } => {
                    assert!(output.exists(), "{f:?}");
                    assert_eq!(output.extension().unwrap(), f.extension());
                    assert!(output.file_stem().unwrap().to_string_lossy().starts_with("Tester - Test Tone"), "{output:?}");
                    assert!(std::fs::metadata(&output).unwrap().len() > 1000, "{f:?} is suspiciously small");
                }
                other => panic!("{f:?}: {other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// What the output holds, as ffmpeg itself reports it: (tags, has a picture stream).
    fn describe(ff: &Path, file: &Path) -> (HashMap<String, String>, bool) {
        let out = ffmpeg::command(ff).args(["-hide_banner", "-nostdin", "-i"]).arg(file).output().unwrap();
        let text = String::from_utf8_lossy(&out.stderr).into_owned();
        (ffmpeg::parse_probe(&text).tags, text.contains("Video:"))
    }

    #[test]
    fn tags_and_cover_art_per_format() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("cover");
        // A FLAC source with tags and an attached picture.
        let png = dir.join("cover.png");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=64x64", "-frames:v", "1"])
            .arg(&png)
            .status()
            .unwrap()
            .success());
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-i"])
            .arg(&png)
            .args(["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"])
            .args(["-metadata", "title=Song", "-metadata", "artist=Me", "-metadata", "album=Disc", "-metadata", "track=3/10"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        assert!(describe(&ff, &src).1, "the test source has no picture");

        let r = Runner::new(ff.clone());
        let out = dir.join("out");
        let dest = Destination { pattern: "{name}-{title}".into(), ..Destination::new("t", &out.to_string_lossy()) };
        for (i, f) in Format::ALL.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *f),
                    cleanup: TagCleanup::default(),
                    destination: dest.clone(),
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                1,
            );
        }
        wait_idle(&r);
        let mut report = Vec::new();
        for (id, s) in r.snapshot() {
            let f = Format::ALL[id as usize];
            let JobStatus::Done { output } = s else { panic!("{f:?}: {s:?}") };
            let (tags, picture) = describe(&ff, &output);
            let has = |k: &str, v: &str| tags.get(k).map(String::as_str) == Some(v);
            report.push(format!("{:>5}: title={} artist={} album={} track={} picture={}", f.id(), has("title", "Song"), has("artist", "Me"), has("album", "Disc"), tags.get("track").cloned().unwrap_or_default(), picture));
            // The picture follows what the container can hold - never more, never less.
            assert_eq!(picture, f.holds_cover(),  "{f:?}
{}", report.join("
"));
            // Raw AC-3 is a bare stream: no tags at all.
            if f.holds_tags() {
                assert!(has("title", "Song") && has("artist", "Me"), "{f:?} lost its tags
{}", report.join("
"));
            }
        }
        eprintln!("{}", report.join("
"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wma_gets_windows_names_a_real_cover_and_honours_the_removal_list() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("wma");
        let png = dir.join("cover.png");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=blue:s=64x64", "-frames:v", "1"])
            .arg(&png)
            .status()
            .unwrap()
            .success());
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-i"])
            .arg(&png)
            .args(["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"])
            .args(["-metadata", "title=Song", "-metadata", "artist=Me", "-metadata", "date=2024-12-22"])
            .args(["-metadata", "RELEASETYPE=LP", "-metadata", "LYRICIST=Poet", "-metadata", "AMLP_ID=suno:1"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() };
        let has = |bytes: &[u8], s: &str| {
            let needle = utf16(s);
            bytes.windows(needle.len()).any(|w| w == needle.as_slice())
        };

        let r = Runner::new(ff.clone());
        let out = dir.join("out");
        let dest = Destination { pattern: "{name}-{title}".into(), ..Destination::new("t", &out.to_string_lossy()) };
        let preset = Preset::new("wma", Format::Wma);
        let cleaned = TagCleanup {
            exceptions: vec!["LYRICIST".into(), "AMLP*".into()],
            remove_cover: true,
            ..TagCleanup::new("clean")
        };
        for (i, c) in [TagCleanup::keep(), cleaned].into_iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: preset.clone(),
                    cleanup: c,
                    destination: Destination { pattern: format!("{{name}}-{i}"), ..dest.clone() },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                1,
            );
        }
        wait_idle(&r);
        let snap = r.snapshot();
        let output = |id: u64| match &snap.iter().find(|(i, _)| *i == id).unwrap().1 {
            JobStatus::Done { output } => output.clone(),
            s => panic!("{s:?}"),
        };

        let bytes = std::fs::read(output(0)).unwrap();
        for name in ["WM/Year", "MusicBrainz/Album Type", "WM/Writer", "AMLP_ID", "WM/Picture"] {
            assert!(has(&bytes, name), "{name} missing from the plain WMA");
        }
        let (tags, picture) = describe(&ff, &output(0));
        assert!(picture, "the cover is not readable as a picture: {tags:?}");
        assert_eq!(tags.get("title").map(String::as_str), Some("Song"));
        // The whole file still decodes after its header was rewritten.
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-v", "error", "-i"])
            .arg(output(0))
            .args(["-f", "null", "-"])
            .output()
            .map(|o| o.status.success() && o.stderr.is_empty())
            .unwrap_or(false));

        let bytes = std::fs::read(output(1)).unwrap();
        assert!(has(&bytes, "WM/Year"), "the date was lost");
        assert!(!has(&bytes, "WM/Writer"), "LYRICIST was not removed");
        assert!(!has(&bytes, "AMLP_ID"), "the wildcard did not remove AMLP_ID");
        assert!(!has(&bytes, "WM/Picture"), "the cover was not removed");
        let (_, picture) = describe(&ff, &output(1));
        assert!(!picture);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Every advanced option, set to a non-default value, must be accepted by the shipped ffmpeg:
    /// an option name or value ffmpeg does not know fails the conversion outright.
    #[test]
    fn every_advanced_option_is_accepted_by_ffmpeg() {
        use crate::options::{FlacStereo, OpusApplication, OpusBandwidth, OpusVbr};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("advanced");
        let src = tone(&ff, &dir, "tone.wav", 2);

        let mut mp3 = Preset::new("mp3", Format::Mp3);
        mp3.rate_control = crate::preset::RateControl::Abr;
        mp3.options.mp3_algorithm = Some(2);
        mp3.options.mp3_joint_stereo = Some(true);
        mp3.options.mp3_lowpass_hz = Some(16000);
        mp3.options.mp3_highpass_hz = Some(60);
        mp3.options.mp3_reservoir = false;
        mp3.options.mp3_copyright = true;
        mp3.options.mp3_original = false;

        let mut ogg = Preset::new("ogg", Format::Ogg);
        ogg.bitrate_kbps = 160;
        ogg.options.ogg_min_kbps = Some(96);
        ogg.options.ogg_max_kbps = Some(224);

        let mut opus = Preset::new("opus", Format::Opus);
        opus.options.opus_application = OpusApplication::Voip;
        opus.options.opus_vbr = OpusVbr::Constrained;
        opus.options.opus_frame_tenths = Some(400);
        opus.options.opus_complexity = Some(5);
        opus.options.opus_bandwidth = Some(OpusBandwidth::Wide);
        opus.options.opus_phase_inversion = false;

        let mut flac = Preset::new("flac", Format::Flac);
        flac.options.flac_block_size = Some(4608);
        flac.options.flac_stereo = Some(FlacStereo::MidSide);
        flac.options.flac_max_lpc = Some(8);
        flac.options.flac_lpc_precision = Some(12);
        flac.options.flac_min_partition = Some(1);
        flac.options.flac_max_partition = Some(5);
        flac.options.flac_exhaustive = true;

        let mut oggflac = flac.clone();
        oggflac.format = Format::OggFlac;

        let mut aac = Preset::new("aac", Format::Aac);
        aac.options.aac_cutoff_hz = Some(16000);
        let mut adts = aac.clone();
        adts.format = Format::AacAdts;

        let cases = [mp3, ogg, opus, flac, oggflac, aac, adts];
        let r = Runner::new(ff.clone());
        let out = dir.join("out");
        for (i, p) in cases.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: p.clone().normalized(),
                    cleanup: TagCleanup::default(),
                    destination: Destination { folder: out.to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                2,
            );
        }
        wait_idle(&r);
        for (id, s) in r.snapshot() {
            let p = &cases[id as usize];
            match s {
                JobStatus::Done { output } => {
                    let len = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                    assert!(len > 500, "{:?} produced an empty file", p.format);
                }
                other => panic!("{:?} with its advanced options: {other:?}", p.format),
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn progress_climbs_while_a_file_converts() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("progress");
        let src = tone(&ff, &dir, "long.wav", 900);
        let r = Runner::new(ff);
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset { bitrate_kbps: 320, ..Preset::new("t", Format::Mp3) },
                cleanup: TagCleanup::default(),
                destination: Destination::source(),
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        let mut seen: Vec<u32> = Vec::new();
        let t = Instant::now();
        while r.busy() {
            assert!(t.elapsed() < Duration::from_secs(120));
            if let Some((_, JobStatus::Running { percent })) = r.snapshot().into_iter().next() {
                let p = percent.round() as u32;
                if seen.last() != Some(&p) {
                    seen.push(p);
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        eprintln!("progress samples: {seen:?} in {:?}", t.elapsed());
        assert!(seen.iter().any(|p| *p > 0 && *p < 100), "never saw an intermediate value: {seen:?}");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress went backwards: {seen:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn progress_climbs_for_a_file_with_cover_art_too() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("progress-cover");
        let png = dir.join("c.png");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=500x500", "-frames:v", "1"])
            .arg(&png)
            .status()
            .unwrap()
            .success());
        let src = dir.join("long.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=900", "-i"])
            .arg(&png)
            .args(["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"])
            .args(["-metadata", "title=T"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff);
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset { bitrate_kbps: 320, ..Preset::new("t", Format::Mp3) },
                cleanup: TagCleanup::default(),
                destination: Destination::source(),
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        let mut seen: Vec<u32> = Vec::new();
        let t = Instant::now();
        while r.busy() {
            assert!(t.elapsed() < Duration::from_secs(120));
            if let Some((_, JobStatus::Running { percent })) = r.snapshot().into_iter().next() {
                let p = percent.round() as u32;
                if seen.last() != Some(&p) {
                    seen.push(p);
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        eprintln!("progress samples (with cover): {seen:?} in {:?}", t.elapsed());
        assert!(seen.iter().any(|p| *p > 0 && *p < 100), "never saw an intermediate value: {seen:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A FLAC whose tag has a repeated field and a free-form field - the shape of a real library
    /// that ffmpeg alone flattens. Built with the FORMAT-SPECIFIC tag type: lofty's generic tag drops
    /// free-form fields on a save, which is why the converter patches instead of rewriting.
    fn multi_value_source(ff: &Path, dir: &Path) -> PathBuf {
        use lofty::{config::{ParseOptions, WriteOptions}, file::AudioFile, flac::FlacFile};
        let src = dir.join("multi.flac");
        assert!(ffmpeg::command(ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "flac"])
            .args(["-metadata", "TITLE=Baroque", "-metadata", "ALBUM=Anthology", "-metadata", "ARTIST=yaiol"])
            .args(["-metadata", "AMLP_ID=suno:6c177a46", "-metadata", "RELEASETYPE=LP"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let mut file = std::fs::File::open(&src).unwrap();
        let mut flac = FlacFile::read_from(&mut file, ParseOptions::new()).unwrap();
        drop(file);
        let vc = flac.vorbis_comments_mut().expect("ffmpeg wrote vorbis comments");
        vc.push("ALBUMARTIST".to_owned(), "yaiol".to_owned());
        vc.push("ALBUMARTIST".to_owned(), "Ars Ricercata".to_owned());
        flac.save_to_path(&src, WriteOptions::default()).unwrap();
        src
    }

    #[test]
    fn repeated_and_free_form_fields_survive_in_every_format_that_can_hold_them() {
        use lofty::{file::TaggedFileExt, tag::ItemKey};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("multi");
        let src = multi_value_source(&ff, &dir);
        eprintln!("source tags seen by ffmpeg: {:?}", describe(&ff, &src).0);
        let r = Runner::new(ff.clone());
        let formats = [Format::Mp3, Format::Flac, Format::Aac, Format::Alac, Format::Ogg, Format::Opus, Format::Aiff, Format::Wav];
        for (i, f) in formats.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *f),
                    cleanup: TagCleanup::default(),
                    destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                2,
            );
        }
        wait_idle(&r);
        let mut report = Vec::new();
        for (id, s) in r.snapshot() {
            let f = formats[id as usize];
            let JobStatus::Done { output } = s else { panic!("{f:?}: {s:?}") };
            let tf = lofty::read_from_path(&output).unwrap();
            let tag = tf.primary_tag().or_else(|| tf.first_tag()).expect("output has a tag");
            let artists: Vec<String> = tag.get_strings(ItemKey::AlbumArtist).map(str::to_owned).collect();
            // The free-form fields ffmpeg carried must still be there after the patch.
            let (raw, _) = describe(&ff, &output);
            let has_custom = raw.get("amlp_id").map(String::as_str) == Some("suno:6c177a46");
            report.push(format!("{:>5}: albumartist={artists:?} pictures={} amlp_id_kept={has_custom}", f.id(), tag.pictures().len()));
        }
        eprintln!("{}", report.join("
"));
        for line in &report {
            assert!(line.contains("\"yaiol\", \"Ars Ricercata\""), "a repeated field was flattened or lost:
{}", report.join("
"));
            assert!(line.contains("amlp_id_kept=true"), "a free-form field was lost:
{}", report.join("
"));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn both_id3_versions_keep_a_repeated_field_as_separate_values() {
        use lofty::{file::TaggedFileExt, tag::ItemKey};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("id3");
        let src = multi_value_source(&ff, &dir);
        let r = Runner::new(ff.clone());
        for (i, v) in [Id3Version::V23, Id3Version::V24].into_iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset { id3_version: v, ..Preset::new("t", Format::Mp3) },
                    cleanup: TagCleanup::default(),
                    destination: Destination { folder: dir.join(format!("out{i}")).to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                1,
            );
        }
        wait_idle(&r);
        let snap = r.snapshot();
        for (id, want) in [(0u64, 3u8), (1, 4)] {
            let JobStatus::Done { output } = snap.iter().find(|(i, _)| *i == id).unwrap().1.clone() else { panic!("{snap:?}") };
            let bytes = std::fs::read(&output).unwrap();
            assert_eq!(&bytes[..3], b"ID3");
            assert_eq!(bytes[3], want, "the file is not ID3v2.{want}");
            let tf = lofty::read_from_path(&output).unwrap();
            let artists: Vec<String> = tf.primary_tag().unwrap().get_strings(ItemKey::AlbumArtist).map(str::to_owned).collect();
            assert_eq!(artists, ["yaiol", "Ars Ricercata"], "ID3v2.{want}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn listed_tags_and_the_cover_are_removed_and_nothing_else() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("clean");
        let png = dir.join("c.png");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=64x64", "-frames:v", "1"])
            .arg(&png)
            .status()
            .unwrap()
            .success());
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-i"])
            .arg(&png)
            .args(["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"])
            .args(["-metadata", "TITLE=Keep me", "-metadata", "ARTIST=Me", "-metadata", "RELEASETYPE=LP"])
            .args(["-metadata", "COMMENT=junk", "-metadata", "AMLP_ID=suno:1", "-metadata", "ENCODERSETTINGS=Lavf"])
            .args(["-metadata", "REPLAYGAIN_TRACK_GAIN=-6 dB", "-metadata", "REPLAYGAIN_ALBUM_GAIN=-5 dB"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let remove: Vec<String> = ["comment", "REPLAYGAIN_*", "AMLP_ID", "ENCODERSETTINGS"].map(String::from).to_vec();
        let formats = [Format::Mp3, Format::Flac, Format::Aac, Format::Ogg, Format::Opus, Format::Aiff, Format::Wav];
        let r = Runner::new(ff.clone());
        for (i, f) in formats.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *f),
                    cleanup: TagCleanup { exceptions: remove.clone(), remove_cover: true, ..TagCleanup::new("c") },
                    destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                3,
            );
        }
        wait_idle(&r);
        let mut report = Vec::new();
        for (id, s) in r.snapshot() {
            let f = formats[id as usize];
            let JobStatus::Done { output } = s else { panic!("{f:?}: {s:?}") };
            let (tags, picture) = describe(&ff, &output);
            let has = |k: &str| tags.contains_key(k);
            report.push(format!(
                "{:>5}: title={} artist={} releasetype={} | comment={} amlp_id={} replaygain={} encodersettings={} picture={}",
                f.id(), has("title"), has("artist"), has("releasetype"), has("comment"), has("amlp_id"),
                tags.keys().any(|k| k.starts_with("replaygain")), has("encodersettings") || has("encoder_settings"), picture
            ));
        }
        eprintln!("{}", report.join("
"));
        for line in &report {
            assert!(line.contains("title=true artist=true releasetype=true"), "something that should stay was lost:
{}", report.join("
"));
            assert!(line.contains("comment=false amlp_id=false replaygain=false encodersettings=false picture=false"), "something listed survived:
{}", report.join("
"));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_all_leaves_no_text_tag_in_any_format() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("clean-all");
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=Gone", "-metadata", "ARTIST=Gone", "-metadata", "ALBUM=Gone", "-metadata", "RELEASETYPE=LP"])
            .args(["-metadata", "COMMENT=junk", "-metadata", "AMLP_ID=suno:1", "-metadata", "DATE=2024"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        let formats = Format::ALL;
        for (i, f) in formats.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *f),
                    cleanup: TagCleanup { mode: CleanupMode::Delete, ..TagCleanup::new("c") },
                    destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                3,
            );
        }
        wait_idle(&r);
        let mut left = Vec::new();
        for (id, s) in r.snapshot() {
            let f = formats[id as usize];
            let JobStatus::Done { output } = s else { panic!("{f:?}: {s:?}") };
            let (tags, _) = describe(&ff, &output);
            for k in ["title", "artist", "album", "releasetype", "comment", "amlp_id", "date", "year"] {
                if tags.contains_key(k) {
                    left.push(format!("{}: {k}", f.id()));
                }
            }
        }
        assert!(left.is_empty(), "tags survived remove-all: {left:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_all_except_the_listed_keeps_exactly_those() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("clean-except");
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=Stay", "-metadata", "ARTIST=Stay", "-metadata", "ALBUM=Go", "-metadata", "COMMENT=Go"])
            .args(["-metadata", "AMLP_ID=suno:1", "-metadata", "DATE=2024"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        let formats: Vec<Format> = Format::ALL.into_iter().filter(|f| f.holds_tags()).collect();
        for (i, f) in formats.iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *f),
                    cleanup: TagCleanup {
                        mode: CleanupMode::Delete,
                        exceptions: vec!["TITLE".into(), "artist".into()],
                        ..TagCleanup::new("c")
                    },
                    destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                3,
            );
        }
        wait_idle(&r);
        let mut wrong = Vec::new();
        for (id, s) in r.snapshot() {
            let f = formats[id as usize];
            let JobStatus::Done { output } = s else { panic!("{f:?}: {s:?}") };
            let (tags, _) = describe(&ff, &output);
            for k in ["title", "artist"] {
                if !tags.contains_key(k) {
                    wrong.push(format!("{}: {k} was lost", f.id()));
                }
            }
            for k in ["album", "comment", "amlp_id", "date", "year"] {
                if tags.contains_key(k) {
                    wrong.push(format!("{}: {k} survived", f.id()));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names_separated_by_a_semicolon_in_one_entry_are_all_used() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("semi");
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=Stay", "-metadata", "COMMENT=Go", "-metadata", "ALBUM=Go"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Flac),
                // ONE stored entry, as the box keeps what was typed.
                cleanup: TagCleanup { exceptions: vec!["comment; ALBUM".into()], ..TagCleanup::new("c") },
                destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        let (_, s) = r.snapshot().into_iter().next().unwrap();
        let JobStatus::Done { output } = s else { panic!("{s:?}") };
        let (tags, _) = describe(&ff, &output);
        assert!(tags.contains_key("title"));
        assert!(!tags.contains_key("comment") && !tags.contains_key("album"), "{tags:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stray_tyer_frame_does_not_blank_the_year_of_an_mp3() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("tyer");
        let src = dir.join("src.flac");
        // The source carries the date AND a raw TYER field holding the whole date.
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=T", "-metadata", "DATE=2025-11-24", "-metadata", "TYER=2025-11-24"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Mp3),
                cleanup: TagCleanup::default(),
                destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        let (_, s) = r.snapshot().into_iter().next().unwrap();
        let JobStatus::Done { output } = s else { panic!("{s:?}") };
        let bytes = std::fs::read(&output).unwrap();
        let at = bytes.windows(4).position(|w| w == b"TYER").expect("a TYER frame");
        let size = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = &bytes[at + 10..at + 10 + size];
        // ISO-8859-1 text after the encoding byte, NUL terminated: a bare year, not the whole date.
        let text = String::from_utf8_lossy(&body[1..]).trim_end_matches('\0').to_owned();
        assert_eq!(text, "2025", "the year frame holds {text:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_full_date_reaches_an_mp3_as_year_day_month_and_time() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("fulldate");
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=T", "-metadata", "DATE=2024-12-31T23:59:59"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Mp3),
                cleanup: TagCleanup::default(),
                destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        let (_, s) = r.snapshot().into_iter().next().unwrap();
        let JobStatus::Done { output } = s else { panic!("{s:?}") };
        let bytes = std::fs::read(&output).unwrap();
        // The text of a v2.3 frame: an encoding byte, then ISO-8859-1 or UTF-16 (BOM first).
        let frame = |id: &[u8; 4]| -> String {
            let at = bytes.windows(4).position(|w| w == id).unwrap_or_else(|| panic!("no {} frame", String::from_utf8_lossy(id)));
            let size = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
            let body = &bytes[at + 10..at + 10 + size];
            let text: Vec<u16> = if body[0] == 1 {
                body[3..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
            } else {
                body[1..].iter().map(|&b| u16::from(b)).collect()
            };
            String::from_utf16_lossy(&text).trim_end_matches('\0').to_owned()
        };
        assert_eq!(frame(b"TYER"), "2024");
        assert_eq!(frame(b"TDAT"), "3112", "day then month");
        assert_eq!(frame(b"TIME"), "2359", "hour then minute");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wav_whose_year_frame_is_not_a_year_still_gets_its_id3_chunk() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("strict-wav");
        let plain = tone(&ff, &dir, "plain.wav", 1);
        // A tagger writes whatever the YEAR field holds into the year frame, here a note and not a year.
        // An ID3v2.3 year is four digits, and the tag library's strict mode refuses the whole file over it.
        let frame = |id: &[u8; 4], body: &[u8]| {
            let mut v = id.to_vec();
            v.extend((body.len() as u32).to_be_bytes());
            v.extend([0, 0]);
            v.extend(body);
            v
        };
        let mut tit2 = vec![1u8, 0xFF, 0xFE];
        tit2.extend("Kept".encode_utf16().flat_map(|u| u.to_le_bytes()));
        let mut tyer = vec![0u8];
        tyer.extend(b"circa 2025");
        let frames = [frame(b"TIT2", &tit2), frame(b"TYER", &tyer)].concat();
        let n = frames.len();
        let syncsafe = [((n >> 21) & 127) as u8, ((n >> 14) & 127) as u8, ((n >> 7) & 127) as u8, (n & 127) as u8];
        let mut tag = b"ID3".to_vec();
        tag.extend([3, 0, 0]);
        tag.extend(syncsafe);
        tag.extend(frames);
        let mut chunk = b"ID3 ".to_vec();
        chunk.extend((tag.len() as u32).to_le_bytes());
        chunk.extend(&tag);
        if tag.len() % 2 == 1 {
            chunk.push(0);
        }
        let mut bytes = std::fs::read(&plain).unwrap();
        bytes.extend(&chunk);
        let riff_size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        let src = dir.join("src.wav");
        std::fs::write(&src, bytes).unwrap();
        assert!(
            lofty::read_from_path(&src).is_err(),
            "the strict reader accepted the file: this test no longer exercises the fallback"
        );

        let r = Runner::new(ff.clone());
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Wav),
                cleanup: TagCleanup::default(),
                destination: Destination { folder: dir.join("out").to_string_lossy().into_owned(), ..Destination::new("t", "") },
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        let (_, s) = r.snapshot().into_iter().next().unwrap();
        let JobStatus::Done { output } = s else { panic!("{s:?}") };
        let bytes = std::fs::read(&output).unwrap();
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle));
        assert!(has(b"id3 "), "the WAV has no ID3 chunk");
        assert!(has(b"TIT2"), "the ID3 chunk has no title frame");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fields_with_a_frame_of_their_own_are_written_as_that_frame_not_a_custom_one() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("real-frames");
        let src = dir.join("src.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=T", "-metadata", "LYRICIST=Lyr", "-metadata", "CONDUCTOR=Con"])
            .args(["-metadata", "CONTENTGROUP=Grp", "-metadata", "ALBUMSORT=AlbS", "-metadata", "ARTISTSORT=ArtS"])
            .args(["-metadata", "TITLESORT=TitS", "-metadata", "ALBUMARTISTSORT=AaS", "-metadata", "COMPOSERSORT=CoS"])
            .args(["-metadata", "BPM=121", "-metadata", "ISRC=ISRC123", "-metadata", "LYRICS=Some words"])
            .args(["-metadata", "CUSTOMFIELD=Stays"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        let formats = [Format::Mp3, Format::Aiff, Format::Wav];
        for (i, fmt) in formats.iter().enumerate() {
            for (j, v) in [Id3Version::V23, Id3Version::V24].into_iter().enumerate() {
                r.start(
                    vec![JobSpec {
                        id: (i * 2 + j) as u64,
                        input: src.clone(),
                        source_root: None,
                        preset: Preset { id3_version: v, ..Preset::new("t", *fmt) },
                        cleanup: TagCleanup::default(),
                        destination: Destination {
                            folder: dir.join(format!("out-{i}-{j}")).to_string_lossy().into_owned(),
                            ..Destination::new("t", "")
                        },
                        overwrite: Overwrite::Rename,
                        effects: Default::default(),
                    }],
                    2,
                );
            }
        }
        wait_idle(&r);
        let mut wrong = Vec::new();
        for (id, s) in r.snapshot() {
            let (fmt, v) = (formats[id as usize / 2], if id % 2 == 0 { "v2.3" } else { "v2.4" });
            let JobStatus::Done { output } = s else { panic!("{fmt:?} {v}: {s:?}") };
            let bytes = std::fs::read(&output).unwrap();
            let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
            for frame in ["TEXT", "TPE3", "TIT1", "TSOA", "TSOP", "TSOT", "TSO2", "TSOC", "TBPM", "TSRC", "USLT"] {
                if !has(frame.as_bytes()) {
                    wrong.push(format!("{fmt:?} {v}: no {frame} frame"));
                }
            }
            // The placeholder ids lofty is given for the three v2.4-only frames never reach the file.
            for ph in ["TZOA", "TZOP", "TZOT"] {
                if has(ph.as_bytes()) {
                    wrong.push(format!("{fmt:?} {v}: placeholder {ph} left in the file"));
                }
            }
            // A field with no frame of its own stays a custom one.
            if !has(b"CUSTOMFIELD") && !bytes.windows(22).any(|w| w == "CUSTOMFIELD".encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>().as_slice()) {
                wrong.push(format!("{fmt:?} {v}: the custom field is gone"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn totals_and_odd_values_survive_a_conversion_to_mp3_and_m4a() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("totals");
        let src = dir.join("src.flac");
        // Written with the tag library: ffmpeg would rename COMMENT and drop what it does not know.
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "TITLE=T", "-metadata", "TRACKNUMBER=3", "-metadata", "TRACKTOTAL=10"])
            .args(["-metadata", "DISCNUMBER=1", "-metadata", "DISCTOTAL=2", "-metadata", "BPM=128.5"])
            .args(["-metadata", "LANGUAGE=eng"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let r = Runner::new(ff.clone());
        for (i, fmt) in [Format::Mp3, Format::Aac].iter().enumerate() {
            r.start(
                vec![JobSpec {
                    id: i as u64,
                    input: src.clone(),
                    source_root: None,
                    preset: Preset::new("t", *fmt),
                    cleanup: TagCleanup::default(),
                    destination: Destination { folder: dir.join(format!("o{i}")).to_string_lossy().into_owned(), ..Destination::new("t", "") },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                }],
                2,
            );
        }
        wait_idle(&r);
        let mut outputs = Vec::new();
        for (id, s) in r.snapshot() {
            let JobStatus::Done { output } = s else { panic!("{s:?}") };
            outputs.push((id, std::fs::read(output).unwrap()));
        }
        outputs.sort_by_key(|(id, _)| *id);
        let has = |bytes: &[u8], needle: &str| {
            let wide: Vec<u8> = needle.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) || bytes.windows(wide.len()).any(|w| w == wide.as_slice())
        };
        // MP3: the number carries its total, and no custom total frame is left.
        let (mp3, m4a) = (&outputs[0].1, &outputs[1].1);
        assert!(has(mp3, "3/10") && has(mp3, "1/2"), "track and disc totals are not merged in the MP3");
        assert!(!has(mp3, "TRACKTOTAL") && !has(mp3, "DISCTOTAL"), "a custom total frame is left in the MP3");
        // M4A: a BPM that is not a whole number is kept (free-form), and the language is not dropped.
        assert!(has(m4a, "128.5"), "the fractional BPM was lost from the M4A");
        assert!(has(m4a, "LANGUAGE"), "the language was dropped from the M4A");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_converted_folder_keeps_its_sub_folders() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("tree");
        let root = dir.join("Music");
        let deep = root.join("Rock").join("CD 1");
        std::fs::create_dir_all(&deep).unwrap();
        // The generator names its own output, so make the tones in a scratch folder and move them.
        let a = tone(&ff, &dir, "a.wav", 1);
        let b = tone(&ff, &dir, "b.wav", 1);
        std::fs::rename(&a, deep.join("a.wav")).unwrap();
        std::fs::rename(&b, root.join("b.wav")).unwrap();
        let out = dir.join("Out");
        let dest = Destination::new("t", &out.to_string_lossy());
        let mk = |id, input: PathBuf| JobSpec {
            id,
            input,
            source_root: Some(root.clone()),
            preset: Preset::new("t", Format::Mp3),
            cleanup: TagCleanup::default(),
            destination: dest.clone(),
            overwrite: Overwrite::Rename,
            effects: Default::default(),
        };
        let r = Runner::new(ff);
        r.start(vec![mk(1, deep.join("a.wav")), mk(2, root.join("b.wav"))], 2);
        wait_idle(&r);
        for (_, s) in r.snapshot() {
            assert!(matches!(s, JobStatus::Done { .. }), "{s:?}");
        }
        assert!(out.join("Music").join("Rock").join("CD 1").join("a.mp3").exists());
        assert!(out.join("Music").join("b.mp3").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tags_survive_the_conversion() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("tags");
        let src = tone(&ff, &dir, "tone.flac", 1);
        let r = Runner::new(ff.clone());
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Mp3),
                cleanup: TagCleanup::default(),
                destination: Destination::source(),
                overwrite: Overwrite::Overwrite,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        let JobStatus::Done { output } = r.snapshot()[0].1.clone() else { panic!("{:?}", r.snapshot()) };
        let p = ffmpeg::probe(&ff, &output).unwrap();
        assert_eq!(p.tags.get("title").map(String::as_str), Some("Test Tone"));
        assert_eq!(p.tags.get("artist").map(String::as_str), Some("Tester"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_existing_output_is_skipped_under_the_skip_policy() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("skip");
        let src = tone(&ff, &dir, "a.wav", 1);
        std::fs::write(dir.join("a.mp3"), b"old").unwrap();
        let r = Runner::new(ff);
        r.start(
            vec![JobSpec {
                id: 1,
                input: src,
                source_root: None,
                preset: Preset::new("t", Format::Mp3),
                cleanup: TagCleanup::default(),
                destination: Destination::source(),
                overwrite: Overwrite::Skip,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        assert!(matches!(r.snapshot()[0].1, JobStatus::Skipped { .. }));
        assert_eq!(std::fs::read(dir.join("a.mp3")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_ffmpeg_cannot_read_fails_with_a_reason() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("bad");
        let bad = dir.join("not-audio.mp3");
        std::fs::write(&bad, b"this is not audio at all").unwrap();
        let r = Runner::new(ff);
        r.start(
            vec![JobSpec {
                id: 1,
                input: bad,
                source_root: None,
                preset: Preset::new("t", Format::Mp3),
                cleanup: TagCleanup::default(),
                destination: Destination::source(),
                overwrite: Overwrite::Overwrite,
                effects: Default::default(),
            }],
            1,
        );
        wait_idle(&r);
        match &r.snapshot()[0].1 {
            JobStatus::Failed { reason } => assert!(!reason.is_empty()),
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn queued_jobs_can_be_cancelled_before_they_start() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("cancel");
        let src = tone(&ff, &dir, "a.wav", 1);
        let mk = |id| JobSpec {
            id,
            input: src.clone(),
            source_root: None,
            preset: Preset::new("t", Format::Flac),
            cleanup: TagCleanup::default(),
            destination: Destination::source(),
            overwrite: Overwrite::Rename,
            effects: Default::default(),
        };
        let r = Runner::new(ff);
        // One worker: job 2 is still queued when it is cancelled.
        r.start(vec![mk(1), mk(2)], 1);
        r.cancel(2);
        wait_idle(&r);
        let snap = r.snapshot();
        assert!(matches!(snap[1].1, JobStatus::Cancelled), "{snap:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tone of `sound` seconds followed by `tail` seconds of digital silence, 44.1 kHz mono WAV.
    fn tone_with_tail(ff: &Path, dir: &Path, name: &str, sound: u32, tail: f64) -> PathBuf {
        let out = dir.join(name);
        let ok = ffmpeg::command(ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:duration={sound}"))
            .args(["-af"])
            .arg(format!("apad=pad_dur={tail}"))
            .args(["-t"])
            .arg(format!("{}", sound as f64 + tail))
            .args(["-metadata", "title=Tail", "-metadata", "artist=Tester"])
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok, "could not generate the tone with a tail");
        out
    }

    /// What the effects' own measuring pass says about a file - the same numbers the product uses.
    fn measure(ff: &Path, file: &Path) -> crate::effects::Analysis {
        let probe = ffmpeg::probe(ff, file).unwrap();
        let out = ffmpeg::command(ff)
            .args(["-hide_banner", "-nostdin", "-nostats", "-loglevel", "info", "-i"])
            .arg(file)
            .args(["-map", "0:a:0", "-af"])
            .arg(format!("{},{}", crate::effects::scan_filter(-60.0), crate::effects::METER_FILTER))
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        crate::effects::parse_analysis(&String::from_utf8_lossy(&out.stderr), probe.duration_us as f64 / 1e6)
    }

    fn effects_job(src: &Path, out: &Path, format: Format, effects: crate::effects::EffectsEntry, cleanup: TagCleanup) -> JobSpec {
        JobSpec {
            id: 1,
            input: src.to_path_buf(),
            source_root: None,
            preset: Preset::new("t", format),
            cleanup,
            destination: Destination { folder: out.to_string_lossy().into_owned(), pattern: "out".into(), ..Destination::new("t", "") },
            overwrite: Overwrite::Rename,
            effects,
        }
    }

    fn run_one(ff: PathBuf, job: JobSpec) -> PathBuf {
        let r = Runner::new(ff);
        r.start(vec![job], 1);
        wait_idle(&r);
        match r.snapshot().remove(0).1 {
            JobStatus::Done { output } => output,
            other => panic!("{other:?}"),
        }
    }

    /// Every id of a WAV's `LIST INFO` chunk with its text.
    fn riff_info(path: &Path) -> Vec<(String, String)> {
        let b = std::fs::read(path).unwrap();
        let mut out = Vec::new();
        let mut at = 12;
        while at + 8 <= b.len() {
            let id = &b[at..at + 4];
            let size = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
            let body = &b[at + 8..(at + 8 + size).min(b.len())];
            if id == b"LIST" && body.starts_with(b"INFO") {
                let mut p = 4;
                while p + 8 <= body.len() {
                    let n = u32::from_le_bytes([body[p + 4], body[p + 5], body[p + 6], body[p + 7]]) as usize;
                    let v = &body[p + 8..(p + 8 + n).min(body.len())];
                    out.push((
                        String::from_utf8_lossy(&body[p..p + 4]).into_owned(),
                        String::from_utf8_lossy(v).trim_end_matches('\0').to_owned(),
                    ));
                    p += 8 + n + (n & 1);
                }
            }
            at += 8 + size + (size & 1);
        }
        out
    }

    /// Every row of the mapping table that names a RIFF INFO id: a file holding the field reaches a
    /// WAV with that id, carrying the value. The ids are what ffmpeg itself writes (measured 2026-10-15).
    #[test]
    fn each_riff_info_id_of_the_mapping_is_what_a_wav_gets() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("riff-ids");
        let rows: Vec<_> = crate::mapping::FIELDS.iter().filter(|f| f.riff.is_some()).collect();
        assert_eq!(rows.len(), 9, "the mapping names {} INFO ids", rows.len());
        for f in rows {
            let key = match f.name {
                "YEAR" => "date",
                "TRACK" => "track",
                n => n,
            };
            let value = match f.name {
                "YEAR" => "2024",
                "TRACK" => "7",
                _ => "Value-of-field",
            };
            let src = dir.join(format!("{}.flac", f.name));
            let ok = ffmpeg::command(&ff)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
                .arg("-metadata")
                .arg(format!("{key}={value}"))
                .arg(&src)
                .status()
                .unwrap()
                .success();
            assert!(ok, "could not make a source holding {key}");
            let out = run_one(
                ff.clone(),
                JobSpec {
                    id: 1,
                    input: src,
                    source_root: None,
                    preset: Preset::new("t", Format::Wav),
                    cleanup: TagCleanup::default(),
                    destination: Destination {
                        folder: dir.join(format!("o-{}", f.name)).to_string_lossy().into_owned(),
                        ..Destination::new("t", "")
                    },
                    overwrite: Overwrite::Rename,
                    effects: Default::default(),
                },
            );
            let id = f.riff.unwrap();
            let got = riff_info(&out);
            let hit = got.iter().find(|(i, _)| i == id).unwrap_or_else(|| panic!("{}: no {id} in {got:?}", f.name));
            assert!(hit.1.contains(value), "{}: {id} holds {:?}", f.name, hit.1);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_silence_at_the_end_is_exactly_the_wanted_length() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("silence-end");
        let mut e = crate::effects::EffectsEntry::new("t");
        e.add(crate::effects::Kind::SilenceEnd);
        // A long tail is cut down to 2 s, a short tail and none at all are brought up to it.
        for (name, tail) in [("long.wav", 3.5), ("short.wav", 0.3), ("none.wav", 0.0)] {
            let src = tone_with_tail(&ff, &dir, name, 3, tail);
            let out = dir.join(format!("out-{name}"));
            let file = run_one(ff.clone(), effects_job(&src, &out, Format::Flac, e.clone(), TagCleanup::default()));
            let a = measure(&ff, &file);
            let song_end = a.song_end_s.unwrap_or(a.duration_s);
            let silence = a.duration_s - song_end;
            assert!((silence - 2.0).abs() < 0.12, "{name}: {silence:.3} s of silence after the song ({a:?})");
            assert!((song_end - 3.0).abs() < 0.12, "{name}: the song itself must be left whole ({a:?})");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn normalising_reaches_the_target_without_clipping() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("normalise");
        let src = tone(&ff, &dir, "quiet.wav", 4);
        let before = measure(&ff, &src).loudness_lufs.unwrap();
        let mut e = crate::effects::EffectsEntry::new("t");
        e.add(crate::effects::Kind::Volume);
        let out = dir.join("out");
        let file = run_one(ff.clone(), effects_job(&src, &out, Format::Flac, e, TagCleanup::default()));
        let after = measure(&ff, &file);
        assert!(before < -19.0, "the fixture is meant to be quiet: {before}");
        assert!((after.loudness_lufs.unwrap() + 16.0).abs() < 0.6, "{before} LUFS became {:?}", after.loudness_lufs);
        assert!(after.true_peak_db.unwrap() <= -0.9, "the limiter keeps the peak under the ceiling: {:?}", after.true_peak_db);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replaygain_tags_cannot_be_removed_by_the_tags_entry() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("replaygain");
        let src = tone(&ff, &dir, "quiet.wav", 3);
        let mut e = crate::effects::EffectsEntry::new("t");
        e.add(crate::effects::Kind::ReplayGain);
        // The strictest cleaning there is: delete everything.
        let mut cleanup = TagCleanup::new("none");
        cleanup.mode = CleanupMode::Delete;
        let mut wrong = Vec::new();
        for format in [Format::Flac, Format::Mp3, Format::Aac, Format::Opus, Format::Wav, Format::Wma, Format::Aiff, Format::WavPack] {
            let out = dir.join(format!("out-{format:?}"));
            let file = run_one(ff.clone(), effects_job(&src, &out, format, e.clone(), cleanup.clone()));
            let tags: HashMap<String, String> = if format == Format::Wma {
                crate::asf::attributes(&file).unwrap().into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect()
            } else {
                describe(&ff, &file).0.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect()
            };
            match tags.get("replaygain_track_gain") {
                Some(v) if v.trim_end().ends_with("dB") && v.trim_start().starts_with(['+', '-']) => {}
                other => wrong.push(format!("{format:?}: gain {other:?} in {tags:?}")),
            }
            if !tags.contains_key("replaygain_track_peak") {
                wrong.push(format!("{format:?}: no peak in {tags:?}"));
            }
            if tags.contains_key("title") {
                wrong.push(format!("{format:?}: the Tags entry was meant to delete the title"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_job_is_seen_analysing_and_then_converting() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("analysing");
        // Long and low-rate: a measuring pass that lasts long enough to be caught, a small file.
        let src = dir.join("long.wav");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1200:sample_rate=8000"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let mut e = crate::effects::EffectsEntry::new("t");
        e.add(crate::effects::Kind::SilenceEnd);
        e.add(crate::effects::Kind::Volume);
        let out = dir.join("out");
        let r = Runner::new(ff);
        r.start(vec![effects_job(&src, &out, Format::Flac, e, TagCleanup::default())], 1);
        let (mut analysing, mut running, mut last) = (false, false, JobStatus::Queued);
        let t = Instant::now();
        while r.busy() {
            assert!(t.elapsed() < Duration::from_secs(120), "queue did not finish");
            if let Some((_, s)) = r.snapshot().into_iter().next() {
                match s {
                    JobStatus::Analysing { .. } => analysing = true,
                    JobStatus::Running { .. } => running = true,
                    _ => {}
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        if let Some((_, s)) = r.snapshot().into_iter().next() {
            last = s;
        }
        assert!(analysing, "the Analysing state was never seen");
        assert!(running, "the conversion's own state was never seen");
        assert!(matches!(last, JobStatus::Done { .. }), "{last:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn original_keeps_the_format_of_each_file() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("original");
        let wav = tone(&ff, &dir, "a.wav", 2);
        // The same tone in other formats, made by ffmpeg itself.
        let mut sources = vec![("wav", wav.clone())];
        for (ext, extra) in [("flac", vec![]), ("mp3", vec!["-b:a", "128k"]), ("ogg", vec![]), ("m4a", vec!["-c:a", "aac"]), ("opus", vec![])] {
            let p = dir.join(format!("a.{ext}"));
            assert!(ffmpeg::command(&ff)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(&wav)
                .args(&extra)
                .arg(&p)
                .status()
                .unwrap()
                .success(), "{ext}");
            sources.push((ext, p));
        }
        let out = dir.join("out");
        let r = Runner::new(ff);
        let jobs: Vec<JobSpec> = sources
            .iter()
            .enumerate()
            .map(|(i, (_, p))| JobSpec {
                id: i as u64,
                input: p.clone(),
                source_root: None,
                preset: Preset::original(),
                cleanup: TagCleanup::default(),
                destination: Destination { folder: out.join(i.to_string()).to_string_lossy().into_owned(), pattern: "out".into(), ..Destination::new("t", "") },
                overwrite: Overwrite::Rename,
                effects: Default::default(),
            })
            .collect();
        r.start(jobs, 2);
        wait_idle(&r);
        for (id, status) in r.snapshot() {
            let want = sources[id as usize].0;
            match status {
                JobStatus::Done { output } => assert_eq!(output.extension().unwrap(), want, "{output:?}"),
                other => panic!("{want}: {other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn original_refuses_a_codec_no_output_format_keeps() {
        assert_eq!(crate::format::Format::keeping("flac", "flac"), Some(Format::Flac));
        assert_eq!(crate::format::Format::keeping("flac", "oga"), Some(Format::OggFlac));
        assert_eq!(crate::format::Format::keeping("aac", "m4a"), Some(Format::Aac));
        assert_eq!(crate::format::Format::keeping("aac", "aac"), Some(Format::AacAdts));
        assert_eq!(crate::format::Format::keeping("pcm_s24le", "aiff"), Some(Format::Aiff));
        assert_eq!(crate::format::Format::keeping("pcm_s16le", "wav"), Some(Format::Wav));
        assert_eq!(crate::format::Format::keeping("ape", "ape"), None);
        assert_eq!(crate::format::Format::keeping("", "mp3"), None);
    }

    fn entry_of(items: Vec<crate::effects::Effect>) -> crate::effects::EffectsEntry {
        crate::effects::EffectsEntry { items, ..crate::effects::EffectsEntry::new("t") }.normalized()
    }

    /// The highest sample (dBFS) of `len` seconds of a file starting at `at`.
    fn peak_of(ff: &Path, file: &Path, at: f64, len: f64) -> f64 {
        let out = ffmpeg::command(ff)
            .args(["-hide_banner", "-nostdin", "-ss"])
            .arg(format!("{at}"))
            .args(["-t"])
            .arg(format!("{len}"))
            .args(["-i"])
            .arg(file)
            .args(["-af", "volumedetect", "-f", "null", "-"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stderr).into_owned();
        crate::effects::parse_analysis(&text, 0.0).max_volume_db.unwrap_or(f64::NEG_INFINITY)
    }

    fn length_of(ff: &Path, file: &Path) -> f64 {
        ffmpeg::probe(ff, file).unwrap().duration_us as f64 / 1e6
    }

    #[test]
    fn trimming_silence_takes_the_start_and_the_end_but_not_the_middle() {
        use crate::effects::{Effect, TrimSilence};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("trim");
        // 2 s of silence, 1.5 s of tone, 1 s of silence, 1.5 s of tone, 2 s of silence.
        let src = dir.join("src.wav");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1.5"])
            .args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono:d=1"])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1.5"])
            .args(["-filter_complex", "[0:a][1:a][2:a]concat=n=3:v=0:a=1,adelay=2000,apad=pad_dur=2"])
            .args(["-t", "8", "-ar", "44100", "-ac", "1"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let before = length_of(&ff, &src);
        let out = dir.join("out");
        let e = entry_of(vec![Effect::TrimSilence(TrimSilence::default())]);
        let file = run_one(ff.clone(), effects_job(&src, &out, Format::Flac, e, TagCleanup::default()));
        let after = length_of(&ff, &file);
        // 4 s of sound with the silence between kept, plus two margins of 50 ms.
        assert!((after - 4.1).abs() < 0.15, "{before} s became {after} s");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_range_and_a_limit_keep_exactly_what_they_say() {
        use crate::effects::{CutRange, Effect, LimitDuration};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("range");
        let src = tone(&ff, &dir, "ten.wav", 10);
        let out = dir.join("out");
        let range = entry_of(vec![Effect::CutRange(CutRange { from_s: 2.0, to_s: 6.0 })]);
        let a = run_one(ff.clone(), effects_job(&src, &out.join("a"), Format::Flac, range, TagCleanup::default()));
        assert!((length_of(&ff, &a) - 4.0).abs() < 0.1, "{}", length_of(&ff, &a));
        let limit = entry_of(vec![Effect::LimitDuration(LimitDuration { seconds: 3.0 })]);
        let b = run_one(ff.clone(), effects_job(&src, &out.join("b"), Format::Flac, limit, TagCleanup::default()));
        assert!((length_of(&ff, &b) - 3.0).abs() < 0.1, "{}", length_of(&ff, &b));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_fades_open_and_close_the_sound() {
        use crate::effects::{Effect, Fade};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("fades");
        let src = tone(&ff, &dir, "tone.wav", 5);
        let out = dir.join("out");
        let e = entry_of(vec![Effect::FadeIn(Fade { ms: 1500 }), Effect::FadeOut(Fade { ms: 1500 })]);
        let file = run_one(ff.clone(), effects_job(&src, &out, Format::Flac, e, TagCleanup::default()));
        let (start, middle, end) = (peak_of(&ff, &file, 0.0, 0.15), peak_of(&ff, &file, 2.0, 1.0), peak_of(&ff, &file, 4.85, 0.15));
        assert!(start < middle - 10.0, "start {start} dB, middle {middle} dB");
        assert!(end < middle - 10.0, "end {end} dB, middle {middle} dB");
        assert!((length_of(&ff, &file) - 5.0).abs() < 0.1, "a fade does not change the length");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_volume_modes_reach_what_they_promise() {
        use crate::effects::{Effect, Volume, VolumeMode};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("volume");
        let src = tone(&ff, &dir, "tone.wav", 4);
        let before = peak_of(&ff, &src, 0.0, 4.0);
        let out = dir.join("out");
        let run = |name: &str, v: Volume| {
            let e = entry_of(vec![Effect::Volume(v)]);
            run_one(ff.clone(), effects_job(&src, &out.join(name), Format::Flac, e, TagCleanup::default()))
        };
        // A fixed gain: +6 dB.
        let g = run("gain", Volume { mode: VolumeMode::Gain, gain_db: 6.0, ..Volume::default() });
        assert!((peak_of(&ff, &g, 0.0, 4.0) - before - 6.0).abs() < 0.3, "gain");
        // The peak lands on its target.
        let p = run("peak", Volume { mode: VolumeMode::Peak, target_peak_db: -3.0, ..Volume::default() });
        assert!((peak_of(&ff, &p, 0.0, 4.0) + 3.0).abs() < 0.3, "peak {}", peak_of(&ff, &p, 0.0, 4.0));
        // ReplayGain applied: the filter, run on the result, has nothing left to ask for. (Its estimate
        // is its own, not exactly -18 LUFS, so it is asked, not the meter.)
        let r = run("rg", Volume { mode: VolumeMode::ReplayGain, ..Volume::default() });
        let left = measure(&ff, &r).rg_gain_db.unwrap();
        assert!(left.abs() < 0.5, "{left} dB left to apply");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_album_shares_one_gain_and_each_track_keeps_its_own() {
        use crate::effects::{Effect, ReplayGainTags};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("album");
        let make = |name: &str, volume: &str, album: &str| {
            let p = dir.join(name);
            assert!(ffmpeg::command(&ff)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
                .args(["-af", volume, "-metadata"])
                .arg(format!("album={album}"))
                .args(["-metadata", "title=T"])
                .arg(&p)
                .status()
                .unwrap()
                .success());
            p
        };
        let quiet = make("quiet.flac", "volume=-6dB", "Alb");
        let loud = make("loud.flac", "volume=6dB", "Alb");
        let other = make("other.flac", "volume=0dB", "Another");
        let e = entry_of(vec![Effect::ReplayGain(ReplayGainTags { album: true })]);
        let out = dir.join("out");
        let r = Runner::new(ff.clone());
        let jobs: Vec<JobSpec> = [&quiet, &loud, &other]
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut j = effects_job(p, &out.join(i.to_string()), Format::Flac, e.clone(), TagCleanup::default());
                j.id = i as u64;
                j
            })
            .collect();
        r.start(jobs, 3);
        wait_idle(&r);
        let tags_of = |id: u64| -> HashMap<String, String> {
            let (_, st) = r.snapshot().into_iter().find(|(i, _)| *i == id).unwrap();
            let JobStatus::Done { output } = st else { panic!("{id}: {st:?}") };
            describe(&ff, &output).0.into_iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect()
        };
        let num = |t: &HashMap<String, String>, k: &str| -> f64 {
            t.get(k).unwrap_or_else(|| panic!("no {k} in {t:?}")).trim_end_matches(" dB").trim().parse().unwrap()
        };
        let (q, l, o) = (tags_of(0), tags_of(1), tags_of(2));
        // Two tracks of one album: the same album gain, different track gains 12 dB apart.
        assert!((num(&q, "replaygain_album_gain") - num(&l, "replaygain_album_gain")).abs() < 0.05, "{q:?} {l:?}");
        assert!((num(&q, "replaygain_track_gain") - num(&l, "replaygain_track_gain") - 12.0).abs() < 0.6);
        // A different album in the same folder is its own: for one track the album gain is its own.
        assert!((num(&o, "replaygain_album_gain") - num(&o, "replaygain_track_gain")).abs() < 0.6, "{o:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tone_at(ff: &Path, dir: &Path, name: &str, hz: u32, seconds: u32) -> PathBuf {
        let out = dir.join(name);
        assert!(ffmpeg::command(ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency={hz}:duration={seconds}"))
            .args(["-ac", "2"])
            .arg(&out)
            .status()
            .unwrap()
            .success());
        out
    }

    /// The highest sample (dBFS) of one channel of a file.
    fn channel_peak(ff: &Path, file: &Path, channel: u8) -> f64 {
        let out = ffmpeg::command(ff)
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(file)
            .args(["-af"])
            .arg(format!("aformat=channel_layouts=stereo,pan=mono|c0=c{channel},volumedetect"))
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stderr).into_owned();
        crate::effects::parse_analysis(&text, 0.0).max_volume_db.unwrap_or(f64::NEG_INFINITY)
    }

    fn convert_with(ff: &Path, src: &Path, out: &Path, items: Vec<crate::effects::Effect>) -> PathBuf {
        run_one(ff.to_path_buf(), effects_job(src, out, Format::Flac, entry_of(items), TagCleanup::default()))
    }

    #[test]
    fn the_channels_effect_swaps_picks_and_mixes() {
        use crate::effects::{Channels, ChannelsMode, Effect};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("channels");
        // A tone on the LEFT only.
        let src = dir.join("left.wav");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
            .args(["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono"])
            .args(["-filter_complex", "[0:a][1:a]join=inputs=2:channel_layout=stereo", "-t", "2"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        assert!(channel_peak(&ff, &src, 0) > -30.0 && channel_peak(&ff, &src, 1) < -60.0);
        let out = dir.join("out");
        let swapped = convert_with(&ff, &src, &out.join("swap"), vec![Effect::Channels(Channels { mode: ChannelsMode::Swap })]);
        assert!(channel_peak(&ff, &swapped, 0) < -60.0 && channel_peak(&ff, &swapped, 1) > -30.0, "swap");
        let right = convert_with(&ff, &src, &out.join("right"), vec![Effect::Channels(Channels { mode: ChannelsMode::Right })]);
        assert!(channel_peak(&ff, &right, 0) < -60.0 && channel_peak(&ff, &right, 1) < -60.0, "the right channel is silent, on both sides");
        let left = convert_with(&ff, &src, &out.join("left"), vec![Effect::Channels(Channels { mode: ChannelsMode::Left })]);
        assert!(channel_peak(&ff, &left, 0) > -30.0 && channel_peak(&ff, &left, 1) > -30.0, "left on both sides");
        let mono = convert_with(&ff, &src, &out.join("mono"), vec![Effect::Channels(Channels { mode: ChannelsMode::Mono })]);
        let m = channel_peak(&ff, &mono, 0);
        assert!(m > -30.0 && m < channel_peak(&ff, &src, 0) - 3.0, "a mix of a tone and silence is half as loud: {m}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_tempo_changes_the_length_and_the_silence_follows_it() {
        use crate::effects::{Effect, SilenceEnd, Tempo};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("tempo");
        let src = tone_at(&ff, &dir, "ten.wav", 440, 10);
        let out = dir.join("out");
        let fast = convert_with(&ff, &src, &out.join("fast"), vec![Effect::Tempo(Tempo { percent: 100 })]);
        assert!((length_of(&ff, &fast) - 5.0).abs() < 0.15, "{}", length_of(&ff, &fast));
        let slow = convert_with(&ff, &src, &out.join("slow"), vec![Effect::Tempo(Tempo { percent: -50 })]);
        assert!((length_of(&ff, &slow) - 20.0).abs() < 0.3, "{}", length_of(&ff, &slow));
        // With the silence at the end: 5 s of sound, then 2 s.
        let both = convert_with(
            &ff,
            &src,
            &out.join("both"),
            vec![Effect::Tempo(Tempo { percent: 100 }), Effect::SilenceEnd(SilenceEnd::default())],
        );
        assert!((length_of(&ff, &both) - 7.0).abs() < 0.2, "{}", length_of(&ff, &both));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_filters_of_tone_shaping_do_what_their_names_say() {
        use crate::effects::{Dynamics, DynamicsMode, Effect, Equalizer, LowCut, Tone};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("shape");
        let low = tone_at(&ff, &dir, "p50.wav", 50, 3);
        let mid = tone_at(&ff, &dir, "p440.wav", 440, 3);
        let out = dir.join("out");
        // A low cut at 1 kHz takes a 50 Hz tone away.
        let cut = convert_with(&ff, &low, &out.join("cut"), vec![Effect::LowCut(LowCut { hz: 1000 })]);
        assert!(peak_of(&ff, &cut, 1.0, 1.0) < peak_of(&ff, &low, 1.0, 1.0) - 20.0, "low cut");
        // +12 dB of bass lifts a 50 Hz tone.
        let bass = convert_with(&ff, &low, &out.join("bass"), vec![Effect::Tone(Tone { bass_db: 12, treble_db: 0 })]);
        assert!(peak_of(&ff, &bass, 1.0, 1.0) > peak_of(&ff, &low, 1.0, 1.0) + 6.0, "bass");
        // The 500 Hz band lifts a 440 Hz tone; the 8 kHz band does not.
        let mut up = [0; 10];
        up[4] = 12;
        let eq = convert_with(&ff, &mid, &out.join("eq"), vec![Effect::Equalizer(Equalizer { gains_db: up })]);
        assert!(peak_of(&ff, &eq, 1.0, 1.0) > peak_of(&ff, &mid, 1.0, 1.0) + 6.0, "equaliser");
        let mut far = [0; 10];
        far[8] = 12;
        let eq2 = convert_with(&ff, &mid, &out.join("eq2"), vec![Effect::Equalizer(Equalizer { gains_db: far })]);
        assert!((peak_of(&ff, &eq2, 1.0, 1.0) - peak_of(&ff, &mid, 1.0, 1.0)).abs() < 1.0, "a far band leaves the tone");
        // A compressor with a low threshold lowers the tone.
        let comp = convert_with(
            &ff,
            &mid,
            &out.join("comp"),
            vec![Effect::Dynamics(Dynamics { mode: DynamicsMode::Compressor, threshold_db: -40.0, ratio: 10 })],
        );
        assert!(peak_of(&ff, &comp, 1.0, 1.0) < peak_of(&ff, &mid, 1.0, 1.0) - 4.0, "compressor");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_restoring_effects_run_and_keep_the_length() {
        use crate::effects::{DeEsser, Denoise, Dynamics, DynamicsMode, Effect, Repair};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("restore");
        let src = tone_at(&ff, &dir, "tone.wav", 440, 3);
        let out = dir.join("out");
        let cases: Vec<(&str, Vec<Effect>)> = vec![
            ("repair", vec![Effect::Repair(Repair::default())]),
            ("denoise", vec![Effect::Denoise(Denoise::default())]),
            ("deesser", vec![Effect::DeEsser(DeEsser::default())]),
            ("normaliser", vec![Effect::Dynamics(Dynamics { mode: DynamicsMode::Normaliser, ..Dynamics::default() })]),
        ];
        for (name, items) in cases {
            let file = convert_with(&ff, &src, &out.join(name), items);
            assert!((length_of(&ff, &file) - 3.0).abs() < 0.2, "{name}: {}", length_of(&ff, &file));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_volume_is_set_from_what_the_processing_leaves() {
        use crate::effects::{Effect, LowCut, Volume};
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("measured-after");
        // A loud 50 Hz hum and a quiet 2 kHz tone. The low cut takes the hum away, so what is left is
        // the quiet tone, and the volume must be set from THAT: measured on the source (the hum
        // dominates) the track would come out about 20 dB too quiet.
        let src = dir.join("mix.wav");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=50:duration=4"])
            .args(["-f", "lavfi", "-i", "sine=frequency=2000:duration=4"])
            .args(["-filter_complex", "[1:a]volume=-20dB[b];[0:a][b]amix=inputs=2:normalize=0"])
            .args(["-ac", "2"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let out = dir.join("out");
        let file = convert_with(&ff, &src, &out, vec![Effect::LowCut(LowCut { hz: 1000 }), Effect::Volume(Volume::default())]);
        let after = measure(&ff, &file).loudness_lufs.unwrap();
        assert!((after + 16.0).abs() < 1.0, "{after} LUFS");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dither_makes_a_16_bit_output_from_a_24_bit_source_and_changes_the_bits() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("dither");
        let src = dir.join("src24.wav");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
            .args(["-af", "volume=-40dB", "-c:a", "pcm_s24le"])
            .arg(&src)
            .status()
            .unwrap()
            .success());
        let mut results = Vec::new();
        for (name, dither) in [("plain", false), ("dithered", true)] {
            let mut j = effects_job(&src, &dir.join(name), Format::Wav, Default::default(), TagCleanup::default());
            j.preset.bit_depth = Some(16);
            j.preset.dither = dither;
            assert_eq!(j.preset.dither_filter().is_some(), dither);
            let file = run_one(ff.clone(), j);
            let info = ffmpeg::command(&ff).args(["-hide_banner", "-nostdin", "-i"]).arg(&file).output().unwrap();
            assert!(String::from_utf8_lossy(&info.stderr).contains("pcm_s16le"), "{name} is 16 bit");
            results.push(std::fs::read(&file).unwrap());
        }
        assert_ne!(results[0], results[1], "the dither changed the samples");
        // A 24-bit output never dithers, whatever the switch says.
        let mut p = Preset::new("t", Format::Wav);
        p.bit_depth = Some(24);
        p.dither = true;
        assert!(p.normalized().dither_filter().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// What a file's tags say, in the forms two files can be compared by.
    #[derive(Default, Debug)]
    struct Seen {
        /// Every text value ffmpeg reports.
        values: std::collections::BTreeSet<String>,
        /// "3/10": the track number and its total, whichever way the format holds them.
        track: Option<String>,
        disc: Option<String>,
        /// The year, four digits (an ID3v2.3 date is two frames).
        year: Option<String>,
        cover: bool,
    }

    fn seen(ff: &Path, file: &Path, format: Format) -> Seen {
        let _ = format;
        let tags = ffmpeg::probe(ff, file).unwrap().tags;
        // Every key ffmpeg holds, the language included (its banner leaves that one out).
        let meta = ffmpeg::command(ff)
            .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
            .arg(file)
            .args(["-f", "ffmetadata", "-"])
            .output()
            .unwrap();
        let held: Vec<String> = String::from_utf8_lossy(&meta.stdout)
            .lines()
            .filter_map(|l| l.split_once('=').map(|(_, v)| v.trim().to_owned()))
            .collect();
        // An Ogg keeps the language as the name of its stream: `Stream #0:0(eng)` in the banner.
        let banner = ffmpeg::command(ff).args(["-hide_banner", "-nostdin", "-i"]).arg(file).output().unwrap();
        let streams: Vec<String> = String::from_utf8_lossy(&banner.stderr)
            .lines()
            .filter_map(|l| {
                let rest = l.trim().strip_prefix("Stream #")?;
                let open = rest.find('(')?;
                let close = rest[open..].find(')')? + open;
                (rest[..open].chars().all(|c| c.is_ascii_digit() || c == ':')).then(|| rest[open + 1..close].to_owned())
            })
            .collect();
        let held: Vec<String> = held.into_iter().chain(streams).collect();
        // What ffmpeg's listing leaves out (the language, an M4A's conductor/publisher/BPM) is read the
        // way the app reads it, so the check is of the files, not of ffmpeg's banner.
        let extra: Vec<String> = crate::tags::source_only_fields(file, &tags).into_iter().map(|(_, v)| v).collect();
        let get = |keys: &[&str]| keys.iter().find_map(|k| tags.get(*k)).map(|v| v.trim().to_owned());
        let pair = |n: Option<String>, total: Option<String>| {
            n.filter(|n| !n.is_empty()).map(|n| {
                let (num, inline) = match n.split_once('/') {
                    Some((a, b)) => (a.trim().to_owned(), Some(b.trim().to_owned())),
                    None => (n.clone(), None),
                };
                format!("{num}/{}", inline.or(total).unwrap_or_default())
            })
        };
        // A picture is a video stream to ffmpeg, whichever way the format stores it (a block, a frame,
        // an atom, a library record) - the same eye for all eighteen formats.
        let info = ffmpeg::command(ff).args(["-hide_banner", "-nostdin", "-i"]).arg(file).output().unwrap();
        let cover = String::from_utf8_lossy(&info.stderr).contains("Video:");
        Seen {
            track: pair(get(&["track", "tracknumber"]), get(&["tracktotal", "totaltracks"])),
            disc: pair(get(&["disc", "discnumber"]), get(&["disctotal", "totaldiscs"])),
            year: get(&["date", "year"]).map(|d| d.chars().take(4).collect()).filter(|y: &String| y.len() == 4),
            cover,
            values: tags.into_values().chain(extra).chain(held).collect(),
        }
    }

    /// The eighteen output formats are each other's sources and targets. A value both the source and
    /// the target can hold - text, the track and disc with their totals, the year, two album artists,
    /// the cover - must come through every one of the 18 x 18 pairs.
    #[test]
    fn a_field_survives_from_any_format_to_any_format() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("any-to-any");
        // Every row of the mapping table, each with a value of its own, plus the numbers, the date and
        // the fields that have no row (a free-form one the user made up).
        let mut fields: Vec<(String, String)> = Vec::new();
        let special = ["TRACK", "YEAR", "DISCNUMBER", "BPM", "COMPILATION", "ORIGYEAR", "ENCODERSETTINGS"];
        for fd in crate::mapping::FIELDS {
            if !special.contains(&fd.name) {
                fields.push((fd.name.to_owned(), format!("v-{}", fd.name.to_ascii_lowercase())));
            }
        }
        for (k, v) in [
            ("BPM", "121"), ("ORIGYEAR", "1999"), ("TRACKNUMBER", "3"), ("TRACKTOTAL", "10"), ("DISCNUMBER", "2"),
            ("DISCTOTAL", "5"), ("DATE", "2025-11-24"), ("RELEASETYPE", "v-releasetype"), ("ORGANIZATION", "v-organization"),
            ("SUBGENRE", "v-subgenre"), ("MYOWNFIELD", "v-myownfield"),
        ] {
            fields.push((k.to_owned(), v.to_owned()));
        }
        let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        // A cover picture to carry.
        let jpg = dir.join("cover.jpg");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=64x64", "-frames:v", "1"])
            .arg(&jpg)
            .status()
            .unwrap()
            .success());
        let flac = dir.join("source.flac");
        let mut c = ffmpeg::command(&ff);
        c.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-i"]);
        c.arg(&jpg).args(["-map", "0:a", "-map", "1:v", "-c:v", "mjpeg", "-disposition:v", "attached_pic"]);
        for (k, v) in fields {
            c.args(["-metadata", &format!("{k}={v}")]);
        }
        assert!(c.arg(&flac).status().unwrap().success());
        // Two album artists: a repeated field, which ffmpeg cannot write - the tag library does.
        {
            use lofty::{config::{ParseOptions, WriteOptions}, file::AudioFile, flac::FlacFile};
            let mut fh = std::fs::File::open(&flac).unwrap();
            let mut file = FlacFile::read_from(&mut fh, ParseOptions::new()).unwrap();
            drop(fh);
            file.vorbis_comments_mut().unwrap().push("ALBUMARTIST".to_owned(), "v-albumartist2".to_owned());
            file.save_to_path(&flac, WriteOptions::new()).unwrap();
        }

        let formats = Format::ALL;
        let run = |inputs: Vec<(usize, PathBuf)>, out: &Path| -> Vec<(usize, PathBuf)> {
            let r = Runner::new(ff.clone());
            let mut jobs = Vec::new();
            for (n, (tag, input)) in inputs.iter().enumerate() {
                for (t, fmt) in formats.iter().enumerate() {
                    let mut j = effects_job(input, &out.join(format!("{tag}-{t}")), *fmt, Default::default(), TagCleanup::default());
                    j.id = (n * formats.len() + t) as u64;
                    jobs.push(j);
                }
            }
            r.start(jobs, 8);
            wait_idle(&r);
            let mut done: Vec<(u64, PathBuf)> = r
                .snapshot()
                .into_iter()
                .map(|(id, s)| match s {
                    JobStatus::Done { output } => (id, output),
                    other => panic!("job {id}: {other:?}"),
                })
                .collect();
            done.sort();
            done.into_iter().map(|(id, p)| (id as usize, p)).collect()
        };

        // The FLAC into every format: the sources, and what each format can hold.
        let sources = run(vec![(0, flac.clone())], &dir.join("s"));
        let base: Vec<Seen> = sources.iter().map(|(i, p)| seen(&ff, p, formats[*i])).collect();
        // Every one of those into every format.
        let pairs = run(sources.iter().map(|(i, p)| (*i, p.clone())).collect(), &dir.join("p"));
        let mut lost: Vec<String> = Vec::new();
        for (idx, out) in &pairs {
            let (si, ti) = (idx / formats.len(), idx % formats.len());
            let (s, t) = (&base[si], &base[ti]);
            let got = seen(&ff, out, formats[ti]);
            let pair = format!("{:?} -> {:?}", formats[si], formats[ti]);
            // A target that holds tags holds every field (a custom frame, a free-form atom, an extended
            // attribute, a comment): nothing the source has may be missing from it - not only what the
            // app happens to write for a FLAC.
            if !formats[ti].holds_tags() || formats[ti] == Format::Au {
                continue;
            }
            for v in s.values.iter() {
                let plain = (v.starts_with("v-") || v == "121" || v == "1999") && !v.contains("albumartist");
                if plain && !got.values.contains(v) {
                    lost.push(format!("{pair}: {v}"));
                }
            }
            // Two album artists: both names in one value, however the format joins them.
            let both = |x: &Seen| x.values.iter().any(|v| v.contains("v-albumartist") && v.contains("v-albumartist2"));
            if both(s) && both(t) && !both(&got) {
                lost.push(format!("{pair}: the second album artist"));
            }
            // Numbers, the year, the cover.
            if s.track.is_some() && s.track == t.track && got.track != s.track {
                lost.push(format!("{pair}: track {:?} became {:?}", s.track, got.track));
            }
            if s.disc.is_some() && s.disc == t.disc && got.disc != s.disc {
                lost.push(format!("{pair}: disc {:?} became {:?}", s.disc, got.disc));
            }
            if s.year.is_some() && s.year == t.year && got.year != s.year {
                lost.push(format!("{pair}: year {:?} became {:?}", s.year, got.year));
            }
            if s.cover && t.cover && !got.cover {
                lost.push(format!("{pair}: the cover"));
            }
        }
        assert!(lost.is_empty(), "{} thing(s) lost on the way:\n{}", lost.len(), lost.join("\n"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_conductor_stays_a_conductor_and_a_performer_stays_a_performer() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("conductor");
        // An MP3 whose TPE3 frame holds a conductor (ffmpeg's key for TPE3 is `performer`).
        let mp3 = dir.join("c.mp3");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "performer=Karajan", "-metadata", "title=T"])
            .arg(&mp3)
            .status()
            .unwrap()
            .success());
        // A FLAC with a PERFORMER of its own.
        let flac = dir.join("p.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "PERFORMER=Callas", "-metadata", "title=T"])
            .arg(&flac)
            .status()
            .unwrap()
            .success());
        let key_of = |file: &Path, value: &str| -> Option<String> {
            ffmpeg::probe(&ff, file).unwrap().tags.into_iter().find(|(_, v)| v == value).map(|(k, _)| k.to_ascii_lowercase())
        };
        let out = dir.join("out");
        let run = |src: &Path, fmt: Format, name: &str| {
            run_one(ff.clone(), effects_job(src, &out.join(name), fmt, Default::default(), TagCleanup::default()))
        };
        assert_eq!(key_of(&run(&mp3, Format::Flac, "a"), "Karajan").as_deref(), Some("conductor"), "MP3 conductor into FLAC");
        assert_eq!(key_of(&run(&flac, Format::Flac, "b"), "Callas").as_deref(), Some("performer"), "FLAC performer stays");
        assert_eq!(key_of(&run(&mp3, Format::Mp3, "c"), "Karajan").as_deref(), Some("performer"), "MP3 to MP3 is the same TPE3 frame");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_the_container_says_about_itself_is_not_carried_as_a_tag() {
        let Some(ff) = ffmpeg() else { return };
        let dir = scratch("container");
        let flac = dir.join("s.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(["-metadata", "title=T", "-metadata", "BPM=121"])
            .arg(&flac)
            .status()
            .unwrap()
            .success());
        let out = dir.join("out");
        let has = |file: &Path, needle: &str| {
            let b = std::fs::read(file).unwrap();
            let utf16: Vec<u8> = needle.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            b.windows(needle.len()).any(|w| w == needle.as_bytes()) || b.windows(utf16.len()).any(|w| w == utf16.as_slice())
        };
        let run = |src: &Path, fmt: Format, name: &str| run_one(ff.clone(), effects_job(src, &out.join(name), fmt, Default::default(), TagCleanup::default()));
        // M4A to M4A: its brands were written back as free-form atoms (MAJOR_BRAND...).
        let m4a = run(&flac, Format::Aac, "m4a");
        let again = run(&m4a, Format::Aac, "m4a2");
        for junk in ["MAJOR_BRAND", "MINOR_VERSION", "COMPATIBLE_BRANDS"] {
            assert!(!has(&again, junk), "{junk} reached the M4A");
        }
        // WMA to MP3: the 1970 creation time became a custom frame.
        let wma = run(&flac, Format::Wma, "wma");
        let mp3 = run(&wma, Format::Mp3, "mp3");
        assert!(!has(&mp3, "creation_time"), "creation_time reached the MP3");
        // MP3 to FLAC: ffmpeg's lower-case names are written in capitals, like a FLAC made from a FLAC.
        let flac2 = run(&mp3, Format::Flac, "flac");
        assert!(has(&flac2, "TITLE="), "TITLE is in capitals");
        assert!(!has(&flac2, "title="), "no lower-case title");
        let _ = std::fs::remove_dir_all(dir);
    }
}
