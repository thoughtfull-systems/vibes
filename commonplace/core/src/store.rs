// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Keeps a library on disk as a Loro snapshot plus a log of the updates made since.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use loro::{ExportMode, LoroDoc, VersionVector};

use crate::Error;

const COMPACT_AFTER: u64 = 4 << 20;
const LOCK: &str = "lock";
const LOG: &str = "library.log";
const SNAPSHOT: &str = "library.loro";

pub(crate) struct Store {
    dir: PathBuf,
    // Holds the lock for as long as the store is open.
    _lock: File,
    log: File,
    saved: VersionVector,
}

impl Store {
    /// Locks the library in `dir` and loads it into `doc`.
    pub(crate) fn open(dir: &Path, doc: &LoroDoc) -> Result<Self, Error> {
        fs::create_dir_all(dir)?;
        let lock = File::create(dir.join(LOCK))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::Locked(dir.to_owned())),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        match fs::read(dir.join(SNAPSHOT)) {
            Ok(bytes) => {
                doc.import(&bytes).map_err(loro_error)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut log =
            OpenOptions::new().create(true).read(true).append(true).open(dir.join(LOG))?;
        let updates = read_log(&mut log)?;
        if !updates.is_empty() {
            doc.import_batch(&updates).map_err(loro_error)?;
        }
        let mut store = Self { dir: dir.to_owned(), _lock: lock, log, saved: doc.oplog_vv() };
        if !updates.is_empty() {
            store.compact(doc)?;
        }
        Ok(store)
    }

    /// Appends the changes since the last save to the log.
    pub(crate) fn save(&mut self, doc: &LoroDoc) -> Result<(), Error> {
        let current = doc.oplog_vv();
        if current == self.saved {
            return Ok(());
        }
        let bytes = doc.export(ExportMode::updates(&self.saved)).map_err(loro_error)?;
        let length = u32::try_from(bytes.len()).map_err(|_| loro_error("update too large"))?;
        let mut record = Vec::with_capacity(bytes.len() + 4);
        record.extend_from_slice(&length.to_le_bytes());
        record.extend_from_slice(&bytes);
        self.log.write_all(&record)?;
        self.log.sync_data()?;
        self.saved = current;
        if self.log.metadata()?.len() > COMPACT_AFTER {
            self.compact(doc)?;
        }
        Ok(())
    }

    /// Replaces the snapshot with the whole document and empties the log.
    fn compact(&mut self, doc: &LoroDoc) -> Result<(), Error> {
        let bytes = doc.export(ExportMode::Snapshot).map_err(loro_error)?;
        let temporary = self.dir.join(format!("{SNAPSHOT}.new"));
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, self.dir.join(SNAPSHOT))?;
        File::open(&self.dir)?.sync_all()?;
        // Every update in the log is now in the snapshot. If emptying the log fails, the next open
        // imports those updates again, which Loro ignores.
        self.log.set_len(0)?;
        self.log.sync_all()?;
        Ok(())
    }
}

/// Reads every complete record, and cuts off a partial record left by an interrupted write.
fn read_log(log: &mut File) -> Result<Vec<Vec<u8>>, Error> {
    let mut bytes = Vec::new();
    log.seek(SeekFrom::Start(0))?;
    log.read_to_end(&mut bytes)?;
    let mut updates = Vec::new();
    let mut offset = 0;
    while let Some(header) = bytes.get(offset..offset + 4) {
        let length = u32::from_le_bytes(header.try_into().unwrap()) as usize;
        let Some(update) = bytes.get(offset + 4..offset + 4 + length) else {
            break;
        };
        updates.push(update.to_vec());
        offset += 4 + length;
    }
    if offset < bytes.len() {
        log.set_len(offset as u64)?;
    }
    Ok(updates)
}

fn loro_error(error: impl ToString) -> Error {
    Error::Loro(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_off_partial_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG);
        fs::write(&path, [1, 0, 0, 0, 42, 9, 0, 0, 0, 1, 2]).unwrap();
        let mut log = OpenOptions::new().read(true).append(true).open(&path).unwrap();
        assert_eq!(read_log(&mut log).unwrap(), vec![vec![42]]);
        assert_eq!(fs::read(&path).unwrap(), [1, 0, 0, 0, 42]);
    }

    #[test]
    fn refuses_second_writer() {
        let dir = tempfile::tempdir().unwrap();
        let _store = Store::open(dir.path(), &LoroDoc::new()).unwrap();
        assert!(matches!(Store::open(dir.path(), &LoroDoc::new()), Err(Error::Locked(_))));
    }
}
