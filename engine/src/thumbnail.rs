//! Cover thumbnails for the file list.
//!
//! The picture is read straight out of the file's tag with the tag library and shrunk in-process:
//! no process is started, so a folder of hundreds of files fills in at the speed of reading a few
//! kilobytes from each. ffmpeg is only the fallback, for a file the tag library cannot read or a
//! picture the in-process decoder does not know (a WMA's cover, say, or a WebP).

use image::{ImageFormat, imageops::FilterType};
use lofty::{
    config::ParseOptions,
    file::TaggedFileExt,
    picture::PictureType,
    probe::Probe,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    io::Cursor,
    path::Path,
    sync::{LazyLock, Mutex, atomic::{AtomicUsize, Ordering}},
};

/// Thumbnails already made, by the picture they came from. An album usually carries the SAME cover
/// in every track, so the first track pays for the decode and the other thirty only read their tag
/// and hash the bytes - about a millisecond each instead of about forty.
///
/// The key is the picture's length plus a 64-bit hash of its bytes (and the target size). A clash
/// would show one cover on the wrong row, which at 2^-64 is not a risk worth a slower key.
static CACHE: LazyLock<Mutex<HashMap<(u64, usize, u32), Vec<u8>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Enough for a large folder of different albums; past it the cache is simply emptied.
const CACHE_LIMIT: usize = 512;

/// How many pictures were actually decoded (as opposed to answered from the cache). For tests.
static DECODED: AtomicUsize = AtomicUsize::new(0);

fn picture_key(bytes: &[u8], size: u32) -> (u64, usize, u32) {
    let mut h = std::hash::DefaultHasher::new();
    bytes.hash(&mut h);
    (h.finish(), bytes.len(), size)
}

/// The embedded picture's bytes, front cover first. Only the tags are parsed: no audio properties,
/// which is the part of reading a file that costs time.
fn embedded_picture(path: &Path) -> Option<Vec<u8>> {
    let file = Probe::open(path)
        .ok()?
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let pic = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;
    Some(pic.data().to_vec())
}

/// A square JPEG of `size` px - cropped to fill, so every thumbnail is the same shape - from a
/// picture's bytes. `None` when the format is not one the decoder was built with.
///
/// ⚠ CLAUDE: the first step down is `thumbnail()` (a fast box sample), and only the small result goes
/// through the filtered `resize_to_fill`. Filtering a 1400 px cover directly costs 40-50 ms, as much as
/// decoding it. A JPEG decoder that scales while decoding (`jpeg-decoder`) was tried and was SLOWER
/// (about 220 ms a cover against 25 ms for the SIMD decoder `image` uses) - do not reintroduce it.
fn shrink(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(size * 2, size * 2);
    let thumb = img.resize_to_fill(size, size, FilterType::Triangle).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    thumb.write_to(&mut out, ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

/// The cover of `path` as a small square JPEG, or `None` when it has none.
///
/// `ffmpeg` is the fallback and may be `None` (the app then simply has no thumbnails it cannot make
/// itself).
pub fn cover_thumbnail(ffmpeg: Option<&Path>, path: &Path, size: u32) -> Option<Vec<u8>> {
    if let Some(bytes) = embedded_picture(path) {
        let key = picture_key(&bytes, size);
        if let Some(hit) = CACHE.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(hit);
        }
        DECODED.fetch_add(1, Ordering::Relaxed);
        if let Some(jpg) = shrink(&bytes, size) {
            if let Ok(mut c) = CACHE.lock() {
                if c.len() >= CACHE_LIMIT {
                    c.clear();
                }
                c.insert(key, jpg.clone());
            }
            return Some(jpg);
        }
    }
    crate::ffmpeg::cover_jpeg(ffmpeg?, path, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg;
    use std::time::Instant;

    fn ffmpeg_path() -> Option<std::path::PathBuf> {
        ffmpeg::locate()
    }

    #[test]
    fn a_cover_is_read_in_process_and_is_a_small_square_jpeg() {
        let Some(ff) = ffmpeg_path() else { return };
        let dir = std::env::temp_dir().join(format!("yaiol-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("c.png");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=600x300", "-frames:v", "1"])
            .arg(&png)
            .status()
            .unwrap()
            .success());
        let with = dir.join("with.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=duration=1", "-i"])
            .arg(&png)
            .args(["-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"])
            .arg(&with)
            .status()
            .unwrap()
            .success());
        let without = dir.join("without.flac");
        assert!(ffmpeg::command(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=duration=1"])
            .arg(&without)
            .status()
            .unwrap()
            .success());

        // No ffmpeg handed over: the answer can only have come from the tag library.
        let t = Instant::now();
        let jpg = cover_thumbnail(None, &with, 96).expect("the picture is read without ffmpeg");
        let took = t.elapsed();
        assert!(jpg.starts_with(&[0xFF, 0xD8]));
        let decoded = image::load_from_memory(&jpg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (96, 96));
        let t2 = Instant::now();
        let _ = ffmpeg::cover_jpeg(&ff, &with, 96).expect("ffmpeg path");
        eprintln!("in-process thumbnail: {took:?}   ffmpeg process: {:?}", t2.elapsed());

        // The same picture in a second file is answered from the cache, not decoded again.
        let copy = dir.join("copy.flac");
        std::fs::copy(&with, &copy).unwrap();
        let before = DECODED.load(Ordering::Relaxed);
        let again = cover_thumbnail(None, &copy, 96).expect("the cached thumbnail");
        assert_eq!(again, jpg);
        assert_eq!(DECODED.load(Ordering::Relaxed), before, "an identical picture was decoded a second time");

        assert!(cover_thumbnail(None, &without, 96).is_none());
        assert!(cover_thumbnail(None, &dir.join("missing.flac"), 96).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
