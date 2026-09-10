# Licensing

**The Pocket application is GPL-3.0-only.** That is the licence of `src/`
and of the binary this repository produces.

## The two-licence arrangement

| Layer | Licence |
|---|---|
| This repository's application — `src/`, and the `pocket` binary | **GPL-3.0-only** |
| `crates/pocket-core` — the pass model, the PKPass reader, the store | **MPL-2.0** |
| [cosmic-pim](https://github.com/Magnetar-OS/cosmic-pim), the shared substrate | **MPL-2.0** |

This mirrors what Slate, Circle and Envelope have with cosmic-pim, and for the
same reason: MPL-2.0's boundary is the *file*. Modify a file in the substrate
and you publish that file; linking it imposes nothing on your own code. That
lets one implementation be shared by several applications — including
consumers that are not GPL — while improvements to it stay public.

MPL-2.0 is a "Secondary Licence" under its own §3.3, so GPL-3 absorbs it: the
binary distributed from this repository is GPL-3 as a whole, and the MPL'd
files remain MPL for anyone who extracts them.

**The trap.** MPL-2.0 Exhibit B ("Incompatible With Secondary Licenses") turns
that compatibility off. If it ever appears on a file in `pocket-core` or in
cosmic-pim, this application can no longer legally link it. Headers here are
Exhibit A only:

```rust
// SPDX-License-Identifier: MPL-2.0
```

## Why `pocket-core` is MPL-2.0 rather than GPL, from the first commit

Because it is expected to move. The suite's test for substrate-shaped code —
*would a second application want it?* — is already answered yes for the pass
model: Envelope wants it to offer "Add to wallet" on a `.pkpass` attachment,
and `peek` wants it to preview one. When that second caller lands, the crate
graduates into cosmic-pim as `cosmic-pim-pass`, which is MPL-2.0.

Choosing that licence now makes the move a `git mv`. Choosing GPL now and MPL
later would mean asking every contributor by then for permission to relicense —
a conversation that gets harder every month and has sunk this kind of move
before.

## The files in this repository

`LICENSE` is the GPL-3.0 text — the licence of the application and of the
binary. `LICENSE.MPL-2.0` is the Mozilla Public License 2.0, shipped beside it
because that binary statically links MPL-2.0 files and a recipient is entitled
to their terms. `NOTICE` carries the attribution obligations that travel with
the binary.

## Upstream obligation worth repeating

The canonical rationale for the arrangement, the provenance of the substrate's
borrowed code, and the outstanding obligations are at
[cosmic-pim/LICENSING.md](https://github.com/Magnetar-OS/cosmic-pim/blob/main/LICENSING.md).

One item from it once blocked distribution and applies the moment this
repository takes a dependency on the substrate: parts of cosmic-pim derive from
[Meltemi](https://github.com/entro314-labs/meltemi), which carries no
whole-repository licence and instead grants MPL-2.0 (Exhibit A only) on each
donor file. A future port must extend that grant table in the same commit.

## No claim over PassKit

Pocket reads Apple's PKPass format. The format is documented publicly and
this is a clean-room reader — no Apple code, no Apple certificates, and no
ability to *create* a signed pass, which requires a certificate only Apple
issues. "Apple", "Apple Wallet", "PassKit" and "Apple Pay" are Apple's
trademarks, used here only to say what this software reads.
