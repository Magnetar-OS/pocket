# Changelog

All notable changes to Pocket are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Adding a pass that carries a serial number but no pass type identifier no
  longer replaces another issuer's pass that happens to have the same serial
  number. Two cards both numbered 1 are two cards.
- Adding several passes at once says what happened to each: how many were
  added, how many replaced an earlier version, and how many were already in
  the wallet. It said all of them were added.
- A pass opened from the file manager and then added by dropping the same
  file on the window, or choosing it in the dialog, is listed once. It stayed
  in the list twice: as the opened file, and as the pass in the wallet.
- A file that could not be read is named until the next files are opened or
  added. The line stayed above the list for as long as Pocket ran.
- Escape closes the "Remove from your wallet?" question, leaving the pass.
  It did nothing there.
- The count above the list is of the passes in the wallet. A pass opened
  from a file and not yet added was counted with them.

## [1.3.1] - 2026-10-03

### Changed

- Rebuilt against the current COSMIC libraries (libcosmic `5a8bd94`).

## [1.3.0] - 2026-09-30

### Added

- **Add to wallet.** A pass opened from the file manager is shown with an
  "Add to wallet" button beside it; pressing it keeps the pass. It was shown
  and then forgotten, and keeping one meant copying the file into the passes
  folder by hand.
- **Add pass…** in the header opens a file dialog and adds the `.pkpass` files
  chosen there. A file that is not a pass is listed by name with the reason,
  and does not stop the others.
- Dropping `.pkpass` files on the window adds them, from a file manager or
  — through the document portal — from a sandboxed application.
- `.pkpasses` bundles, which airlines send for a booking with several
  travellers, open and add as every pass they hold. Pocket is offered for
  them in "Open With" too. Each pass in a bundle is held to the same size
  limits as a single `.pkpass`.
- **Remove from wallet**, under a stored pass, deletes it after asking.
- Adding a pass the wallet already holds does not add a second copy. A pass is
  the same pass when its issuer's type identifier and its serial number match:
  the identical file is reported as already there, and a newer version — a
  re-sent boarding pass with a new gate — replaces the stored one.
- "Open with Pocket" on another pass while Pocket is running shows that pass
  in the window that is already up, selected and ready to present, instead of
  starting a second window. If a barcode is on the whole screen at the time,
  the new pass joins the list without taking its place.

### Changed

- Rebuilt against the current COSMIC libraries (libcosmic `6af8b70`).
- Passes are added through the suite's crash-safe writer
  (`cosmic_pim_core::atomic`), and checked first with the same reader and the
  same size limits as every later read: a pass is in the wallet whole, or not
  at all. The folders Pocket creates for them are readable by you alone.
- Built against `pocket-core` 1.1.0.

## [1.2.0] - 2026-09-29

### Added

- "Open with Pocket" from a file manager shows the chosen `.pkpass` at the
  top of the list, selected and ready to present, marked as opened from a file
  rather than kept in the wallet. The file was ignored and Pocket opened on
  the existing list. A file that cannot be read is listed with the reason.
- An expired pass says "Expired" on its face and in the list, and a voided one
  says so in the list as well as on its face.

### Changed

- Rebuilt against the current COSMIC libraries (libcosmic `03d7dcb`).
- Past passes sink to the bottom of the list: upcoming passes come first,
  soonest at the top, then cards without a date, then voided, expired and
  past passes, most recent first. Last year's flights sat above today's.

### Fixed

- Passes whose dates carry no seconds — the form Apple's own examples use,
  such as `2014-12-05T09:00-08:00` — keep their relevant date and expiry. They
  were read as undated, so a boarding pass sorted below the loyalty cards.
- Leaving the presenter before the desktop had answered no longer leaves the
  screen at full brightness and kept awake, and presenting again quickly no
  longer loses track of the brightness to restore.
- One oversized or hostile `.pkpass` — a header claiming a terabyte, or a
  zip bomb — no longer takes the whole wallet down. It is listed as unreadable
  and the other passes load.
- A pass folder without its `pass.pkpass`, or a `.pkpass` saved loose in the
  passes folder, is counted as unreadable instead of silently left out.
- Each pass that could not be read is listed by its folder name with the
  reason, under the count. Only the count was shown, so there was no telling
  which boarding pass was broken.

## [1.1.0] - 2026-09-22

### Changed

- Rebuilt against the current COSMIC libraries (libcosmic `03c8f93`).

## [1.0.2] - 2026-09-16

### Fixed

- Packages are built. The v1.0.1 release stopped at the pipeline's formatting
  check before producing any, so it has no downloads; this is the first
  Pocket release with packages.

## [1.0.1] - 2026-09-16

### Added

- Packages. Pocket is built as a `.deb`, `.rpm` and Arch package on every
  release and published to the `[magnetar]` pacman repository. v1.0.0 was
  tagged without a release pipeline, so this is the first version anyone can
  install without building it.

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

[Unreleased]: https://github.com/Magnetar-OS/pocket/compare/v1.3.1...HEAD
[1.3.1]: https://github.com/Magnetar-OS/pocket/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/Magnetar-OS/pocket/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/Magnetar-OS/pocket/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/Magnetar-OS/pocket/compare/v1.0.2...v1.1.0
[1.0.2]: https://github.com/Magnetar-OS/pocket/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/Magnetar-OS/pocket/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/Magnetar-OS/pocket/releases/tag/v1.0.0
