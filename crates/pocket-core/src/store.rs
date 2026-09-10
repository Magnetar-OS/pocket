// SPDX-License-Identifier: MPL-2.0

//! Passes on disk.
//!
//! One directory per pass, holding the `.pkpass` **verbatim**. The suite
//! already stores server bytes untouched for exactly one reason — a model
//! covers less than the format carries, so round-tripping through it loses
//! what it does not know about. For a pass the consequence is harder than
//! lost `X-` properties: a `.pkpass` is signed over its own bytes, so
//! re-serialising one destroys the signature and there is no way back.
//!
//! So the file is the source of truth and [`Pass`] is derived from it on
//! every read. That is the same relationship `cosmic-pim` has between its
//! vdir and its SQLite index.
//!
//! Layout, matching the suite's `$XDG_DATA_HOME` convention:
//!
//! ```text
//! $XDG_DATA_HOME/pocket/passes/
//!   <id>/
//!     pass.pkpass        the bytes exactly as they arrived
//! ```
//!
//! `POCKET_PASS_DIR` overrides the root, as `COSMIC_PIM_CALENDAR_DIR` does
//! for Slate — it is what makes a sandboxed run against scratch data possible.

use crate::model::Pass;
use std::path::{Path, PathBuf};

/// The file inside each pass directory that holds the original archive.
const PASS_FILE: &str = "pass.pkpass";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no data directory: neither POCKET_PASS_DIR nor XDG_DATA_HOME/HOME is set")]
    NoDataDir,
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A pass as it sits in the store: its id, its bytes, and what they parse to.
#[derive(Clone, Debug)]
pub struct StoredPass {
    /// The directory name. Stable, and what a launcher plugin or a URL refers
    /// to — deliberately not the serial number, which is only unique within
    /// one `passTypeIdentifier`.
    pub id: String,
    pub path: PathBuf,
    pub pass: Pass,
}

/// What one pass on disk failed with.
#[derive(Clone, Debug)]
pub struct UnreadablePass {
    pub id: String,
    pub reason: String,
}

/// The result of reading the store: what loaded, and what did not.
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub passes: Vec<StoredPass>,
    pub unreadable: Vec<UnreadablePass>,
}

/// The directory of passes.
#[derive(Clone, Debug)]
pub struct PassStore {
    root: PathBuf,
}

impl PassStore {
    /// Opens the store at the default location.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoDataDir`] when no data directory can be determined.
    pub fn open_default() -> Result<Self, Error> {
        Ok(Self::open(default_root().ok_or(Error::NoDataDir)?))
    }

    /// Opens the store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every pass that reads back cleanly, with the failures reported beside
    /// them.
    ///
    /// A pass that will not parse does not stop the others loading — one
    /// corrupt file must not empty the wallet — but it is not swallowed
    /// either: the caller gets the reason so it can be shown.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when the store's root cannot be listed. A root
    /// that does not exist yet is an empty store, not an error.
    pub fn list(&self) -> Result<Listing, Error> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Listing::default());
            }
            Err(source) => {
                return Err(Error::Io {
                    path: self.root.clone(),
                    source,
                });
            }
        };

        let mut passes = Vec::new();
        let mut failures = Vec::new();
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path().join(PASS_FILE);
            if !path.is_file() {
                continue;
            }
            match self.load(&id) {
                Ok(pass) => passes.push(pass),
                Err(reason) => failures.push(UnreadablePass { id, reason }),
            }
        }

        // Soonest-relevant first, then passes with no date, then by title —
        // which is the order a wallet is useful in. A pass with no relevant
        // date is a loyalty card, and it belongs below today's flight.
        passes.sort_by(|a, b| {
            (a.pass.relevant_date.is_none(), a.pass.relevant_date)
                .cmp(&(b.pass.relevant_date.is_none(), b.pass.relevant_date))
                .then_with(|| a.pass.title().cmp(b.pass.title()))
        });
        Ok(Listing {
            passes,
            unreadable: failures,
        })
    }

    /// Reads one pass by id, returning the parse failure as a string when it
    /// will not read.
    fn load(&self, id: &str) -> Result<StoredPass, String> {
        let path = self.root.join(id).join(PASS_FILE);
        let bytes = std::fs::read(&path).map_err(|why| why.to_string())?;
        let pass = crate::pkpass::read(&bytes).map_err(|why| why.to_string())?;
        Ok(StoredPass {
            id: id.to_owned(),
            path,
            pass,
        })
    }

    /// The original archive bytes for a pass.
    ///
    /// The bytes, not the model: handing a `.pkpass` back out — to `peek`, to
    /// a file manager, to another device — has to hand back what arrived, or
    /// its signature no longer verifies.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when the file cannot be read.
    pub fn bytes(&self, id: &str) -> Result<Vec<u8>, Error> {
        let path = self.root.join(id).join(PASS_FILE);
        std::fs::read(&path).map_err(|source| Error::Io { path, source })
    }
}

/// `$POCKET_PASS_DIR`, else `$XDG_DATA_HOME/pocket/passes`, else
/// `$HOME/.local/share/pocket/passes`.
#[must_use]
pub fn default_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("POCKET_PASS_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(data.join("pocket").join("passes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_that_does_not_exist_is_an_empty_store_not_an_error() {
        let store = PassStore::open("/nonexistent/pocket/passes");
        let listing = store.list().unwrap();
        assert!(listing.passes.is_empty());
        assert!(listing.unreadable.is_empty());
    }

    #[test]
    fn a_corrupt_pass_is_reported_without_hiding_the_others() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("broken")).unwrap();
        std::fs::write(dir.path().join("broken").join(PASS_FILE), b"not a zip").unwrap();

        let listing = PassStore::open(dir.path()).list().unwrap();
        assert!(listing.passes.is_empty());
        assert_eq!(listing.unreadable.len(), 1);
        assert_eq!(listing.unreadable[0].id, "broken");
    }
}
