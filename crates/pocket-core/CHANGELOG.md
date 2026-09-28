# Changelog

All notable changes to `pocket-core` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The Pocket
application keeps its own changelog at the repository root.

## [Unreleased]

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
