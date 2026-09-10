# Changelog

All notable changes to Pocket are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] — 2026-09-10

First release. The pass pipeline works end to end: a `.pkpass` on disk becomes
a pass on screen whose barcode a reader can scan.

### Added

- **The pass model** (`pocket-core`) — a pass, its style, its fields, its
  colours and its barcodes, read from the PassKit JSON issuers actually write.
  The legacy singular `barcode` key is still honoured, and a pass with no style
  is an error rather than a silent default.
- **The `.pkpass` reader** — validates every file in the archive against the
  signed `manifest.json` by SHA-1 digest. A file appended after signing, or a
  file whose bytes were altered, is rejected.
- **The pass store** — an on-disk directory read without letting one corrupt
  pass hide its neighbours.
- **Barcodes** — all four symbologies PassKit defines (QR, Aztec, PDF417,
  Code 128), encoded in the character set `messageEncoding` names, with the
  quiet zone treated as part of the symbol. The test suite scans each symbol
  back and compares the string, rather than asserting on module counts.
- **The application** — a libcosmic front end with a filterable pass list and a
  full-screen presenter that inhibits idle and raises the backlight while a
  barcode is on screen, through the XDG portal and the COSMIC settings daemon.
- **Translation** — the desktop entry and AppStream metainfo are generated from
  the Fluent catalogues, so the name and summary are translated in the
  applications menu and the software centre.

### Known limitations

- **Pass signatures are not verified.** The manifest digest proves the archive
  is internally consistent; it does not prove who signed it. Treat a pass as
  unauthenticated until this lands.
- Passes must be placed in the store directory by hand — there is no import
  yet.

See [ROADMAP.md](ROADMAP.md) for what comes next.

[1.0.0]: https://github.com/Magnetar-OS/pocket/releases/tag/v1.0.0
