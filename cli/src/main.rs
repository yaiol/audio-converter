// ⚠ CLAUDE - NO HARDCODED MESSAGES IN THIS FILE.
//   Every message a user can read goes through the i18n lookup with the key a string literal at the call
//   site, like the window. The one exception is the --help text clap renders: documentation of a flag
//   is conventionally English, and the flags themselves are not translated.

//! yac - yaiol audio converter, on the command line.
//!
//! The same engine and THE SAME saved choices as the window: presets, tag cleanings, effects and
//! destinations are read from the window's own `settings.json` and picked by name. Nothing here
//! converts, tags or measures anything - this crate parses arguments, calls the engine and prints.

use clap::{Parser, ValueEnum};
use engine::{
    Destination, JobSpec, JobStatus, Overwrite, Runner,
    destination::SOURCE_ID,
    i18n,
    preset::ORIGINAL_ID,
    settings::{self, NONE_ID, Settings},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

const PRODUCT: &str = env!("YAIOL_PRODUCT_NAME");
const VERSION: &str = env!("YAIOL_APP_VERSION");

#[derive(Clone, Copy, ValueEnum)]
enum OverwriteArg {
    Skip,
    Overwrite,
    Rename,
}

/// Convert audio files with the presets, tag settings, effects and destinations saved in Audio Converter.
#[derive(Parser)]
#[command(name = "yac", version = VERSION)]
struct Args {
    /// Files and folders to convert (a folder is searched recursively)
    inputs: Vec<PathBuf>,

    /// Saved preset, by name or id ("original" keeps each file's own format)
    #[arg(short, long, value_name = "NAME")]
    preset: Option<String>,

    /// Saved tag setting, by name or id (left out: every tag and the cover are kept)
    #[arg(short, long, value_name = "NAME")]
    tags: Option<String>,

    /// Saved effects, by name or id (left out: nothing is applied)
    #[arg(short, long, value_name = "NAME")]
    effects: Option<String>,

    /// Saved destination (folder, file-name pattern, structure), by name or id
    #[arg(short, long, value_name = "NAME")]
    destination: Option<String>,

    /// Write into this folder (with -d: the destination's pattern, in this folder)
    #[arg(short, long, value_name = "FOLDER")]
    output: Option<PathBuf>,

    /// Conversions at once (1 to the machine's physical cores)
    #[arg(short, long, value_name = "N")]
    jobs: Option<u32>,

    /// What to do when an output already exists
    #[arg(long, value_enum)]
    overwrite: Option<OverwriteArg>,

    /// List the saved presets, tag settings, effects and destinations, and exit
    #[arg(long)]
    list: bool,

    /// Print only failures and the final summary
    #[arg(short, long)]
    quiet: bool,
}

/// A saved entry a flag can name: its id and its display name.
struct Entry {
    id: String,
    name: String,
}

fn entries<'a>(first: Option<(&str, String)>, rest: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<Entry> {
    first
        .into_iter()
        .map(|(id, name)| Entry { id: id.to_owned(), name })
        .chain(rest.map(|(id, name)| Entry { id: id.to_owned(), name: name.to_owned() }))
        .collect()
}

/// The id of the entry `wanted` names: its id, or its name in any case.
fn pick(flag: &str, wanted: &str, list: &[Entry], lang: &str) -> Result<String, String> {
    let w = wanted.trim();
    if let Some(e) = list.iter().find(|e| e.id == w).or_else(|| list.iter().find(|e| e.name.eq_ignore_ascii_case(w))) {
        return Ok(e.id.clone());
    }
    let names = list.iter().map(|e| format!("\"{}\"", e.name)).collect::<Vec<_>>().join(", ");
    Err(i18n::t(lang, "msgCliUnknown").replace("{flag}", flag).replace("{name}", w).replace("{list}", &names))
}

/// The four lists of saved entries, in the window's order, each led by its window-only choice.
struct Lists {
    presets: Vec<Entry>,
    tags: Vec<Entry>,
    effects: Vec<Entry>,
    destinations: Vec<Entry>,
}

