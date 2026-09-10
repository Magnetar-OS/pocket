# Pocket

A wallet for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch), built
with [libcosmic](https://github.com/pop-os/libcosmic).

Boarding passes, event tickets, loyalty and membership cards, coupons — held as
the `.pkpass` files they arrived in, and presented with their barcodes ready to
scan.

**This is early.** The pass model, the PKPass reader and the on-disk store work
and are tested, and a pass can now be scanned off the screen: all four PassKit
symbologies render, and there is a full-screen presenter. Passes still have to
be put in the directory by hand, and their signatures are still not verified;
see [Status](#status) for the honest line and [ROADMAP.md](ROADMAP.md) for the
rest.

## Part of a family

Pocket is deliberately small, because most of what a wallet appears to need
already exists on this desktop and is owned by something that does it properly.

| It needs | Owned by | So this repo |
|---|---|---|
| A vault, and the system secret store | [Locket](https://github.com/entro314-labs/locket) | is a Secret Service client. It writes no crypto. |
| Accounts, sync, crash-safe storage | [cosmic-pim](https://github.com/Magnetar-OS/cosmic-pim) | links the substrate. |
| Showing and reminding you about a dated thing | [Slate](https://github.com/Magnetar-OS/slate) | writes events to the shared vdir. |
| Mail, and parsing it | [Envelope](https://github.com/Magnetar-OS/envelope) | reads the maildir Envelope already syncs. |

What is left is passes: what one *is*, how a `.pkpass` is read and verified, and
how a barcode is put on screen so a reader can scan it.

[ARCHITECTURE.md](ARCHITECTURE.md) is the canonical description of how the
layers fit, which integration costs what, and what is architecturally out of
reach. Read it before adding anything.

## Three strands, separated by who holds the secret

**Passes and tickets** are stored here, byte for byte. A `.pkpass` is signed
over its own bytes, so anything that rewrites one destroys its signature; the
file is the source of truth and the model is derived on every read.

**Cards and documents** are split. The card face — issuer, colours, barcode — is
a pass and lives here. The numbers that authenticate you go to Locket over
`org.freedesktop.secrets`, because Locket already owns that bus name and has the
vault, the TPM and FIDO2 key slots, and the lock-on-suspend behaviour. One vault
per machine.

**Payments** need no code here. Because cards live in Locket's vault, autofilling
one into a checkout form is a feature of Locket's existing browser extension and
its native messaging host, over data that is already where that extension looks.
Pocket ships no second extension.

## Status

Working and tested:

- `.pkpass` reading — the archive, `pass.json`, all five styles, both the current
  `barcodes` array and the pre-iOS-9 singular `barcode` key
- Manifest verification in **both** directions: every file the manifest names is
  present with the digest it claims, and no file in the archive is unaccounted
  for. A truncated download and content appended after signing both fail.
- The pass store: one directory per pass holding the original archive, sorted
  soonest-relevant first, with unreadable passes reported rather than dropped
- **Barcodes, all four symbologies** — PDF417 and Aztec for airlines and rail,
  QR for events and loyalty, Code 128 for older retail — drawn at whole-pixel
  module sizes, black on white whatever the theme is, with the quiet zone the
  specification asks for. `messageEncoding` is obeyed rather than assumed, so a
  Latin-1 payload scans back as Latin-1; an encoding that cannot be produced is
  refused rather than guessed at.
- **A presenter**: the barcode full-screen on white, with idle inhibited
  through the XDG portal and the backlight raised through
  `com.system76.CosmicSettingsDaemon` for as long as it is up, and both given
  back on the way out. Escape leaves.
- **The pass face**: the issuer's colours and logo text, and the field layout
  each of the five styles specifies — a boarding pass shows origin and
  destination side by side, and the back of the pass is shown below.
- A COSMIC application that lists passes by style and shows the selected one

Not done, and not pretended otherwise:

- **The PKCS#7 signature is not verified.** Manifest digests catch corruption;
  they do not prove a pass came from the issuer it names. The WWDR chain is
  public so the check is implementable — a function returning "valid" without
  doing it would be worse than its absence.
- **There is no import.** A `.pkpass` has to be copied into the pass directory
  by hand; opening one from the file manager or a dialogue needs a crash-safe
  writer, and that writer already exists as `cosmic_pim_core::atomic`. It
  arrives with the substrate dependency rather than being written a second time
  here.
- No extraction from mail or PDFs, no calendar writing, no Locket client, no
  applet, no launcher or `peek` plugin, no packaging.

It runs on one machine, its author's. Nobody has reviewed it and no distribution
ships it.

## Try it

```sh
just run-sandboxed
```

That builds and runs against `/tmp/pocket-passes` via `POCKET_PASS_DIR`,
so it leaves the real wallet alone. Drop a `.pkpass` into
`/tmp/pocket-passes/<any-name>/pass.pkpass` and restart.

## Building

The toolchain is pinned in `rust-toolchain.toml`.

```sh
just check-all       # fmt, clippy -D warnings, tests, metadata validation
just build-release
sudo just install
```

Packagers should use `just vendor` when making the source tarball,
`just build-vendored` in the build chroot, and
`just rootdir=${DESTDIR} install`.

## Layout

```
src/                  the COSMIC front end — app shell, pass face, barcode
                      drawing, presenter, screen state, i18n
crates/
  pocket-core      the pass model, the PKPass reader, the barcode encoder,
                      the store (MPL-2.0,
                      no toolkit dependency, expected to graduate into
                      cosmic-pim as cosmic-pim-pass)
resources/            desktop entry and metainfo templates, icons
i18n/<lang>/          one Fluent catalogue per locale
build.rs              generates the desktop entry and metainfo from the
                      catalogue, so the app's name is translated in the
                      applications menu and the software centre too
justfile              build, install and metadata checks — the packager's path
cosmic-conventions.md what the COSMIC repositories agree on, read off source
```

## Licence

The application is **GPL-3.0-only**. `pocket-core` is **MPL-2.0**, because it
is shaped to move into the cosmic-pim substrate. See [LICENSING.md](LICENSING.md).
