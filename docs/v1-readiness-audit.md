# Hop v1.0 Readiness Audit

Judgment from the code on `main` at `528ea8c`, plus `docs/performance-audit.md`. No two-Mac hardware run was done for this audit. Percentages below are judgments, not measurements.

## Executive Summary

The move itself is a small, coherent product. One Mac offers a device, the other chooses **Move here**, and the Mac that has the device disconnects only after it accepts. If the take fails, that Mac connects the device again (`src/link.rs` `give_devices`, `src/handoff.rs` `after_release`).

It is not ready for a stranger to install and trust. Failures that are not "Hop is closed" are still described as **Hop isn't running**. Sleep, wake, and a dead network have no code and no recorded hardware pass. Releases are ad-hoc signed, so macOS still asks for **Open Anyway**.

## Current Product Strengths

- The menu states the core facts: **Connected here**, **Connected to {Mac}**, **Move here**, **Moving…**, **Keep on this Mac**, **Unavailable**, **Hop isn't running** (`src/menu_bar.rs`).
- Offering a device is **Let {Mac} move it**. That does not disconnect anything. **Move here** is a separate click on the other Mac.
- A second move while one is running returns busy (`begin_move` in `src/link.rs`). The menu hides other **Move here** rows while one device is **Moving…**.
- Trust is a local click after both menus show the same code (`wire::pair_code`, menu item **Codes match**). Strangers and replayed ids are refused (`src/wire.rs`).
- Identity is created on this Mac and stored mode `0o600` (`src/wire.rs`). There is no account and no cloud client in `src/`.
- CI on `macos-26` runs `cargo fmt --check`, `cargo check`, and `cargo test` (`.github/workflows/ci.yml`).

## Critical Gaps

`PeerUnreachable` is one sentence for many causes. `stayed_notice` always says "Hop isn't running on the other Mac. The devices stayed connected here." (`src/handoff.rs`). `link::claim` returns that reason for a refused TCP connect, a 20s read timeout, a bad seal, and any reply other than `released`, `absent`, `refuse`, `busy`, or `still` (`src/link.rs`). The menu line **Hop isn't running** is a different check: Bonjour has no resolved peer (`menu_peer_line` in `src/main.rs`). A Wi-Fi blip and a Mac that is asleep look the same as Hop being quit. The user cannot tell what to do next.

Bluetooth read failures do not reach that menu. `paired_devices` returns "Hop could not read Bluetooth." (`src/bluetooth.rs`). `list_error` is drawn in the Devices window (`src/main.rs`), and no menu item opens that window. If Bluetooth permission is missing, the menu can simply show no devices.

The holding Mac reconnects in `give_devices` without going through `Hop::report`. The Mac that clicked **Move here** is the one that shows a notice. When `connect_all` fails there, the outcome is `StayedThere`: "The other Mac still has the devices." That is the end state after rollback, not "this Mac could not connect, so it went back."

## Reliability Gaps

There is no sleep or wake handler. `serve` in `src/link.rs` polls Bonjour until the process exits. Whether those sockets still work after sleep is untested. The same is true for a Mac restart, Hop being quit on one side, and Bluetooth coming back late.

Timeouts that do exist: the wanter waits 20s for `want`, the holder waits 70s for `took` and then reconnects, the menu gives up at 90s and reports `PeerUnreachable` even if that thread has not finished (`drive_send` in `src/main.rs`). Dropping the unfinished thread does not stop it.

`NotPaired` has a clear sentence in `stayed_notice`, but the live `want` path never sends `unpaired`. `wire::reply` ignores the paired set. A device that is not paired on the destination fails inside `connect_all` and is reported as `StayedThere`.

No retry button. After the notice, **Move here** comes back. That is enough once the sentence is true.

## UX Gaps

The happy path is readable. **Moving…** stays up until the move thread finishes, and the menu does not say the device has moved before that.

**Codes match** is the whole first-run trust step. The row shows the code and the other Mac's name. The README says to compare them. A rename is not justified. A person with both screens can do it. A person who allows a code they did not compare has trusted that Mac, and there is no menu item to undo `peers.json`.

