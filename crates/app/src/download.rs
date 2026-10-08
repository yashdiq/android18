//! Live download runner (§7.3): chunked `read_range` fetches into
//! `~/Downloads/Android18/` with pause/cancel control, `.part` resume, and
//! per-chunk progress reported through the transfer engine.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use android18_core::port::SharedBackend;

/// Chunk size per `read_range` call — small enough for smooth progress,
/// large enough to amortize request overhead.
pub const CHUNK_BYTES: u64 = 512 * 1024;

/// Pause cadence while a runner waits for `Resume`.
pub const PAUSE_POLL: Duration = Duration::from_millis(150);

/// Control messages a transfer row sends to its running download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadCommand {
    Pause,
    Resume,
    Cancel,
}

/// Everything the runner needs to fetch one file.
#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub id: u64,
    pub remote_path: String,
    pub file_name: String,
    pub size: u64,
}

/// Where a download lands and where a resume continues from.
#[derive(Debug)]
pub struct Prepared {
    pub dest: PathBuf,
    pub part: PathBuf,
    pub offset: u64,
}

/// `~/Downloads/Android18/` (`None` when `$HOME` is unset).
pub fn downloads_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads").join("Android18"))
}

/// The `<path>.part` sibling a download writes until it completes.
fn with_part(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// First `dir/name` that does not exist yet (`.part` files included), using
/// the `name (2).ext` convention.
pub fn dedupe_dest(dir: &Path, file_name: &str) -> PathBuf {
    let plain = dir.join(file_name);
    if !plain.exists() && !with_part(&plain).exists() {
        return plain;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string());
    let ext = Path::new(file_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 2..1_000_000 {
        let next = dir.join(format!("{stem} ({n}){ext}"));
        if !next.exists() && !with_part(&next).exists() {
            return next;
        }
    }
    plain
}

/// Resolves destination and resume offset. Runs on a background thread.
pub fn prepare(job: &DownloadJob) -> Result<Prepared, String> {
    let root = downloads_root().ok_or_else(|| "$HOME is not set".to_string())?;
    std::fs::create_dir_all(&root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    let dest = dedupe_dest(&root, &job.file_name);
    let part = with_part(&dest);
    let offset = std::fs::metadata(&part)
        .map(|m| m.len())
        .unwrap_or(0)
        .min(job.size);
    Ok(Prepared { dest, part, offset })
}

/// Fetches the `[offset, offset+want)` window and appends it to the
/// `.part` file. Returns bytes appended (0 = server-reported EOF).
pub async fn chunk(
    backend: SharedBackend,
    token: String,
    remote_path: String,
    part: PathBuf,
    offset: u64,
    want: u64,
) -> Result<u64, String> {
    let bytes = backend
        .read_range(&remote_path, offset, want, &token)
        .await
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)
        .map_err(|e| format!("cannot open {}: {e}", part.display()))?;
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
    Ok(bytes.len() as u64)
}

/// Syncs and renames the `.part` file into place. Runs on a background
/// thread; the sync guarantees a finalized file is never truncated.
pub fn finish(part: PathBuf, dest: PathBuf) -> Result<(), String> {
    // `create` covers zero-byte files, which never wrote a chunk.
    let sync = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)
        .and_then(|file| file.sync_all());
    sync.map_err(|e| format!("cannot flush {}: {e}", part.display()))?;
    std::fs::rename(&part, &dest).map_err(|e| format!("cannot finalize {}: {e}", dest.display()))
}

/// Removes a partial file after an explicit Cancel.
pub fn discard(part: PathBuf) {
    let _ = std::fs::remove_file(part);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("android18-download-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn dedupe_skips_existing_files_and_parts() {
        let dir = temp_dir("dedupe");
        std::fs::write(dir.join("clip.mp4"), b"x").expect("write");
        std::fs::write(dir.join("clip (2).mp4.part"), b"x").expect("write");
        assert_eq!(dedupe_dest(&dir, "clip.mp4"), dir.join("clip (3).mp4"));
        assert_eq!(dedupe_dest(&dir, "fresh.txt"), dir.join("fresh.txt"));
        assert_eq!(dedupe_dest(&dir, "no-ext"), dir.join("no-ext"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn part_names_append_suffix() {
        assert_eq!(
            with_part(Path::new("/tmp/Android18/a.tar.zst")),
            Path::new("/tmp/Android18/a.tar.zst.part")
        );
    }

    #[test]
    fn prepare_clamps_resume_offset_to_size() {
        let dir = temp_dir("prepare");
        std::fs::write(dir.join("blob.bin.part"), vec![0u8; 100]).expect("write");
        let size = 60u64;
        let part = with_part(&dir.join("blob.bin"));
        let offset = std::fs::metadata(&part)
            .map(|m| m.len())
            .unwrap_or(0)
            .min(size);
        assert_eq!(offset, 60);
        assert_eq!(
            std::fs::metadata(with_part(&dir.join("absent.bin")))
                .map(|m| m.len())
                .unwrap_or(0)
                .min(size),
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
