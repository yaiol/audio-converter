//! The one conversion runner of the process, the ffmpeg it drives, and the ffmpeg install.
//!
//! ffmpeg is looked up when the process starts (only in the folder every yaiol app shares, never on PATH)
//! and again after an install. No ffmpeg is a real state the window reports instead of failing later at
//! Convert.

use engine::{
    Runner,
    download::{Download, Latest},
};
use std::sync::LazyLock;

pub static RUNNER: LazyLock<Runner> = LazyLock::new(|| {
    let runner = Runner::without_ffmpeg();
    refresh_ffmpeg(&runner);
    runner
});

pub static DOWNLOAD: LazyLock<Download> = LazyLock::new(Download::new);

/// Which ffmpeg yaiol says is the compatible one; asked at launch and again when Settings opens.
pub static LATEST: LazyLock<Latest> = LazyLock::new(Latest::new);

/// Look for ffmpeg again; true when one was found.
pub fn refresh_ffmpeg(runner: &Runner) -> bool {
    let dir = engine::settings::ffmpeg_dir();
    let found = engine::ffmpeg::locate(dir.as_deref());
    // A catalogue that cannot be read is the same as none: the effects page says they are not installed.
    let _ = engine::effects::load_installed(dir.as_deref());
    let have = found.is_some();
    runner.set_ffmpeg(found);
    have
}