**Allow** on the menu bar is only the waiting state. It does not say which permission macOS wants. Local Network denial is indistinguishable from the other Mac being gone.

The window still has **Let it move**. The menu, which is the product, says **Let {Mac} move it**. The README matches the menu.

## Distribution Gaps

`Cargo.toml` is `0.2.0`. `script/package-macos.sh` ad-hoc signs unless `APPLE_SIGNING_IDENTITY` is set. `script/notarize-macos.sh` notarizes only when Developer ID secrets are present. They are not in CI. The README correctly tells people to use **Open Anyway** or `xattr`. There is no checksum, no Homebrew cask, and no release workflow. A stranger cannot install Hop the way they install other Mac utilities.

## Testing Gaps

Unit tests cover the decisions that must not disconnect early: unreachable ask, failed take reconnects, failed release reconnects, stranger and bad signature refused, replay refused, both Macs compute one code (`src/handoff.rs`, `src/wire.rs`). One link test checks that a short socket read waits for the rest of the frame.

There is no test that boots two peers, and no recorded run of sleep, wake, or a disappearing device. Those need two Macs. Do not treat the unit tests as that evidence.

## OSS / Support Gaps

README, CONTRIBUTING, SECURITY, and CHANGELOG exist. SECURITY.md says to open a private advisory and not to paste Bluetooth addresses or `~/Library/Application Support/Hop`. There is no issue template and no troubleshooting page. A stranger whose move failed sees one sentence that may name the wrong cause, and has no next step written down except the README's "the other Mac needs to be awake, on the same network, and running Hop."

## Competitor Patterns Worth Adopting

Pattern: Launch at login for a menu-bar helper.
Why it matters: If Hop is not open, the other Mac can only say **Hop isn't running**.
Does Hop need it: Yes, after the failure text is honest.
Priority: P1

Pattern: One obvious recovery for a failed action.
Why it matters: **Move here** already returns, but only if the notice is true.
Does Hop need it: The sentence first. A separate Try again control is not needed.
Priority: P0 for the sentence, not a new button

Pattern: A way to drop a trusted peer.
Why it matters: **Codes match** writes `peers.json` with no reverse action (`link::allow`).
Does Hop need it: Yes, once, for a wrong allow.
Priority: P1

Pattern: Signed, notarized download.
Why it matters: Gatekeeper is the first thing a new user hits.
Does Hop need it: Yes, before calling it public.
Priority: P0

## Competitor Features We Should NOT Copy

- A Bluetooth manager: battery, rename, forget-device, audio routing (AirBuddy, ToothFairy).
- Move All, location rules, and automatic switching.
- More than two Macs. `Crowd` already refuses that case in one sentence.
- Accounts, cloud sync, telemetry, a custom updater.

## P0 — Required for v1.0

1. Say what actually failed.
   - Code: `stayed_notice` for `PeerUnreachable`; `claim` maps connect errors, timeouts, and bad replies onto that one reason.
   - Now: the menu says Hop is not running.
   - Impact: the user quits and reopens Hop, or does nothing, for a network or timeout failure.
   - Change: separate "Hop isn't running", "the other Mac did not answer", and "Bluetooth could not be read", and show the Bluetooth one in the menu, not only the hidden window.
   - Risk: low if the new sentences use the existing `StayReason`s and the menu notice. Do not change when a disconnect is allowed.

2. Run the manual matrix below on two Macs. Fix only a failure that shows up.
   - Code: no wake handler, no hardware notes in the repo.
   - Now: sleep, wake, and a dead network are guesses.
   - Impact: a shipped v1 that drops a keyboard after sleep would be the product failing.
   - Change: record pass or fail. Add a Bonjour restart only if wake leaves the peer stuck on **Hop isn't running** while Hop is open.
   - Risk: a speculative reconnect loop can flap the peer. Do not add one first.

