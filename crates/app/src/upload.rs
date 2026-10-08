//! Live upload runner support (§7.3): local-file slicing for the chunked
//! `upload_chunk` wire call. The loop itself lives in
//! [`crate::workspace::Workspace::spawn_upload`]; each helper is stateless
//! (open + seek + read) so runners stay cancelable between chunks.

use std::path::PathBuf;

/// Bytes per `upload_chunk` call — mirrors the download chunk size so
/// progress cadence feels symmetric in both directions.
pub const CHUNK_BYTES: usize = 512 * 1024;

/// Control messages a transfer row sends to its running upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadCommand {
    Pause,
    Resume,
    Cancel,
}

/// Everything the runner needs to push one local file to the phone.
#[derive(Debug, Clone)]
pub struct UploadJob {
    pub id: u64,
    pub local: PathBuf,
    pub remote_folder: String,
    pub name: String,
    pub size: u64,
}

/// Full path of the file as the phone should store it.
impl UploadJob {
    pub fn remote_path(&self) -> String {
        format!("{}/{}", self.remote_folder, self.name)
    }
}

/// Sanity-checks the source before queuing the transfer. Runs on the UI
/// thread only for the `metadata` call; the read itself stays backgrounded.
pub fn probe(path: &PathBuf) -> Result<u64, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    Ok(meta.len())
}

/// Reads `want` bytes at `offset` (stateless open/seek/read so no file
/// handle has to cross an await). Runs on a background thread.
pub fn read_chunk(job: &UploadJob, offset: u64, want: usize) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    use std::io::Seek as _;
    use std::io::SeekFrom;

    let mut file = std::fs::File::open(&job.local)
        .map_err(|e| format!("cannot open {}: {e}", job.local.display()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("cannot seek {}: {e}", job.local.display()))?;
    let mut buffer = vec![0u8; want];
    let read = file
        .read(&mut buffer)
        .map_err(|e| format!("cannot read {}: {e}", job.local.display()))?;
    buffer.truncate(read);
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("android18-upload-{tag}-{}.bin", std::process::id()));
        std::fs::write(&path, bytes).expect("temp file");
        path
    }

    #[test]
    fn read_chunk_slices_from_offset() {
        let path = temp_file("slice", b"abcdefgh");
        let job = UploadJob {
            id: 1,
            local: path.clone(),
            remote_folder: "~".into(),
            name: "slice.bin".into(),
            size: 8,
        };
        assert_eq!(read_chunk(&job, 0, 4).expect("chunk"), b"abcd");
        assert_eq!(read_chunk(&job, 4, 4).expect("chunk"), b"efgh");
        assert_eq!(read_chunk(&job, 6, 4).expect("chunk"), b"gh"); // short read at EOF
        assert_eq!(read_chunk(&job, 8, 4).expect("chunk"), b""); // at EOF
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn probe_reports_size_and_rejects_directories() {
        let path = temp_file("probe", b"12345");
        assert_eq!(probe(&path).expect("probe"), 5);
        let dir = std::env::temp_dir();
        assert!(probe(&dir).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn remote_path_joins_folder_and_name() {
        let job = UploadJob {
            id: 1,
            local: PathBuf::from("/tmp/a b.txt"),
            remote_folder: "~/DCIM/Camera".into(),
            name: "a b.txt".into(),
            size: 1,
        };
        assert_eq!(job.remote_path(), "~/DCIM/Camera/a b.txt");
    }
}
