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

`cargo test` covers the move: a Mac that does not answer disconnects nothing, and a failed take connects the device again here.
