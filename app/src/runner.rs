//! The one conversion runner of the process, and the ffmpeg it drives.
//!
//! ffmpeg is looked up once: next to the executable (what the installers ship), then on PATH.
//! `None` is a real state the window reports instead of failing later at Convert.

use engine::Runner;
use std::{path::PathBuf, sync::LazyLock};

pub static FFMPEG: LazyLock<Option<PathBuf>> = LazyLock::new(engine::ffmpeg::locate);

pub static RUNNER: LazyLock<Option<Runner>> = LazyLock::new(|| FFMPEG.clone().map(Runner::new));
