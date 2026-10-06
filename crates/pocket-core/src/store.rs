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
//!
//! # Adding and removing
//!
//! [`PassStore::add`] is the one way a pass gets in: the bytes are read and
//! verified first, exactly as [`PassStore::list`] would read them back, and
//! only then written — through `cosmic_pim_core::atomic`, the suite's
//! crash-safe writer, so a pass is either in the store whole or not there.
//! What arrives is what is stored, byte for byte.
//!
//! A pass is *one* pass however many times it is added. PassKit identifies
//! one by its `passTypeIdentifier` and `serialNumber` together — the pair the
//! issuer's update service addresses it by — so adding a pass the store
//! already holds under that pair replaces the stored copy (an issuer's
//! re-send with a new gate, say) instead of putting a second one beside it.

use crate::model::Pass;
use cosmic_pim_core::atomic;
use sha1::{Digest as _, Sha1};
use std::path::{Component, Path, PathBuf};

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

/// Why a pass could not be added. Nothing was stored.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AddError {
    /// The bytes are not a pass the reader accepts: not a `.pkpass`, altered
    /// after signing, or past the archive limits.
    #[error("not a pass that can be added: {0}")]
    Invalid(#[from] crate::pkpass::Error),
    /// The store could not be looked through for an earlier copy of the pass.
    #[error(transparent)]
    Store(#[from] Error),
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Why a pass could not be removed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RemoveError {
    /// The id is not the name of a pass folder: empty, or a path.
    #[error("{0:?} is not the name of a pass")]
    NotAPass(String),
    #[error("removing {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What adding a pass did to the store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Added {
    /// The pass was not in the store, and now is.
    New,
    /// The store held a different version of this pass — the same type
    /// identifier and serial number, other bytes — and the new one replaced
    /// it.
    Updated,
    /// The store already held exactly these bytes. Nothing was written.
    Unchanged,
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
        for entry in entries {
            // Everything in the root is a pass or says why it is not. A pass
            // the list leaves out without a word is one found missing at the
            // gate.
            let entry = match entry {
                Ok(entry) => entry,
                Err(why) => {
                    failures.push(UnreadablePass {
                        id: self.root.display().to_string(),
                        reason: why.to_string(),
                    });
                    continue;
                }
            };
            let id = entry.file_name().to_string_lossy().into_owned();
            // Hidden entries are the file manager's (`.directory`), not
            // passes.
            if id.starts_with('.') {
                continue;
            }
            if !entry.path().is_dir() {
                failures.push(UnreadablePass {
                    id,
                    reason: format!("not a pass folder: a pass is kept as <name>/{PASS_FILE}"),
                });
                continue;
            }
            if !entry.path().join(PASS_FILE).is_file() {
                failures.push(UnreadablePass {
                    id,
                    reason: format!("the folder holds no {PASS_FILE}"),
                });
                continue;
            }
            match self.load(&id) {
                Ok(pass) => passes.push(pass),
                Err(reason) => failures.push(UnreadablePass { id, reason }),
            }
        }

        let now =
            chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).fixed_offset();
        order(&mut passes, now);
        Ok(Listing {
            passes,
            unreadable: failures,
        })
    }

    /// Reads one pass by id, returning the parse failure as a string when it
    /// will not read.
    fn load(&self, id: &str) -> Result<StoredPass, String> {
        use std::io::Read as _;

        let path = self.root.join(id).join(PASS_FILE);
        // Read no more than the reader would accept, plus the one byte that
        // tells it the file is over: a file of any size must not be loaded
        // whole just to be refused.
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| {
                file.take(crate::pkpass::MAX_ARCHIVE_BYTES + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|why| why.to_string())?;
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

    /// Adds a pass to the store from the bytes of its `.pkpass`.
    ///
    /// The bytes are verified before anything is written — the same reader,
    /// the same manifest check and the same size limits as every later read —
    /// and stored verbatim, atomically: a crash mid-add leaves the store as
    /// it was.
    ///
    /// A pass the store already holds, by type identifier and serial number,
    /// is replaced where it is rather than added again; [`Added`] says which
    /// happened. The folder a new pass gets is named from that pair, so two
    /// processes adding the same pass at once write the same file, not two.
    ///
    /// # Errors
    ///
    /// [`AddError::Invalid`] when the bytes are not a pass the reader
    /// accepts, [`AddError::Store`] when the store cannot be searched for an
    /// earlier copy, and [`AddError::Write`] when the pass cannot be written.
    pub fn add(&self, bytes: &[u8]) -> Result<(StoredPass, Added), AddError> {
        let pass = crate::pkpass::read(bytes)?;

        let earlier = self.list()?.passes.into_iter().find(|stored| {
            same_pass(&stored.pass, &pass)
                // A pass with no identity is the same pass only byte for
                // byte, wherever in the store those bytes sit.
                || (!identified(&pass)
                    && !identified(&stored.pass)
                    && self.bytes(&stored.id).is_ok_and(|kept| kept == bytes))
        });
        let (id, outcome) = match earlier {
            Some(stored) => {
                if self.bytes(&stored.id)? == bytes {
                    return Ok((stored, Added::Unchanged));
                }
                (stored.id, Added::Updated)
            }
            None => (id_for(&pass, bytes), Added::New),
        };

        let folder = self.root.join(&id);
        let path = folder.join(PASS_FILE);
        private_folder(&folder).map_err(|source| AddError::Write {
            path: folder.clone(),
            source,
        })?;
        atomic::write_bytes(&path, bytes, None).map_err(|why| AddError::Write {
            path: path.clone(),
            source: match why {
                atomic::Error::Io(source) => source,
                other => std::io::Error::other(other.to_string()),
            },
        })?;
        Ok((StoredPass { id, path, pass }, outcome))
    }

    /// Removes a pass from the store: its `.pkpass`, and its folder when the
    /// pass was all the folder held.
    ///
    /// Anything else in the folder was not put there by this store and is not
    /// this store's to delete; the folder then stays, and [`PassStore::list`]
    /// reports it as holding no pass.
    ///
    /// # Errors
    ///
    /// [`RemoveError::NotAPass`] when `id` is not a single folder name — it is
    /// joined to the store's root, and must never be a way out of it — and
    /// [`RemoveError::Remove`] when the file or the folder cannot be removed,
    /// a pass that is not there included.
    pub fn remove(&self, id: &str) -> Result<(), RemoveError> {
        let mut components = Path::new(id).components();
        if !matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(_)), None)
        ) {
            return Err(RemoveError::NotAPass(id.to_owned()));
        }

        let folder = self.root.join(id);
        let path = folder.join(PASS_FILE);
        std::fs::remove_file(&path).map_err(|source| RemoveError::Remove { path, source })?;
        match std::fs::remove_dir(&folder) {
            Err(source) if source.kind() != std::io::ErrorKind::DirectoryNotEmpty => {
                Err(RemoveError::Remove {
                    path: folder,
                    source,
                })
            }
            _ => Ok(()),
        }
    }
}

