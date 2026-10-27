//! Installing the ffmpeg yaiol has approved for its apps. The app never uses an ffmpeg it finds on the
//! machine and never ships one: the builds with x264 and x265 are GPL, and bundling one would put this app
//! under the GPL. It downloads the one build the compatibility manifest names for this system, into the folder
//! every yaiol app shares, and refuses anything whose SHA-256 is not the manifest's.
//!
//! The manifest is `ffmpeg.json` on the yaiol site. It holds one entry per schema (the number of the pair an app understands:
//! an ffmpeg and the effects catalogue written for it); an entry is the version, for each system the address, the checksum
//! and the size of one build, and the same three for the catalogue. Changing the ffmpeg of every app is changing that file,
//! after the apps have been tested against it; no app carries a version of its own, and no app carries a catalogue.
//!
//! The pair goes into `<shared folder>\<schema>`: an app of another age reads its own folder and never meets what it cannot read.

use crate::ffmpeg;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

pub const MANIFEST_URL: &str = "https://apps.yaiol.com/ffmpeg.json";

/// The schema of the pair (ffmpeg + catalogue) this code understands: the entry of the manifest it reads, and the name of its folder.
pub const SCHEMA: u32 = 1;

/// The only layout of the manifest file this code reads (the `schemas` map). A file that says otherwise was written for a newer app.
const LAYOUT: u32 = 1;

/// What the installs leave beside the programs: one entry per pack, saying which build it is, so the folder can say
/// what it holds without running anything (the vendors word their version lines differently). A map by pack name:
/// `{ "ffmpeg": { "version": "9.0.2", "sha256": "..." }, "effects": { "version": "9.0.2", "sha256": "..." } }`.
const MARKER: &str = "installed.json";

/// The packs this module installs, and the name of the catalogue file.
const PACK: &str = "ffmpeg";
const EFFECTS_PACK: &str = "effects";
const EFFECTS_FILE: &str = "effects.json";

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Build {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub source: String,
}

/// The effects catalogue written for the ffmpeg of an entry.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Catalogue {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// The entry of the manifest for this app's schema: an ffmpeg and the catalogue written for it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub builds: HashMap<String, Build>,
    pub effects: Catalogue,
}

#[derive(Deserialize)]
struct ManifestFile {
    manifest: u32,
    schemas: HashMap<String, Manifest>,
}

#[derive(Serialize, Deserialize)]
struct Installed {
    version: String,
    sha256: String,
}

