use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use brdu_crypto::tagged;

const CHECK_TAG: &str = "brdu/store/record";
/// A record larger than this is garbage (bounds the allocation when reading).
pub const MAX_RECORD: usize = 64 * 1024 * 1024;

/// **Append-only** record log: `[u32 length][bytes][4 check bytes]`. A crash in the middle of
/// a write leaves an incomplete final record, which `open` **truncates** (nothing before it is lost). It serves the
/// block log and the consensus journal.
pub struct RecordLog {
    path: PathBuf,
    file: File,
}

fn check(bytes: &[u8]) -> [u8; 4] {
    let h = tagged(CHECK_TAG, &[bytes]);
    let mut out = [0u8; 4];
    out.copy_from_slice(&h.as_bytes()[..4]);
    out
}

impl RecordLog {
    /// Opens (or creates) the log and returns the intact records. Truncates a corrupted or incomplete tail.
    pub fn open(path: &Path) -> std::io::Result<(Self, Vec<Vec<u8>>)> {
        let mut raw = Vec::new();
        if let Ok(mut f) = File::open(path) {
            f.read_to_end(&mut raw)?;
        }
        let mut records = Vec::new();
        let mut at = 0usize;
        while let Some(len_bytes) = raw.get(at..at + 4) {
            let len = u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]])
                as usize;
            let (start, end) = (at + 4, at + 4 + len);
            let Some(body) = (len <= MAX_RECORD).then(|| raw.get(start..end)).flatten() else {
                break;
            };
            let Some(sum) = raw.get(end..end + 4) else {
                break;
            };
            if sum != check(body) {
                break;
            }
            records.push(body.to_vec());
            at = end + 4;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.set_len(at as u64)?;
        let mut me = Self {
            path: path.to_path_buf(),
            file,
        };
        me.seek_end()?;
        Ok((me, records))
    }

    fn seek_end(&mut self) -> std::io::Result<()> {
        use std::io::Seek;
        self.file.seek(std::io::SeekFrom::End(0)).map(|_| ())
    }

    /// Appends a record. With `sync`, returns only after the disk confirms (the consensus journal uses this).
    pub fn append(&mut self, bytes: &[u8], sync: bool) -> std::io::Result<()> {
        let len =
            u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("record too large"))?;
        let mut out = Vec::with_capacity(bytes.len() + 8);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(bytes);
        out.extend_from_slice(&check(bytes));
        self.file.write_all(&out)?;
        if sync {
            self.file.sync_data()?;
        }
        Ok(())
    }

    /// Empties the log (the journal restarts at each height).
    pub fn clear(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.seek_end()
    }

    /// The file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Deletes the file (tests and `reset`).
    pub fn remove(path: &Path) -> std::io::Result<()> {
        match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}
