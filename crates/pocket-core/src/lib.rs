// SPDX-License-Identifier: MPL-2.0

//! The pass model, the PKPass reader, and the on-disk store.
//!
//! No toolkit types and no user interface, on the same principle that keeps
//! `cosmic-pim-core` and `peek-engine` free of them: a second front end — a
//! launcher plugin, a `peek` previewer, a CLI — must be able to link this
//! without linking libcosmic.
//!
//! This crate is [MPL-2.0] while the application above it is GPL-3.0-only,
//! because it is shaped to move into the `cosmic-pim` substrate as
//! `cosmic-pim-pass` once a second application wants it. See `LICENSING.md`.
//!
//! # What is deliberately not here
//!
//! **Secrets.** A pass carries an `authenticationToken` for its issuer's
//! update web service, and a card carries numbers that authenticate their
//! holder. Neither belongs in this crate's model, and neither belongs in a
//! vault this project writes: Locket already owns `org.freedesktop.secrets`
//! on this desktop. So [`Pass`] has no token field, and reading one is a
//! separate, explicit call — [`pkpass::authentication_token`] — whose only
//! caller is the code handing it to the Secret Service.
//!
//! **A writer.** Importing a pass has to be crash-safe, and the crash-safe
//! writer already exists in `cosmic_pim_core::atomic`. Rather than keep a
//! second copy here, the store reads; the write path arrives with the
//! substrate dependency. See `ARCHITECTURE.md`.
//!
//! [MPL-2.0]: https://www.mozilla.org/en-US/MPL/2.0/

pub mod barcode;
pub mod model;
pub mod pkpass;
pub mod store;

pub use barcode::{Error as BarcodeError, Symbol};
pub use model::{Barcode, BarcodeFormat, Field, Pass, PassKind, TransitType, parse_color};
pub use pkpass::Error as PkPassError;
pub use store::{Listing, PassStore, StoredPass, UnreadablePass};