/// Whether a pass names both halves of what PassKit identifies it by.
///
/// PassKit requires a type identifier and a serial number, but the reader
/// does not turn a pass away for lacking one. A serial number is unique only
/// within its type, so one without the other identifies nothing: two issuers
/// both numbering from 1 are two passes.
fn identified(pass: &Pass) -> bool {
    !pass.pass_type_identifier.is_empty() && !pass.serial_number.is_empty()
}

/// Whether two passes are the same pass, as PassKit counts it: one type
/// identifier, one serial number.
///
/// A pass that is not [`identified`] is nobody's duplicate by this rule; such
/// passes are told apart by their bytes instead, in [`PassStore::add`] and
/// [`id_for`].
fn same_pass(a: &Pass, b: &Pass) -> bool {
    identified(a)
        && a.pass_type_identifier == b.pass_type_identifier
        && a.serial_number == b.serial_number
}

/// The folder a new pass is stored in: a digest of what identifies it.
///
/// Of the type identifier and serial number, so the same pass always lands in
/// the same folder, whoever adds it and however often. Of the archive itself
/// for a pass that does not name both, so that adding those bytes twice is
/// still one pass. A digest rather than the identifiers themselves because a
/// serial number is whatever the issuer wrote, and a folder name cannot be.
///
/// SHA-1 because it is already here for the manifest; this is a name, not a
/// proof of anything.
fn id_for(pass: &Pass, bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    if !identified(pass) {
        hasher.update(bytes);
    } else {
        hasher.update(pass.pass_type_identifier.as_bytes());
        // A byte neither identifier can hold, so ("ab", "c") and ("a", "bc")
        // are different passes.
        hasher.update([0]);
        hasher.update(pass.serial_number.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Creates a pass's folder, and the store above it, for its owner only.
///
/// A `.pkpass` carries its holder's name, their booking, and the token that
/// authenticates them to the issuer. The writer below creates files with the
/// process's default mode, so it is the folders that keep them private.
/// Folders that already exist are left as their owner made them.
fn private_folder(folder: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder)
}

/// How long after its relevant date a pass with no expiry still counts as
/// current: a day, so that the evening's return leg of a morning flight, or a
/// ticket for a show that ran late, is still at the top.
const STILL_CURRENT_FOR: chrono::TimeDelta = chrono::TimeDelta::days(1);

/// Sorts passes into the order a wallet is useful in, as of `now`.
///
/// 1. Current passes with a date, soonest first — today's flight at the top.
/// 2. Passes with no date, by title: loyalty cards and the like.
/// 3. Past passes, most recent first, so last year's flights sink below the
///    cards instead of sitting above today's. A pass is past when it is
///    voided, when its expiry has passed, or — with no expiry — when its
///    relevant date is more than [`STILL_CURRENT_FOR`] ago.
fn order(passes: &mut [StoredPass], now: chrono::DateTime<chrono::FixedOffset>) {
    let past = |pass: &Pass| {
        pass.voided
            || pass.is_expired(now)
            || (pass.expiration_date.is_none()
                && pass
                    .relevant_date
                    .is_some_and(|date| date + STILL_CURRENT_FOR < now))
    };
    let group = |pass: &Pass| match (past(pass), pass.relevant_date) {
        (false, Some(_)) => 0,
        (false, None) => 1,
        (true, _) => 2,
    };
    passes.sort_by(|a, b| {
        let (a, b) = (&a.pass, &b.pass);
        group(a)
            .cmp(&group(b))
            .then_with(|| match group(a) {
                0 => a.relevant_date.cmp(&b.relevant_date),
                // Most recent first; an undated past pass after the dated.
                2 => b.relevant_date.cmp(&a.relevant_date),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.title().cmp(b.title()))
    });
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

    /// A hostile archive — here, one whose header claims a terabyte — is one
    /// unreadable pass, reported by name; the rest of the wallet still loads.
    #[test]
    fn a_hostile_archive_is_one_unreadable_pass_not_an_empty_wallet() {
        use crate::pkpass::tests::{BOARDING, lying_archive, pkpass};

        let dir = tempfile::tempdir().unwrap();
        for (id, bytes) in [
            ("flight", pkpass(&[("pass.json", BOARDING.as_bytes())])),
            ("hostile", lying_archive("pass.json", b"{}", 1 << 40)),
        ] {
            std::fs::create_dir_all(dir.path().join(id)).unwrap();
            std::fs::write(dir.path().join(id).join(PASS_FILE), bytes).unwrap();
        }

        let listing = PassStore::open(dir.path()).list().unwrap();
        assert_eq!(listing.passes.len(), 1);
        assert_eq!(listing.passes[0].id, "flight");
        assert_eq!(listing.unreadable.len(), 1);
        assert_eq!(listing.unreadable[0].id, "hostile");
    }

    /// A folder with no `pass.pkpass` in it, or a `.pkpass` dropped loose in
    /// the root, is reported rather than passed over. Hidden entries are not
    /// passes and are left alone.
    #[test]
    fn what_is_not_a_pass_is_reported_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("empty")).unwrap();
        std::fs::write(dir.path().join("empty").join("ticket.pkpass"), b"").unwrap();
        std::fs::write(dir.path().join("loose.pkpass"), b"").unwrap();
        std::fs::write(dir.path().join(".directory"), b"").unwrap();

        let listing = PassStore::open(dir.path()).list().unwrap();
        let mut ids: Vec<&str> = listing.unreadable.iter().map(|u| u.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["empty", "loose.pkpass"]);
    }

    /// `BOARDING` with another serial number, and optionally another gate.
    fn boarding(serial: &str, gate: &str) -> Vec<u8> {
        use crate::pkpass::tests::{BOARDING, pkpass};

        let mut json: serde_json::Value = serde_json::from_str(BOARDING).unwrap();
        let object = json.as_object_mut().unwrap();
        object.insert("serialNumber".into(), serial.into());
        object.insert("logoText".into(), format!("Gate {gate}").into());
        pkpass(&[("pass.json", json.to_string().as_bytes())])
    }

    /// A pass that is added is there the next time the store is read, with
    /// the bytes that arrived.
    #[test]
    fn an_added_pass_is_stored_verbatim_and_listed() {
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path().join("pocket").join("passes"));
        let bytes = boarding("ABC123", "A1");

        let (stored, outcome) = store.add(&bytes).unwrap();
        assert_eq!(outcome, Added::New);
        assert_eq!(stored.pass.serial_number, "ABC123");
        assert_eq!(stored.path, store.root().join(&stored.id).join(PASS_FILE));

        let listing = store.list().unwrap();
        assert!(listing.unreadable.is_empty());
        assert_eq!(listing.passes.len(), 1);
        assert_eq!(listing.passes[0].id, stored.id);
        assert_eq!(store.bytes(&stored.id).unwrap(), bytes);
    }

    /// One pass is one pass: adding it again changes nothing, and adding a
    /// newer version of it replaces the stored one instead of sitting beside
    /// it. A different serial number is a different pass.
    #[test]
    fn a_pass_is_deduplicated_by_type_identifier_and_serial_number() {
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());
        let first = boarding("ABC123", "A1");

        let (stored, _) = store.add(&first).unwrap();
        let (again, outcome) = store.add(&first).unwrap();
        assert_eq!(outcome, Added::Unchanged);
        assert_eq!(again.id, stored.id);

        let regated = boarding("ABC123", "B7");
        let (updated, outcome) = store.add(&regated).unwrap();
        assert_eq!(outcome, Added::Updated);
        assert_eq!(updated.id, stored.id);
        assert_eq!(store.bytes(&stored.id).unwrap(), regated);
        assert_eq!(store.list().unwrap().passes.len(), 1);

        let (other, outcome) = store.add(&boarding("XYZ789", "A1")).unwrap();
        assert_eq!(outcome, Added::New);
        assert_ne!(other.id, stored.id);
        assert_eq!(store.list().unwrap().passes.len(), 2);
    }

    /// A pass dropped into the store by hand, in a folder of any name, is
    /// still that pass: adding it again updates it where it is.
    #[test]
    fn a_pass_placed_by_hand_is_found_by_what_it_is_not_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("flight-to-lhr")).unwrap();
        std::fs::write(
            dir.path().join("flight-to-lhr").join(PASS_FILE),
            boarding("ABC123", "A1"),
        )
        .unwrap();

        let store = PassStore::open(dir.path());
        let (stored, outcome) = store.add(&boarding("ABC123", "B7")).unwrap();
        assert_eq!(outcome, Added::Updated);
        assert_eq!(stored.id, "flight-to-lhr");
        assert_eq!(store.list().unwrap().passes.len(), 1);
    }

    /// A pass that names no type identifier and no serial number cannot be
    /// matched by them. It is one pass by its bytes; another such pass is
    /// another pass.
    #[test]
    fn a_pass_with_no_identifiers_is_deduplicated_by_its_bytes() {
        use crate::pkpass::tests::pkpass;

        let anonymous = |name: &str| {
            let json = format!(r#"{{"organizationName":"{name}","generic":{{}}}}"#);
            pkpass(&[("pass.json", json.as_bytes())])
        };
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());

        assert_eq!(store.add(&anonymous("Gym")).unwrap().1, Added::New);
        assert_eq!(store.add(&anonymous("Gym")).unwrap().1, Added::Unchanged);
        assert_eq!(store.add(&anonymous("Library")).unwrap().1, Added::New);
        assert_eq!(store.list().unwrap().passes.len(), 2);
    }

    /// A serial number is unique only within one type identifier, so a pass
    /// that names a serial and no type has nothing to be matched by: two
    /// issuers both numbering from 1 are two passes, and adding the second
    /// must not replace the first.
    #[test]
    fn a_pass_naming_only_one_identifier_is_nobodys_duplicate() {
        use crate::pkpass::tests::pkpass;

        let numbered = |name: &str| {
            let json =
                format!(r#"{{"organizationName":"{name}","serialNumber":"1","generic":{{}}}}"#);
            pkpass(&[("pass.json", json.as_bytes())])
        };
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());

        let (gym, outcome) = store.add(&numbered("Gym")).unwrap();
        assert_eq!(outcome, Added::New);
        let (library, outcome) = store.add(&numbered("Library")).unwrap();
        assert_eq!(outcome, Added::New, "the library card replaced the gym's");
        assert_ne!(library.id, gym.id);
        assert_eq!(store.bytes(&gym.id).unwrap(), numbered("Gym"));
        assert_eq!(store.add(&numbered("Gym")).unwrap().1, Added::Unchanged);
        assert_eq!(store.list().unwrap().passes.len(), 2);
    }

    /// A pass with no identity that is already in the store under a folder
    /// of another name — put there by hand, or by a version that named the
    /// folder differently — is found by its bytes and not added again.
    #[test]
    fn a_pass_with_no_identity_placed_by_hand_is_not_added_again() {
        use crate::pkpass::tests::pkpass;

        let bytes = pkpass(&[(
            "pass.json",
            br#"{"organizationName":"Gym","serialNumber":"1","generic":{}}"#,
        )]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("gym-card")).unwrap();
        std::fs::write(dir.path().join("gym-card").join(PASS_FILE), &bytes).unwrap();

        let store = PassStore::open(dir.path());
        let (stored, outcome) = store.add(&bytes).unwrap();
        assert_eq!(outcome, Added::Unchanged);
        assert_eq!(stored.id, "gym-card");
        assert_eq!(store.list().unwrap().passes.len(), 1);
    }

    /// What the reader would refuse is refused before it is stored: a file
    /// that is not a pass, one altered after signing, and one past the
    /// archive limits. The store is left without so much as a folder.
    #[test]
    fn what_is_not_a_valid_pass_is_not_stored() {
        use crate::pkpass::tests::{BOARDING, lying_archive};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("passes");
        let store = PassStore::open(&root);

        // A `pass.json` changed after its manifest was written.
        let tampered = {
            use std::io::Write as _;

            let manifest = format!(r#"{{"pass.json":"{}"}}"#, "0".repeat(40));
            let mut buffer = Vec::new();
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("pass.json", options).unwrap();
            zip.write_all(BOARDING.as_bytes()).unwrap();
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.finish().unwrap();
            buffer
        };

        for bytes in [
            b"not a zip".to_vec(),
            tampered,
            lying_archive("pass.json", BOARDING.as_bytes(), 1 << 40),
            vec![0; usize::try_from(crate::pkpass::MAX_ARCHIVE_BYTES).unwrap() + 1],
        ] {
            assert!(matches!(store.add(&bytes), Err(AddError::Invalid(_))));
        }
        assert!(!root.exists(), "a refused pass left something behind");
    }

    /// A pass holds its holder's name and the token that authenticates them:
    /// the folders the store creates are for their owner alone, and nothing
    /// is left beside the pass by the write.
    #[cfg(unix)]
    #[test]
    fn the_store_creates_folders_only_their_owner_can_enter() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pocket").join("passes");
        let store = PassStore::open(&root);
        let (stored, _) = store.add(&boarding("ABC123", "A1")).unwrap();

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join(&stored.id)), 0o700);
        let beside: Vec<_> = std::fs::read_dir(root.join(&stored.id))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(beside, [std::ffi::OsString::from(PASS_FILE)]);
    }

    /// A write that cannot land says so, and leaves the pass that was there.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_stored_pass_as_it_was() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());
        let first = boarding("ABC123", "A1");
        let (stored, _) = store.add(&first).unwrap();

        // The pass's folder takes no new files, so the replacement cannot be
        // staged beside the pass it would replace.
        let folder = dir.path().join(&stored.id);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.add(&boarding("ABC123", "B7"));
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(matches!(result, Err(AddError::Write { .. })));
        assert_eq!(store.bytes(&stored.id).unwrap(), first);
    }

    /// Removing a pass takes its file and its folder, and only that pass.
    #[test]
    fn a_removed_pass_is_gone_and_its_neighbours_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());
        let (gone, _) = store.add(&boarding("ABC123", "A1")).unwrap();
        let (kept, _) = store.add(&boarding("XYZ789", "A1")).unwrap();

        store.remove(&gone.id).unwrap();
        assert!(!dir.path().join(&gone.id).exists());
        let listing = store.list().unwrap();
        assert!(listing.unreadable.is_empty());
        assert_eq!(listing.passes.len(), 1);
        assert_eq!(listing.passes[0].id, kept.id);

        // Removing it again is an error, not a silent success.
        assert!(matches!(
            store.remove(&gone.id),
            Err(RemoveError::Remove { .. })
        ));
    }

    /// What else is in a pass's folder was not put there by the store, and
    /// is not deleted with the pass.
    #[test]
    fn removing_a_pass_leaves_what_else_its_folder_holds() {
        let dir = tempfile::tempdir().unwrap();
        let store = PassStore::open(dir.path());
        let (stored, _) = store.add(&boarding("ABC123", "A1")).unwrap();
        let note = dir.path().join(&stored.id).join("receipt.pdf");
        std::fs::write(&note, b"kept").unwrap();

        store.remove(&stored.id).unwrap();
        assert!(note.exists());
        assert!(!dir.path().join(&stored.id).join(PASS_FILE).exists());
    }

    /// An id is a folder name and nothing else. One that is a path must not
    /// become a way to delete a file outside the store.
    #[test]
    fn an_id_that_is_a_path_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join(PASS_FILE), b"not the store's").unwrap();
        let store = PassStore::open(dir.path().join("passes"));

        for id in ["", ".", "..", "../outside", "a/b", "/etc"] {
            assert!(
                matches!(store.remove(id), Err(RemoveError::NotAPass(_))),
                "{id:?} was taken for a pass"
            );
        }
        assert!(outside.join(PASS_FILE).exists());
    }

    /// Upcoming passes first, soonest at the top; then undated cards; then
    /// what is over, most recent first. Strict date order put last year's
    /// flight above today's.
    #[test]
    fn passes_are_ordered_upcoming_then_undated_then_past() {
        use crate::pkpass::tests::{BOARDING, pkpass};

        let stored = |id: &str, relevant: Option<&str>, expiry: Option<&str>, voided: bool| {
            let mut json: serde_json::Value = serde_json::from_str(BOARDING).unwrap();
            let object = json.as_object_mut().unwrap();
            object.insert("logoText".into(), id.into());
            object.remove("relevantDate");
            object.remove("expirationDate");
            if let Some(date) = relevant {
                object.insert("relevantDate".into(), date.into());
            }
            if let Some(date) = expiry {
                object.insert("expirationDate".into(), date.into());
            }
            object.insert("voided".into(), voided.into());
            let text = json.to_string();
            StoredPass {
                id: id.to_owned(),
                path: PathBuf::new(),
                pass: crate::pkpass::read(&pkpass(&[("pass.json", text.as_bytes())])).unwrap(),
            }
        };
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:00+00:00").unwrap();
        let mut passes = vec![
            stored("last-year", Some("2025-09-20T06:40:00+00:00"), None, false),
            stored("card", None, None, false),
            stored("next-week", Some("2026-10-06T08:00:00+00:00"), None, false),
            stored(
                "this-morning",
                Some("2026-09-29T06:00:00+00:00"),
                None,
                false,
            ),
            stored(
                "expired",
                Some("2026-10-01T08:00:00+00:00"),
                Some("2026-09-28T00:00:00+00:00"),
                false,
            ),
            stored("voided", Some("2026-10-02T08:00:00+00:00"), None, true),
            stored("last-month", Some("2026-08-29T06:40:00+00:00"), None, false),
        ];
        order(&mut passes, now);
        let ids: Vec<&str> = passes.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "this-morning",
                "next-week",
                "card",
                "voided",
                "expired",
                "last-month",
                "last-year"
            ]
        );
    }
}