fn lists(s: &Settings) -> Lists {
    let lang = &s.lang;
    Lists {
        presets: entries(
            Some((ORIGINAL_ID, i18n::t(lang, "lblMainPresetOriginal"))),
            s.presets.iter().map(|p| (p.id.as_str(), p.name.as_str())),
        ),
        // No "(None)" entry for tags and effects: leaving the flag out IS none, so there is nothing to name.
        tags: entries(None, s.cleanups.iter().map(|c| (c.id.as_str(), c.name.as_str()))),
        effects: entries(None, s.effects.iter().map(|e| (e.id.as_str(), e.name.as_str()))),
        destinations: entries(
            Some((SOURCE_ID, i18n::t(lang, "lblMainDestSource"))),
            s.destinations.iter().map(|d| (d.id.as_str(), d.name.as_str())),
        ),
    }
}

/// `--list`: what a flag can name, the window's current choice marked with `*`.
fn print_list(s: &Settings) {
    let l = lists(s);
    let block = |flag: &str, list: &[Entry], current: &str| {
        println!("{flag}");
        for e in list {
            println!("  {} {}", if e.id == current { '*' } else { ' ' }, e.name);
        }
    };
    block("--preset", &l.presets, &s.preset_id);
    // Tags and effects are nothing unless a flag asks (the window's selection is not carried over),
    // so none of their entries is the default and none is marked.
    block("--tags", &l.tags, "");
    block("--effects", &l.effects, "");
    block("--destination", &l.destinations, &s.destination_id);
}

