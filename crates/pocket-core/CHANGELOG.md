# Changelog

All notable changes to `pocket-core` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The Pocket
application keeps its own changelog at the repository root.

## [Unreleased]

## [1.1.1]

### Fixed

- `PassStore::add` no longer takes a pass that names a serial number and no
  type identifier (or the reverse) for another issuer's pass with the same
  half. A serial number is unique only within its type, so such a pass has
  no identity to be matched by: adding the second replaced the first. It is
  now one pass only byte for byte, as a pass naming neither already was, and
  is found by its bytes whatever its folder is called.

## [1.1.0]

### Added

- `PassStore::add` stores a pass from the bytes of its `.pkpass`. The bytes
  are verified first — the same reader, manifest check and archive limits as
  every later read — and written verbatim through `cosmic_pim_core::atomic`,
  so a crash mid-add leaves the store as it was. A pass is one pass by its
  `passTypeIdentifier` and `serialNumber`: adding one the store already holds
  replaces the stored copy where it is, and `Added` says whether the pass was
  `New`, `Updated` or `Unchanged`. A pass naming neither identifier is
  deduplicated by its bytes. The folders the store creates are `0700`.
- `PassStore::remove` removes a pass by id: its `.pkpass`, and its folder when
  that was all the folder held. An id that is not a single folder name is
  refused.
- `AddError` and `RemoveError`, both `#[non_exhaustive]`.
- `pkpass::split` returns the passes a file holds: itself for a `.pkpass`,
  each entry for a `.pkpasses` bundle (what an airline sends for several
  travellers). The bundle is read within the same limits as a pass, and each
  pass it yields is verified within them again when it is read or added.

### Changed

- Depends on `cosmic-pim-core` 2, for its atomic writer.

## [1.0.1]

### Changed

- `PassStore::list` puts past passes last: upcoming passes first, soonest at
  the top, then undated ones, then voided, expired and past passes, most
  recent first. A pass with no expiry counts as past a day after its relevant
  date. Strict date order put last year's flights above today's.

### Fixed

- Pass dates without seconds (`2014-12-05T09:00-08:00`, the form Apple's own
  examples use) are read. They came back as `None`.
- A `.pkpass` is read within limits: at most 1024 entries, 16 MiB per entry
  and 64 MiB for the whole archive, compressed or decompressed. An archive
  whose header claimed a huge size aborted the process on allocation, and a
  deflate bomb exhausted memory; both are now an `Error::Read` whose source
  has kind `FileTooLarge`, and `PassStore::list` reports the pass as
  unreadable instead of dying with it. The store reads a file only up to the
  archive limit.
- `PassStore::list` reports everything in the store's root that is not a
  readable pass: a folder without `pass.pkpass`, a file lying loose in the
  root, and a directory entry that could not be read. They were skipped
  silently. Hidden entries are still ignored.

## [1.0.0] - 2026-09-10

First release.
