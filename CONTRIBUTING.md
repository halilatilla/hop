# Contributing

Hop is a menu bar app for two Macs. Small changes that keep a move reliable are welcome.

## What you need

- Apple silicon Mac, macOS 11 or later
- [Xcode](https://developer.apple.com/xcode/) and the command line tools
- Rust 1.98.1, pinned in `rust-toolchain.toml`

```sh
xcode-select --install
cargo test
cargo run --release
```

The first build compiles GPUI from the Zed repository. This repo does not fork Zed.

## Where things live

- `src/bluetooth.rs` reads paired devices and connects or disconnects one
- `src/link.rs` finds the other Mac and runs a move
- `src/handoff.rs` decides whether a move may disconnect anything
- `src/wire.rs` seals the messages between the Macs
- `src/menu_bar.rs` is the menu
- `src/main.rs` is the menu state and the device list

A move asks the Mac that has the device before anything disconnects. If the other Mac does not take it, Hop connects it again on the Mac that had it. Keep that.

## Tests

`cargo test` covers the move: a Mac that does not answer disconnects nothing, and a failed take connects the device again here. A second move is ignored while one is already running.

These need two Macs, so they are manual:

- The other Mac is off the network, then comes back
- Wi-Fi changes while both are running
- A device disappears during a move
- One Mac sleeps and wakes
- Quit Hop and open it again, on one Mac and on both
- Start a move, then try to move another device before it finishes

The menu should keep saying **Moving…** until the other Mac has the device. If the move fails, the device should still be usable on the Mac that had it.

## Idle

Measured on an Apple silicon Mac, menu closed, for one minute. Before this change: mostly 0–0.2% CPU, spikes under 10%, about 70–81 MB. After Bluetooth is read every 8 seconds while idle: mostly 0–0.3% CPU, a few spikes (one sample at 14%), memory settled near 70–80 MB. The process was running about 0.1 seconds after launch. The 250ms timer checks the menu and a move in progress. Bluetooth is read every second while a move is running.

## Release signing

`script/package-macos.sh` uses an ad-hoc signature unless `APPLE_SIGNING_IDENTITY` is set. Notarization runs only when a Developer ID certificate and an app-specific password are provided to `script/notarize-macos.sh`. Do not commit certificates or passwords.
