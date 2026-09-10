# pocket-core

The pass model, the `.pkpass` reader, and the pass store behind
[Pocket](https://github.com/Magnetar-OS/pocket), a wallet for the COSMIC
desktop.

No toolkit dependency: this crate knows what a pass *is*, not how to draw one.
That split is what lets a second front end — or a headless caller — use it.

## What it does

- **The model** — a pass, its style, its fields, its colours and its barcodes,
  parsed from the PassKit JSON issuers actually write rather than from the
  subset the specification suggests.
- **The reader** — opens a `.pkpass` archive, validates every file against the
  signed `manifest.json` by SHA-1 digest, and rejects a pass whose bytes were
  appended to or altered after signing.
- **The store** — an on-disk directory of passes, read without hiding a corrupt
  neighbour behind an error.
- **The barcodes** — all four symbologies PassKit defines (QR, Aztec, PDF417
  and Code 128), encoded with the character set `messageEncoding` names, quiet
  zone included.

## What it does not do

It does not verify the pass **signature**. The manifest digest proves the
archive is internally consistent; it does not prove who signed it. Treat a pass
read by this crate as unauthenticated until that lands.

It writes no crypto and holds no secrets — the application handles that through
the Secret Service.

## Licence

MPL-2.0. The Pocket application is GPL-3.0-only and links this crate rather
than absorbing it, because this crate is shaped to graduate into the
[cosmic-pim](https://github.com/Magnetar-OS/cosmic-pim) substrate as
`cosmic-pim-pass` once a second application wants it.
