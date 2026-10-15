//! Times the cover thumbnails of the files given, stage by stage.
//!
//!   cargo run -p audio-converter-engine --example thumb -- <files...>

use audio_converter_engine::{ffmpeg, thumbnail};
use std::{path::PathBuf, time::Instant};

fn main() {
    let ff = ffmpeg::locate();
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(&arg);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let t = Instant::now();
        let a = thumbnail::cover_thumbnail(None, &path, 96);
        let in_process = t.elapsed();

        let t = Instant::now();
        let b = ff.as_deref().and_then(|f| ffmpeg::cover_jpeg(f, &path, 96));
        let process = t.elapsed();

        println!(
            "{:>6} MB  in-process {:>8.1?} ({})   ffmpeg {:>8.1?} ({})  {}",
            size / 1_000_000,
            in_process,
            a.map(|v| format!("{} B", v.len())).unwrap_or_else(|| "none".into()),
            process,
            b.map(|v| format!("{} B", v.len())).unwrap_or_else(|| "none".into()),
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        );
    }
}
