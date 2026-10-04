# Hop

Hop sends the Bluetooth devices you choose from this Mac to your other Mac. That can be a mouse, a keyboard, headphones, a trackpad, or anything else already paired. Each device stays paired to both Macs and connected to one. You pick which ones move.

The two Macs already know each other. They use the same Apple Account, they are on the same network, and the devices are already paired in Bluetooth on both. Hop does not pair them, and it does not introduce the Macs.

Hop runs in the menu bar. On a Mac the window stays closed until you choose **Devices…**. The status item shows how many devices are chosen. While the other Mac is not running Hop, it reads `no Mac`.

## How a move works

A Bluetooth device such as a Magic Mouse or a pair of headphones connects to one Mac at a time, and it can remember more than one. Pair each device in System Settings → Bluetooth on both Macs once. After that, Hop does not ask you to pair again.

**Send to the other Mac**, in the menu bar or the window, checks that the other Mac can take the chosen devices. If it cannot, Hop leaves them connected here. `IOBluetoothDevice` `closeConnection` belongs only on the path where that check passes, and `openConnection` belongs on the other Mac.

The other Mac has to be awake and running Hop. If it is asleep, the devices have nowhere to land. This build has not discovered the other Mac yet, so Send does not disconnect anything.

## What this build does

The window lists every device paired in Bluetooth. Tap one to include it. The choice is saved as JSON:

- macOS: `~/Library/Application Support/Hop/choice.json`
- `HOP_CONFIG_DIR` overrides that directory

Hop writes a temporary file and renames it into place. A file that does not parse is an error, and Hop starts with nothing chosen. If `choice.json` is missing and an older `chosen.txt` is there, Hop reads that list.

The menu is **Devices…**, **Send to the other Mac**, and **Quit Hop**. Closing the window leaves Hop running.

macOS asks for Bluetooth access so Hop can read the paired devices. Local Network access comes when the two Macs link.

## Build and run on a Mac

Hop tracks the same GPUI revision as Stand. That currently means Rust 1.98.1 (see `rust-toolchain.toml`). Install [Xcode](https://developer.apple.com/xcode/) and the command line tools, then:

```sh
xcode-select --install
cargo run --release
```

The first build compiles GPUI from the Zed repository. This repo does not fork Zed.