fn main() -> ExitCode {
    let args = Args::parse();
    let mut s = settings::load(PRODUCT);
    let lang = s.lang.clone();
    let fail = |msg: String| {
        eprintln!("{msg}");
        ExitCode::from(2)
    };

    if args.list {
        print_list(&s);
        return ExitCode::SUCCESS;
    }
    if args.inputs.is_empty() {
        return fail(i18n::t(&lang, "msgCliNoInput"));
    }

    // The preset and the destination default to the window's selection; the tags and the effects
    // default to (None) - a command that cleans tags or changes the sound says so on its line. A flag
    // replaces one choice for this run only.
    s.cleanup_id = NONE_ID.to_owned();
    s.effects_id = NONE_ID.to_owned();
    let l = lists(&s);
    let picks = [
        ("--preset", &args.preset, &l.presets),
        ("--tags", &args.tags, &l.tags),
        ("--effects", &args.effects, &l.effects),
        ("--destination", &args.destination, &l.destinations),
    ];
    let mut chosen: Vec<Option<String>> = Vec::new();
    for (flag, wanted, list) in picks {
        match wanted {
            Some(w) => match pick(flag, w, list, &lang) {
                Ok(id) => chosen.push(Some(id)),
                Err(e) => return fail(e),
            },
            None => chosen.push(None),
        }
    }
    if let Some(id) = chosen[0].take() {
        s.preset_id = id;
    }
    if let Some(id) = chosen[1].take() {
        s.cleanup_id = id;
    }
    if let Some(id) = chosen[2].take() {
        s.effects_id = id;
    }
    if let Some(id) = chosen[3].take() {
        s.destination_id = id;
    }

    let preset = s.selected_preset();
    let cleanup = s.selected_cleanup();
    let effects = s.selected_effects();
    let mut destination = s.selected_destination();
    if let Some(out) = &args.output {
        let folder = std::path::absolute(out).unwrap_or_else(|_| out.clone()).to_string_lossy().into_owned();
        if destination.is_source() {
            destination = Destination::new("", &folder);
        } else {
            destination.folder = folder;
        }
    }
    let overwrite = match args.overwrite {
        Some(OverwriteArg::Skip) => Overwrite::Skip,
        Some(OverwriteArg::Overwrite) => Overwrite::Overwrite,
        Some(OverwriteArg::Rename) => Overwrite::Rename,
        None => s.overwrite,
    };
    let parallel = args.jobs.unwrap_or(s.parallel).clamp(1, settings::max_parallel()) as usize;

    // The files: each input is itself, or - a folder - everything audio under it, which also becomes
    // the root its sub-folders are kept relative to.
    let mut specs: Vec<JobSpec> = Vec::new();
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for input in &args.inputs {
        if !input.exists() {
            eprintln!("{}", i18n::t(&lang, "msgCliMissing").replace("{path}", &input.display().to_string()));
            continue;
        }
        let root = input.is_dir().then(|| input.clone());
        let mut found = Vec::new();
        engine::collect_audio(input, &mut found);
        for path in found {
            if !seen.insert(path.clone()) {
                continue;
            }
            let id = specs.len() as u64 + 1;
            names.insert(id, path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            specs.push(JobSpec {
                id,
                input: path,
                source_root: root.clone(),
                preset: preset.clone(),
                cleanup: cleanup.clone(),
                destination: destination.clone(),
                overwrite,
                effects: effects.clone(),
            });
        }
    }
    if specs.is_empty() {
        return fail(i18n::t(&lang, "msgCliNoAudio"));
    }

    let Some(ffmpeg) = engine::ffmpeg::locate(engine::settings::ffmpeg_dir().as_deref()) else {
        return fail(i18n::t(&lang, "msgCliNoFfmpeg"));
    };
    let runner = Runner::new(ffmpeg);
    runner.start(specs, parallel);
    run(&runner, &names, &lang, args.quiet)
}

/// Waits for the run, printing each file as it ends, then the summary. Exit 1 when any file failed.
fn run(runner: &Runner, names: &HashMap<u64, String>, lang: &str, quiet: bool) -> ExitCode {
    let mut shown: HashSet<u64> = HashSet::new();
    let name = |id: &u64| names.get(id).map_or("", String::as_str);
    let snapshot = loop {
        std::thread::sleep(Duration::from_millis(200));
        let snap: HashMap<u64, JobStatus> = runner.snapshot().into_iter().collect();
        for (id, status) in &snap {
            if status.is_active() || !shown.insert(*id) {
                continue;
            }
            match status {
                JobStatus::Done { output } if !quiet => println!("{}: {} -> {}", i18n::t(lang, "msgMainStatusDone"), name(id), output.display()),
                JobStatus::Skipped { output } if !quiet => {
                    println!("{}: {} -> {}", i18n::t(lang, "msgMainStatusSkipped"), name(id), output.display())
                }
                JobStatus::Failed { reason } => eprintln!("{}: {}: {reason}", i18n::t(lang, "msgMainStatusFailed"), name(id)),
                _ => {}
            }
        }
        if !snap.values().any(JobStatus::is_active) {
            break snap;
        }
    };
    let summary = engine::run_summary(snapshot.values(), lang);
    if !summary.is_empty() {
        println!("{summary}");
    }
    if snapshot.values().any(|s| matches!(s, JobStatus::Failed { .. })) { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<Entry> {
        entries(Some(("original", "(Original)".to_owned())), [("p1", "MP3 320 kbps"), ("p2", "FLAC")].into_iter())
    }

    #[test]
    fn an_entry_is_found_by_its_id_or_its_name_in_any_case() {
        assert_eq!(pick("--preset", "p2", &list(), "en").unwrap(), "p2");
        assert_eq!(pick("--preset", "mp3 320 KBPS", &list(), "en").unwrap(), "p1");
        assert_eq!(pick("--preset", " flac ", &list(), "en").unwrap(), "p2");
        assert_eq!(pick("--preset", "original", &list(), "en").unwrap(), "original");
    }

    #[test]
    fn an_unknown_name_says_which_flag_and_what_is_saved() {
        let e = pick("--tags", "nope", &list(), "en").unwrap_err();
        assert!(e.contains("--tags") && e.contains("nope"), "{e}");
        assert!(e.contains("\"FLAC\"") && e.contains("\"MP3 320 kbps\""), "{e}");
    }

    #[test]
    fn the_flags_parse() {
        let a = Args::try_parse_from(["yac", "-p", "FLAC", "-t", "none", "-e", "x", "-d", "Export", "-o", "out", "-j", "4", "--overwrite", "rename", "-q", "in"])
            .unwrap();
        assert_eq!(a.preset.as_deref(), Some("FLAC"));
        assert_eq!(a.destination.as_deref(), Some("Export"));
        assert_eq!(a.output, Some(PathBuf::from("out")));
        assert_eq!(a.jobs, Some(4));
        assert!(a.quiet && matches!(a.overwrite, Some(OverwriteArg::Rename)));
        assert_eq!(a.inputs, [PathBuf::from("in")]);
    }

    #[test]
    fn an_unknown_overwrite_policy_is_a_usage_error() {
        assert!(Args::try_parse_from(["yac", "--overwrite", "maybe", "in"]).is_err());
    }
}
