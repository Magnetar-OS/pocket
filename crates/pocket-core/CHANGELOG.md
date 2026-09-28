# Changelog

All notable changes to `pocket-core` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The Pocket
application keeps its own changelog at the repository root.

## [Unreleased]

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

## [1.0.0] - 2026-09-10

First release.
