# Architecture

Where Pocket sits in the COSMIC application family, and — more usefully —
what it is *not allowed* to build because something in the family already owns
it.

## The rule this repository is organised around

Three of the four things a wallet appears to need already exist on this
desktop, each owned by a project that does it properly:

| Need | Already owned by | So Pocket |
|---|---|---|
| An encrypted vault, and the system secret store | **Locket** — owns `org.freedesktop.secrets`, the Secret portal backend, PAM, the SSH agent | is a Secret Service *client*. It writes no crypto. |
| Accounts, credentials, OAuth, sync, crash-safe storage | **cosmic-pim** | links the substrate rather than reimplementing it. |
| Showing a dated thing, and reminding you about it | **Slate** | writes events into the shared vdir. Slate needs no code. |
| Getting mail, and parsing it | **Envelope** / `cosmic-pim-mail` | reads the maildir Envelope already syncs. It opens no IMAP socket. |

What is left — and it is the whole product — is **passes**: what one is, how a
`.pkpass` is read and verified, how a barcode is presented so a reader can
scan it, and where the extraction pipeline turns a confirmation email into
something a person can hold up at a gate.

## The shape

```
                         ┌──────────────────┐
                         │    Pocket     │  application (GPL-3.0-only)
                         │  passes · cards  │  this repo, src/
                         │    documents     │
                         └────────┬─────────┘
                                  │
                     ┌────────────▼────────────┐
                     │     pocket-core      │  MPL-2.0, crates/
                     │  model · pkpass · store │  no toolkit dependency
                     └────────────┬────────────┘
                                  │
        ┌─────────────────┬───────┴────────┬──────────────────┐
        │                 │                │                  │
┌───────▼──────┐  ┌───────▼──────┐  ┌──────▼───────┐  ┌───────▼──────┐
│    Locket    │  │  cosmic-pim  │  │    Slate     │  │   Envelope   │
│ Secret Svc.  │  │  substrate   │  │ shared vdir  │  │   maildir    │
│  (secrets)   │  │ (accts, I/O) │  │  (events)    │  │  (sources)   │
└──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘
```

Three of those four are reached over an interface that already exists and is
already served, so none of them has to know Pocket exists. That is the
point: an integration that requires a patch to another repository is an
integration that rots.

## Why `pocket-core` is MPL-2.0 while the app is GPL-3.0-only

The same two-licence arrangement Slate, Circle and Envelope have with
cosmic-pim, for the same reason, and with one addition: `pocket-core` is
expected to *move*.