/// The manifest's key for this system: `windows-x86_64`, `linux-aarch64`, `macos-aarch64`...
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let file: ManifestFile = serde_json::from_str(text).map_err(|e| format!("the ffmpeg list could not be read: {e}"))?;
        if file.manifest != LAYOUT {
            return Err("the ffmpeg list was written for a newer version of this app".to_owned());
        }
        let mut schemas = file.schemas;
        schemas.remove(&SCHEMA.to_string()).ok_or_else(|| "the ffmpeg list has nothing for this version of the app".to_owned())
    }

    /// The build for this system, when there is one.
    pub fn build(&self) -> Option<&Build> {
        self.builds.get(&platform())
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// Ask the yaiol site which ffmpeg is the compatible one. Blocking: a network round trip.
pub fn fetch_manifest() -> Result<Manifest, String> {
    let mut resp = agent()
        .get(MANIFEST_URL)
        .header("Cache-Control", "no-cache")
        .call()
        .map_err(|e| format!("cannot reach the ffmpeg list: {e}"))?;
    let text = resp.body_mut().read_to_string().map_err(|e| format!("cannot read the ffmpeg list: {e}"))?;
    Manifest::parse(&text)
}

/// The version of the ffmpeg in `dir`, as the install recorded it. `None` when there is none, when the program is there without
/// a record (one put there by hand: such a copy is not a build we know), or when the catalogue written for it is not there: the
/// two are one install.
pub fn installed_version(dir: &Path) -> Option<String> {
    dir.join(ffmpeg::executable_name()).is_file().then_some(())?;
    dir.join(EFFECTS_FILE).is_file().then_some(())?;
    let mut record = read_marker(dir);
    record.remove(EFFECTS_PACK)?;
    record.remove(PACK).map(|i| i.version)
}

/// The catalogue installed with the ffmpeg of `dir`, when the pair is installed.
pub fn catalogue_path(dir: &Path) -> Option<PathBuf> {
    installed_version(dir).map(|_| dir.join(EFFECTS_FILE))
}

/// `dir` holds exactly the pair the manifest names: the same ffmpeg and the same catalogue.
pub fn is_current(dir: &Path, manifest: &Manifest) -> bool {
    let record = read_marker(dir);
    installed_version(dir).is_some_and(|v| v == manifest.version)
        && record.get(EFFECTS_PACK).is_some_and(|e| e.sha256 == manifest.effects.sha256.to_ascii_lowercase())
}

/// The record of every pack in `dir`; empty when there is none or it cannot be read.
fn read_marker(dir: &Path) -> BTreeMap<String, Installed> {
    fs::read_to_string(dir.join(MARKER)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadState {
    Idle,
    Downloading { done: u64, total: Option<u64> },
    Verifying,
    Unpacking,
    Done(PathBuf),
    Failed(String),
}

impl DownloadState {
    pub fn is_active(&self) -> bool {
        matches!(self, DownloadState::Downloading { .. } | DownloadState::Verifying | DownloadState::Unpacking)
    }
}

#[derive(Clone)]
pub struct Download {
    state: Arc<Mutex<DownloadState>>,
}

impl Default for Download {
    fn default() -> Self {
        Download::new()
    }
}

impl Download {
    pub fn new() -> Download {
        Download { state: Arc::new(Mutex::new(DownloadState::Idle)) }
    }

    pub fn snapshot(&self) -> DownloadState {
        self.state.lock().unwrap().clone()
    }

    /// Start installing the manifest's ffmpeg into `dir`. Does nothing while an install is already running.
    pub fn start(&self, dir: PathBuf) {
        {
            let mut st = self.state.lock().unwrap();
            if st.is_active() {
                return;
            }
            *st = DownloadState::Downloading { done: 0, total: None };
        }
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let set = |s: DownloadState| *state.lock().unwrap() = s;
            match run(&dir, &set) {
                Ok(path) => set(DownloadState::Done(path)),
                Err(e) => set(DownloadState::Failed(e)),
            }
        });
    }
}

/// What the yaiol site says the compatible ffmpeg is, as far as this process has asked.
#[derive(Clone, Debug, PartialEq)]
pub enum LatestState {
    Unknown,
    Checking,
    Known(Manifest),
    Failed(String),
}

/// The question "which ffmpeg is the compatible one?", asked in the background so a window never waits for it.
#[derive(Clone)]
pub struct Latest {
    state: Arc<Mutex<LatestState>>,
}

impl Default for Latest {
    fn default() -> Self {
        Latest::new()
    }
}

impl Latest {
    pub fn new() -> Latest {
        Latest { state: Arc::new(Mutex::new(LatestState::Unknown)) }
    }

    pub fn snapshot(&self) -> LatestState {
        self.state.lock().unwrap().clone()
    }

    /// Ask again. Does nothing while an answer is already on its way.
    pub fn check(&self) {
        {
            let mut st = self.state.lock().unwrap();
            if *st == LatestState::Checking {
                return;
            }
            *st = LatestState::Checking;
        }
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let answer = match fetch_manifest() {
                Ok(m) => LatestState::Known(m),
                Err(e) => LatestState::Failed(e),
            };
            *state.lock().unwrap() = answer;
        });
    }
}

fn run(dir: &Path, set: &dyn Fn(DownloadState)) -> Result<PathBuf, String> {
    let manifest = fetch_manifest()?;
    let build = manifest.build().ok_or_else(|| format!("there is no ffmpeg for this system ({}) yet", platform()))?;
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    // The catalogue first, in memory: it is small, and nothing is replaced until both are here and checked.
    let catalogue = {
        let mut resp = agent().get(&manifest.effects.url).call().map_err(|e| format!("download: {e}"))?;
        let bytes = resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| format!("download: {e}"))?;
        if hex(&Sha256::digest(&bytes)) != manifest.effects.sha256.to_ascii_lowercase() {
            return Err("the effects catalogue does not match the checksum yaiol approved".to_owned());
        }
        bytes
    };

    // Only the catalogue changed: the ffmpeg of the folder is the one the manifest names, so it is not fetched again.
    if ffmpeg_is_installed(dir, &manifest.version, &build.sha256) {
        finish(dir, &catalogue, &manifest, build)?;
        return Ok(dir.join(ffmpeg::executable_name()));
    }

    let zip_path = dir.join("ffmpeg-download.zip");
    let digest = {
        let mut resp = agent().get(&build.url).call().map_err(|e| format!("download: {e}"))?;
        let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).or(Some(build.size));
        let mut reader = resp.body_mut().with_config().limit(1 << 31).reader();
        let mut file = fs::File::create(&zip_path).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        let mut last = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if done - last >= 512 * 1024 {
                last = done;
                set(DownloadState::Downloading { done, total });
            }
        }
        hasher.finalize()
    };
    set(DownloadState::Verifying);
    if hex(&digest) != build.sha256.to_ascii_lowercase() {
        let _ = fs::remove_file(&zip_path);
        return Err("the download does not match the checksum yaiol approved".to_owned());
    }
    set(DownloadState::Unpacking);
    let exe = unpack(&zip_path, dir);
    let _ = fs::remove_file(&zip_path);
    let exe = exe?;
    if ffmpeg::is_nonfree(&exe) != Some(false) {
        let _ = fs::remove_file(&exe);
        return Err("the downloaded ffmpeg did not run, or is a non-free build".to_owned());
    }
    finish(dir, &catalogue, &manifest, build)?;
    Ok(exe)
}

