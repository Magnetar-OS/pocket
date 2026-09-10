# Roadmap

Dependency-ordered, not dated. Each milestone says what it is done when, so a
milestone can be argued with rather than merely postponed.

Milestone 1 is done except for import, which the milestone itself defers to the
cosmic-pim dependency in milestone 3. All four symbologies render and scan, the
declared `messageEncoding` is obeyed, there is a full-screen presenter that
holds the screen awake and bright, and a pass is drawn in its issuer's colours
with the layout its style specifies. Everything from milestone 2 down is open.

## Milestone 1 — a pass you can actually use

A wallet whose barcode cannot be scanned is a document viewer. This is the
milestone that makes the application worth opening.

- **Render the barcode.** All four PassKit symbologies — PDF417 and Aztec for
  airlines, QR for events and loyalty, Code 128 for older retail. Rendering
  only QR silently excludes every boarding pass.
- **Honour `messageEncoding`.** It is almost always `iso-8859-1` and it is not
  cosmetic: re-encoding a Latin-1 payload as UTF-8 produces a barcode that
  scans to the wrong string. The model already carries it verbatim.
- **A presenter surface.** Full-screen, maximum contrast, screen blanking and
  dimming inhibited, brightness raised for the duration. This is the one part
  of the UI with a hard external requirement — a gate reader either reads the
  display or it does not.
- **Import.** Open a `.pkpass` from the file manager (the `MimeType=` line is
  already registered) and from a file dialog. Needs the crash-safe writer, so
  it arrives with the cosmic-pim dependency below.
- **The pass face.** Issuer colours, logo text, the field layout each of the
  five styles specifies, and the back of the pass.

Done when: a real airline `.pkpass` opens from the file manager and is scanned
successfully off the laptop screen by a real reader.

**Where this stands.** Rendering, `messageEncoding`, the presenter and the pass
face are implemented, and screenshots of the running application decode back to
the exact message the issuer wrote in all four symbologies — which is the whole
of the criterion above except the reader itself. **Import is not implemented**:
by this milestone's own note it needs the crash-safe writer, and that writer is
`cosmic_pim_core::atomic`, which arrives with the cosmic-pim dependency in
milestone 3 rather than being written a second time here.

## Milestone 2 — verification that means something

Manifest digests catch a corrupt download. They prove nothing about origin.

- **PKCS#7 signature verification** against the Apple WWDR chain, binding the
  `passTypeIdentifier` and `teamIdentifier` in `pass.json` to the certificate
  that signed the manifest. The chain is public; this is implementable.
- **Say which state a pass is in.** Verified, unverified, or failed —
  distinguishable in the UI, because a pass that merely parsed is not the same
  as a pass that is genuine.
- **Expiry and voiding** surfaced rather than inferred: a voided pass is kept
  and shown struck through, never silently deleted. It is a record of
  something that happened.

Done when: a genuine pass verifies, a pass with a tampered `pass.json` fails,
and an expired certificate is distinguishable from an invalid signature.

## Milestone 3 — passes arrive on their own

The leverage milestone, and the one that needs the least new infrastructure
because Envelope already syncs the mail.

- **Depend on cosmic-pim**, for `core::atomic` (the crash-safe writer — do not
  write a second one), `core::ical`, and `cosmic-pim-accounts`.
- **Scan the maildir** at `$XDG_DATA_HOME/mail`, read-only, with no credential
  of its own and no second mail engine. Works with Envelope not running.
- **Extractors**, in order of how often they are the thing in the message:
  `.pkpass` attachments, IATA BCBP boarding-pass strings, PDF tickets (barcode
  plus text), `.ics` attachments.
- **Write the trip to the shared vdir** at `$XDG_DATA_HOME/calendars`. Slate
  then shows it and reminds you about it with no code in Slate. Reuse
  `caldav::itip`'s shape — turning a message into an event is a solved problem
  in this family.
- **Scheduling belongs in the substrate.** Slate's `reminders/` is already
  flagged in cosmic-pim's `00-suite.md` as substrate-shaped code waiting for a
  second caller. This is that second caller; move it rather than copying it.

Done when: a booking confirmation arriving in Envelope produces a pass here and
an event in Slate, without the user doing anything.

## Milestone 4 — cards, and one vault on the machine

- **A Secret Service client** over libsecret, writing to whatever owns
  `org.freedesktop.secrets` — which on this desktop is Locket.
- **Card records**: the face here, the numbers in the vault, joined by
  reference. Payment cards, loyalty numbers, membership and ID documents.
- **The update web service.** Poll the issuer's `webServiceURL` with the token
  from the vault. Poll-only, permanently: the push half runs over APNs and
  needs an Apple certificate. Say so in the UI rather than appearing broken.
- **Autofill is Locket's.** Because the card is in Locket's vault, filling it
  into a checkout form is a feature of Locket's existing extension and
  `locket-nmh`. Pocket ships no second browser extension; the work here is
  agreeing the item shape with Locket.

Done when: a card added here is readable by Locket's own UI and by `secret-tool`,
and locking the vault makes it inaccessible from both.

## Milestone 5 — the desktop notices

Everywhere COSMIC shows or asks for something, the wallet is there.

- **Panel applet**: the next pass. A boarding pass an hour before departure is
  the single highest-value thing a wallet can put in a panel.
- **`jump` plugin**: type a flight number or `pass`, press Enter, the barcode is
  on screen. Security shape fixed before the feature, copying Locket's rule —
  the plugin process sees labels only; matching and rendering happen here.
- **`peek` previewer**: a `command` plugin, `pocket preview %i %o`, so a
  `.pkpass` in a file manager previews as a card. Needs no change to `peek`.
- **`grabit` action**: select a booking reference, add it to the wallet.
- **Notifications** on the events that otherwise happen silently — a pass
  updated by its issuer, a gate change, a pass about to expire.

Done when: none of the four requires a patch to the project it plugs into.

## Milestone 6 — distribution and trust

- **Packaging**: one line in Magnetar's `pkgbuilds/apps/apps.txt`, then AUR,
  Debian and Fedora. Flatpak ships the application only and says so.
- **Release automation**, matching the rest of the family.
- **A threat model in writing.** What holds what, what an attacker with the
  pass directory or a seat on the session bus can and cannot do, and why the
  secrets are somewhere else. The prerequisite for asking anyone to review it.

Done when: it installs from a package, and at least one person who is not the
author keeps their boarding passes in it.

## Non-goals

- **Apple Pay, NFC payments, transit gates, car keys, Express Mode.** They need
  a secure element and payment-scheme membership. Not deferred — unreachable.
- **Creating `.pkpass` files.** Signing needs a certificate only Apple issues.
- **Push-driven updates.** APNs needs an Apple push certificate. Polling is the
  whole of what is available and it is enough.
- **A second vault.** Locket owns the secret store on this desktop.
- **A second browser extension.** Locket already has one.
- **A sync service.** The pass directory is files. Syncthing, a git repo, or
  the substrate's own sync when it grows a store for this.
- **EUDI / mDL (ISO 18013-5).** A conformant EU identity wallet needs a
  certified secure store and a conformance assessment. Different product,
  different threat model.
- **X11.** Wayland-native via libcosmic, like the rest of the family.