The suite's own test for substrate-shaped code is in
[cosmic-pim/00-suite.md](https://github.com/Magnetar-OS/cosmic-pim/blob/main/00-suite.md):
*would a second application want it?* For the pass model the answer is already
yes — Envelope wants it the moment it shows an "Add to wallet" affordance on a
`.pkpass` attachment, and `peek` wants it to preview one. When that second
caller arrives the crate graduates into the substrate as `cosmic-pim-pass`.
Licensing it MPL-2.0 now makes that a `git mv` instead of a relicensing
conversation with everyone who has contributed by then.

See [LICENSING.md](LICENSING.md) for what the arrangement means for the
binaries this repository produces.

## The three strands, and where each one's data lives

Pocket covers all three readings of "a wallet", and they are not three
features bolted together — they are three shapes of the same object, separated
by *who holds the secret*.

### 1. Passes and tickets

Boarding passes, event tickets, coupons. They arrive as `.pkpass` files or as
PDFs, they have a time, and they are presented as a barcode.

**Stored here**, verbatim, under `$XDG_DATA_HOME/pocket/passes/<id>/pass.pkpass`.

The verbatim rule is inherited from cosmic-pim's *server bytes are stored
verbatim* invariant, and it binds harder here. cosmic-pim stores VEVENT text
untouched because a model covers less than the format carries. A `.pkpass` is
**signed over its own bytes**, so re-serialising one does not lose an `X-`
property — it destroys the signature, irreversibly. So the file is the source
of truth and `Pass` is derived on every read.

### 2. Cards and documents

Loyalty and membership cards, payment-card records, IDs, certificates.

**Split deliberately.** The card *face* — issuer, name, colours, the barcode —
is a pass and lives here. The numbers that authenticate the holder go to
Locket over `org.freedesktop.secrets`, because Locket already owns that bus
name, already has the Argon2id + XChaCha20-Poly1305 vault, already has TPM and
FIDO2 key slots, and already locks on session lock and suspend. A second vault
on one machine is not a feature; it is a second thing to get wrong.

The same rule applies to a pass's `authenticationToken` — the credential for
its issuer's update web service. It is why
`pocket_core::pkpass::authentication_token` is a separate call and why
`Pass` has no field for it: no list, render or log line can carry it by
accident.

### 3. Payments

The honest desktop analogue of Apple Pay is **autofilling card details into a
checkout form**, and it needs no new code in this repository. Locket already
ships a browser extension and `locket-nmh`, its native messaging host. Because
strand 2 puts the card in Locket's vault, autofill is a Locket feature over
data that is already where its extension looks.

Pocket contributes the wallet-shaped UI for managing those cards. It does
not ship a second browser extension.

## Where passes come from

The extraction pipeline is the part with the most leverage and the least new
infrastructure, because Envelope already did the hard half.

```
Envelope syncs IMAP/JMAP/Gmail/Graph  →  maildir under $XDG_DATA_HOME/mail
                                              │
                                              │  read-only, no credentials,
                                              │  no second mail engine
                                              ▼
                              pocket extraction pipeline
                                   .pkpass attachment
                                   PDF ticket (barcode + text)
                                   IATA BCBP boarding-pass string
                                   .ics attachment
                                              │
                    ┌─────────────────────────┼──────────────────────┐
                    ▼                         ▼                      ▼
          passes/<id>/pass.pkpass     $XDG_DATA_HOME/calendars   Locket vault
              (this repo)              (Slate shows it, with      (the secrets)
                                        its own reminders)
```

Reading the maildir rather than talking to Envelope is what makes this cheap:
no D-Bus contract to agree, no second copy of a credential, and it works with
Envelope not running. `mbsync` and `notmuch` read the same files for the same
reason.

The reverse direction — Envelope offering "Add to wallet" on an attachment —
is the later, optional half, and it needs `pocket-core` in the substrate
first.

## Integration points, and what each one costs

| Project | Interface | Direction | Needs a change there? |
|---|---|---|---|
| **Locket** | `org.freedesktop.secrets` (libsecret) | Pocket → Locket | No |
| **cosmic-pim** | `cosmic-pim-accounts`, `core::atomic`, `core::ical` | link | No |
| **Slate** | the shared vdir at `$XDG_DATA_HOME/calendars` | file | No |
| **Envelope** | the maildir at `$XDG_DATA_HOME/mail` | file, read-only | No |
| **jump** | an Alfred-style plugin under `share/jump/plugins/` | plugin | No |
| **peek** | a `command` previewer plugin invoking `pocket preview` | plugin | No |
| **grabit** | a declarative action on a selected booking reference | action | No |
| **Magnetar** | one line in `pkgbuilds/apps/apps.txt` | packaging | One line |

None of the first seven requires a patch to another repository. That is not a
coincidence — it is the selection criterion that produced the list.

### The launcher plugin's security shape, decided before the feature

Locket fixed this rule for its own launcher plugin and it applies unchanged
here: **a plugin process is an arbitrary executable running as you**, so it
sees labels only. Matching happens in Pocket, the barcode is rendered by
Pocket, and a pass's fields never cross into the plugin's address space.

## What this cannot do, and will not pretend to

These are architectural, not scheduling.

- **Apple Pay, NFC payments, transit gates, car keys, Express Mode.** They
  require a secure element and payment-scheme membership. No amount of code
  reaches them. A non-goal, not a roadmap item.
- **Creating valid `.pkpass` files.** Signing needs a pass-type certificate
  Apple issues to a developer account. Reading and verifying is unaffected —
  the WWDR chain is public.
- **Push-driven pass updates.** Apple's PassKit web service is plain HTTP and
  is pollable with the token in the pass, so updates work. The *notification*
  that an update exists travels over APNs and needs an Apple push certificate,
  so updates are poll-only and some issuers that require registration first
  will not update at all.
- **Google Wallet passes.** Usually a save-link or a JWT rather than a
  portable file. Mostly out of reach; the PDF many issuers also send is not.
- **EUDI / mDL (ISO 18013-5).** A conformant EU digital identity wallet needs
  a certified secure store and a conformance assessment. That is a different
  product with a different threat model. Explicitly out of scope here.

## Invariants

Each of these exists because getting it wrong loses something.

**A `.pkpass` is never rewritten.** It is signed over its own bytes. Store it
verbatim, derive the model on read, and hand the original bytes back out when
something else wants the file.

**Secrets never enter the pass model.** Not the update token, not a card
number. They go to the Secret Service on an explicit, separate call. A model
that could carry one will eventually be serialised somewhere it should not be.

**An unreadable pass is reported, never dropped.** One corrupt file must not
quietly shrink the wallet — that failure is discovered at a boarding gate.
`PassStore::list` returns what loaded *and* what did not.

**The manifest is checked in both directions.** A file named in the manifest
but absent is a truncated archive; a file present but unnamed is content
appended after signing. Checking only the first direction misses the attack.

**A barcode is drawn in whole modules, dark on light.** The module size is an
integer number of pixels, so every bar is the width the encoder meant; scaling
a symbol to fill a box gives neighbouring modules different widths and changes
the ratio a reader measures the code by. And the issuer's colours stop at the
edge of the barcode — PassKit renders every symbol dark-on-light, and so does
the theme-aware desktop underneath this one, because a dark-mode barcode is an
unscannable barcode.

**The declared `messageEncoding` is obeyed, or the barcode is refused.** The
wrong encoding does not produce a broken barcode; it produces a working one
that scans to a different string. That failure is invisible here and discovered
at a gate, so an encoding that cannot be produced is an error with a reason
rather than a guess.

**The presenter gives back what it borrows.** Idle inhibition and the backlight
level are desktop-wide state. Both are released when the presenter closes, and
the backlight is restored on the blocking path at application exit too, because
a window closed mid-presentation would otherwise leave the screen at maximum
with nothing the user did to connect it to.

**Verification is honest about what it proves.** Manifest digests are checked
today; the PKCS#7 signature is not, so a pass is not yet proven to come from
the issuer it names. That gap is written down here and in the roadmap rather
than hidden behind a function that returns "valid".