/// The program of `dir` is the build the manifest names (the install recorded this version and checksum, and the file is there).
fn ffmpeg_is_installed(dir: &Path, version: &str, sha256: &str) -> bool {
    dir.join(ffmpeg::executable_name()).is_file() && read_marker(dir).get(PACK).is_some_and(|i| i.version == version && i.sha256 == sha256.to_ascii_lowercase())
}

/// Put the catalogue in place and record both packs, once the program is there.
fn finish(dir: &Path, catalogue: &[u8], manifest: &Manifest, build: &Build) -> Result<(), String> {
    let part = dir.join(format!("{EFFECTS_FILE}.part"));
    fs::write(&part, catalogue).map_err(|e| e.to_string())?;
    fs::rename(&part, dir.join(EFFECTS_FILE)).map_err(|e| e.to_string())?;
    // The other packs of the folder keep their entries.
    let mut record = read_marker(dir);
    record.insert(PACK.to_owned(), Installed { version: manifest.version.clone(), sha256: build.sha256.to_ascii_lowercase() });
    record.insert(EFFECTS_PACK.to_owned(), Installed { version: manifest.version.clone(), sha256: manifest.effects.sha256.to_ascii_lowercase() });
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    fs::write(dir.join(MARKER), text).map_err(|e| e.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Take the ffmpeg program out of the archive into `dir`, wherever the archive keeps it, through a `.part`
/// file so a failed copy never leaves a half-written program that looks like a real one. A copy already there
/// is moved aside first, not deleted: Windows lets a running program be renamed but not removed, so an ffmpeg
/// another app is using at that moment does not stop the replacement.
pub fn unpack(zip_path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a zip archive: {e}"))?;
    let name = ffmpeg::executable_name();
    let mut found = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if !entry.is_dir() && entry.name().is_ok_and(|n| n.rsplit('/').next() == Some(name)) {
            found = Some(i);
            break;
        }
    }
    let i = found.ok_or_else(|| format!("{name} is not in the archive"))?;
    let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
    let target = dir.join(name);
    let part = dir.join(format!("{name}.part"));
    {
        let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&part, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let aside = dir.join(format!("{name}.old"));
    let _ = fs::remove_file(&aside);
    let moved = target.exists();
    if moved {
        fs::rename(&target, &aside).map_err(|e| format!("the ffmpeg in use cannot be replaced now: {e}"))?;
    }
    if let Err(e) = fs::rename(&part, &target) {
        if moved {
            let _ = fs::rename(&aside, &target);
        }
        return Err(e.to_string());
    }
    let _ = fs::remove_file(&aside);
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yaiol-vdcv-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SAMPLE: &str = r#"{
        "manifest": 1,
        "schemas": {
            "1": {
                "version": "9.0.2",
                "builds": {
                    "windows-x86_64": { "url": "https://example.test/w.zip", "sha256": "ab", "size": 10, "source": "w" },
                    "linux-x86_64":   { "url": "https://example.test/l.zip", "sha256": "cd", "size": 20, "source": "l" }
                },
                "effects": { "url": "https://example.test/e.json", "sha256": "ef", "size": 30 }
            },
            "2": { "version": "10.0.0", "builds": {}, "effects": { "url": "https://example.test/e2.json", "sha256": "01", "size": 1 } }
        }
    }"#;

    #[test]
    fn the_manifest_is_read_and_gives_the_build_of_this_system() {
        let m = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(m.version, "9.0.2");
        assert_eq!(m.builds.len(), 2);
        assert_eq!(m.effects.sha256, "ef");
        let here = m.build();
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert_eq!(here.unwrap().source, "w");
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(here.unwrap().source, "l");
        } else {
            assert!(here.is_none());
        }
    }

    #[test]
    fn a_manifest_of_another_layout_or_no_manifest_at_all_is_refused() {
        assert!(Manifest::parse(&SAMPLE.replace("\"manifest\": 1", "\"manifest\": 2")).is_err());
        assert!(Manifest::parse(&SAMPLE.replace("\"1\": {", "\"3\": {")).is_err(), "no entry for this schema");
        assert!(Manifest::parse("<html>not found</html>").is_err());
        assert!(Manifest::parse("").is_err());
    }

    #[test]
    fn a_copy_is_a_known_build_only_when_the_install_recorded_it() {
        let dir = scratch("marker");
        assert_eq!(installed_version(&dir), None);
        fs::write(dir.join(ffmpeg::executable_name()), b"x").unwrap();
        assert_eq!(installed_version(&dir), None, "a program put there by hand has no record");
        fs::write(dir.join(MARKER), r#"{"version":"9.0.2","sha256":"ab"}"#).unwrap();
        assert_eq!(installed_version(&dir), None, "the old flat record names no pack");
        fs::write(dir.join(MARKER), r#"{"other":{"version":"1","sha256":"cd"}}"#).unwrap();
        assert_eq!(installed_version(&dir), None, "another pack's record is not ffmpeg's");
        fs::write(dir.join(MARKER), r#"{"ffmpeg":{"version":"9.0.2","sha256":"ab"},"other":{"version":"1","sha256":"cd"}}"#).unwrap();
        assert_eq!(installed_version(&dir), None, "ffmpeg without its catalogue is not an install");
        fs::write(dir.join(MARKER), r#"{"ffmpeg":{"version":"9.0.2","sha256":"ab"},"effects":{"version":"9.0.2","sha256":"ef"}}"#).unwrap();
        assert_eq!(installed_version(&dir), None, "the catalogue record without its file is not an install");
        fs::write(dir.join(EFFECTS_FILE), b"{}").unwrap();
        assert_eq!(installed_version(&dir).as_deref(), Some("9.0.2"));
        assert_eq!(catalogue_path(&dir), Some(dir.join(EFFECTS_FILE)));
        let m = Manifest::parse(SAMPLE).unwrap();
        assert!(is_current(&dir, &m));
        let mut other = m.clone();
        other.effects.sha256 = "99".into();
        assert!(!is_current(&dir, &other), "a new catalogue for the same ffmpeg is an update");
        other = m;
        other.version = "9.0.3".into();
        assert!(!is_current(&dir, &other));
        fs::remove_file(dir.join(ffmpeg::executable_name())).unwrap();
        assert_eq!(installed_version(&dir), None, "a record without the program is nothing");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_catalogue_alone_does_not_fetch_ffmpeg_again() {
        let dir = scratch("keep");
        fs::write(dir.join(ffmpeg::executable_name()), b"x").unwrap();
        fs::write(dir.join(MARKER), r#"{"ffmpeg":{"version":"9.0.2","sha256":"ab"},"effects":{"version":"9.0.2","sha256":"ef"}}"#).unwrap();
        assert!(ffmpeg_is_installed(&dir, "9.0.2", "ab"), "the same build");
        assert!(!ffmpeg_is_installed(&dir, "9.0.3", "ab"), "another version is fetched");
        assert!(!ffmpeg_is_installed(&dir, "9.0.2", "cd"), "another build of the same version is fetched");
        fs::remove_file(dir.join(ffmpeg::executable_name())).unwrap();
        assert!(!ffmpeg_is_installed(&dir, "9.0.2", "ab"), "a record without the program is nothing");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ffmpeg_is_taken_out_of_the_archive_and_nothing_else() {
        let dir = scratch("unpack");
        let zip_path = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("ffmpeg-8.0-essentials_build/bin/ffprobe.exe", o).unwrap();
            w.write_all(b"probe").unwrap();
            w.start_file(format!("ffmpeg-8.0-essentials_build/bin/{}", ffmpeg::executable_name()), o).unwrap();
            w.write_all(b"the program").unwrap();
            w.finish().unwrap();
        }
        let exe = unpack(&zip_path, &dir).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"the program");
        assert!(!dir.join(format!("{}.part", ffmpeg::executable_name())).exists());
        assert!(!dir.join("ffprobe.exe").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_program_is_found_at_the_top_of_an_archive_too_and_replaces_an_older_copy() {
        let dir = scratch("unpack-flat");
        let name = ffmpeg::executable_name();
        fs::write(dir.join(name), b"the old one").unwrap();
        let zip_path = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"the new one").unwrap();
            w.finish().unwrap();
        }
        let exe = unpack(&zip_path, &dir).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"the new one");
        assert!(!dir.join(format!("{name}.old")).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_archive_without_ffmpeg_is_refused() {
        let dir = scratch("unpack-none");
        let zip_path = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            w.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(unpack(&zip_path, &dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
