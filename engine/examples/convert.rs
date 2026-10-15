//! A command-line front end to the same engine the window uses - for checking a real library
//! without the window:
//!
//!   cargo run -p audio-converter-engine --example convert -- <format> <id3: 23|24> <out folder> <files...>
//!
//! It converts into `<out folder>` with the built-in "source" naming, never beside the originals.

use audio_converter_engine::{Destination, Format, Id3Version, JobSpec, JobStatus, Overwrite, Preset, Runner, TagCleanup};
use std::{path::PathBuf, thread, time::Duration};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("usage: convert <format> <23|24> <out folder> <files...>");
        std::process::exit(2);
    }
    let format = Format::from_id(&a[0]).unwrap_or_else(|| panic!("unknown format {}", a[0]));
    let id3 = if a[1] == "24" { Id3Version::V24 } else { Id3Version::V23 };
    let out = PathBuf::from(&a[2]);
    let ffmpeg = audio_converter_engine::ffmpeg::locate().expect("no ffmpeg found");
    let runner = Runner::new(ffmpeg);
    let dest = Destination { folder: out.to_string_lossy().into_owned(), ..Destination::new("cli", "") };
    let specs: Vec<JobSpec> = a[3..]
        .iter()
        .enumerate()
        .map(|(i, f)| JobSpec {
            id: i as u64,
            input: PathBuf::from(f),
            source_root: None,
            preset: Preset { id3_version: id3, ..Preset::new("cli", format) },
            cleanup: TagCleanup::keep(),
            destination: dest.clone(),
            overwrite: Overwrite::Rename,
            effects: Default::default(),
        })
        .collect();
    runner.start(specs, 4);
    while runner.busy() {
        thread::sleep(Duration::from_millis(100));
    }
    for (id, s) in runner.snapshot() {
        match s {
            JobStatus::Done { output } => println!("{id}: {}", output.display()),
            other => println!("{id}: {other:?}"),
        }
    }
}