3. Notarized GitHub release.
   - Code: ad-hoc `codesign` in `script/package-macos.sh`. Notarization is optional in `script/notarize-macos.sh`.
   - Now: every install hits Gatekeeper.
   - Impact: most people stop at "could not verify".
   - Change: Developer ID, notarize, staple, one zip, checksum in the release notes. No custom updater.
   - Risk: secrets and Apple's notarization queue. The app behavior stays the same.

## P1 — Strongly Recommended

1. Launch at login. The other Mac is useless while Hop is not running. One checkbox is enough. No other settings.
2. Remove a trusted Mac. `allow` appends to `peers.json` and nothing in the menu deletes it.
3. A GitHub bug template: macOS version, the menu sentence, whether the device was still usable. Tell people not to paste addresses, matching SECURITY.md.
4. Checksums next to the zip once releases are notarized.

## P2 — Later

- A capture of the current menu. The README already says one is missing.
- A keyboard shortcut for **Move here**.
- Choosing among more than two Macs. Today `Crowd` tells the user to use one other Mac, which is the right v1 limit.

## Explicitly Out of Scope

- Move All
- Battery, device details, and a Bluetooth settings UI
- Accounts, cloud, telemetry, subscriptions
- Location or focus rules
- A custom update client
- Rewriting the 250ms tick. `docs/performance-audit.md` measured idle CPU at about 0–0.1% and kept the tick because it is the click and the end of **Moving…**. That conclusion still matches the code.

## Recommended v1.0 Roadmap

1. Split the failure sentences so **Hop isn't running** is only used when the other Mac is actually absent.
2. Show a Bluetooth read failure in the menu.
3. Run the two-Mac matrix. Patch only what fails, including sleep if the peer does not return.
4. Notarize the release and publish one zip with a checksum.
5. Launch at login, and a way to forget the other Mac.

## Manual Release Test Matrix

Not run for this audit. Unit tests are not a substitute.

| Case | What to watch |
| --- | --- |
| Both Macs awake, device connected here | **Let {Mac} move it**, then **Move here**, then **Moving…**, then **Connected here** on the other Mac |
| Other Mac quits Hop | **Hop isn't running**. The device stays connected here |
| Other Mac asleep, then awake | Peer line returns without restarting Hop |
| This Mac sleeps, then wakes | Same |
| Both sleep, then wake | Devices and the peer line match reality |
| Wi-Fi off during **Moving…** | Device still usable on the Mac that had it. The sentence is not **Hop isn't running** unless Hop is actually gone |
| Bluetooth off, or permission denied | The menu says Bluetooth could not be read |
| Device powered off during the move | Rollback or a clear stuck state. No silent disconnect |
| **Move here** twice, quickly | Second click does not start a second move. Notice is the busy sentence |
| Quit Hop on one Mac and open it again | The allowed Mac is remembered. Devices match Bluetooth |
| Restart one Mac | Same |

## Failure map

| Reason | Cause in the current code | What the user sees | Rollback |
| --- | --- | --- | --- |
| `PeerUnreachable` | No route, timeout, bad reply, 90s UI give-up | "Hop isn't running…" | Nothing disconnected on the ask path. A late thread can still be in `claim` |
| `NeedsAllow` | Peer visible, not allowed (`Seen::Nearby`) | "Allow the other Mac, on both Macs…" | Nothing disconnected |
| `Crowd` | More than one allowed peer | "More than one other Mac…" | Nothing disconnected |
| `Refused` | Reply `refuse` | "The other Mac has not allowed this Mac…" | Nothing disconnected |
| `NotPaired` | Reply `unpaired` on the old ask path | Pair-it-there sentence | Not sent by the live `want` path |
| `StillHere` | Disconnect did not stick (`after_drop`) | "The devices stayed on this Mac." | Stays here |
| `Busy` | `begin_move` already held, or reply `busy` | "Hop is already moving a device." | Nothing new disconnected |
| `StayedThere` | Reply `still`, or this Mac's `connect_all` failed | "The other Mac still has the devices." | Holder reconnects on `still` and on a missing `took` |
