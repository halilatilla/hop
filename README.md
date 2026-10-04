# Hop

Hop shares the Bluetooth devices you check, then either Mac can take them. That can be a mouse, a keyboard, headphones, a trackpad, or anything else already paired. Each device stays paired to both Macs and connected to one.

The two Macs already know each other. They use the same Apple Account, they are on the same network, and the devices are already paired in Bluetooth on both. Hop does not introduce the Macs.

Hop runs in the menu bar. Sharing, connecting, and removing a device all happen in that menu. The status item is the icon. It reads **Allow** while a new Mac is waiting, and **…** while a device is connecting.

## How a move works

A Bluetooth device such as a Magic Mouse or a pair of headphones connects to one Mac at a time, and it can remember more than one. Pair each device in System Settings → Bluetooth on both Macs once. After that, Hop does not ask you to pair again.

Both Macs run Hop. Each one announces itself on the local network. The first time the other Mac appears, the menu bar reads **Allow** and the menu shows the code with **Codes match**. Choose **Codes match** only when that code is the same on the other Mac. After that, Hop remembers the other Mac.

On the Mac that has a device, choose **Share** in the menu. Both Macs then show it under **Shared**. A checkmark means this Mac is using it. **Remove**, under that name, takes it off the shared list and leaves it connected here. **Connect** means the other Mac is using it, and choosing it connects the device here. The other Mac’s name sits at the top of the menu when it is on the network.

That click asks before anything disconnects. The Mac that has the device drops it from Bluetooth, then this Mac connects it. If this Mac does not have it paired yet, it pairs it. If the other Mac does not answer or has not allowed this Mac, nothing is dropped. If the device does leave and this Mac does not connect it, Hop connects it again on the Mac that had it. That failure is one notification, replaced if you try again.

The other Mac has to be awake, on the same network, and running Hop. A Mac that has not been allowed cannot make Hop disconnect anything.

## What this build does

The shared list is saved as JSON:

- macOS: `~/Library/Application Support/Hop/choice.json`
- `HOP_CONFIG_DIR` overrides that directory

Hop writes a temporary file and renames it into place. A file that does not parse is an error, and Hop starts with nothing chosen. If `choice.json` is missing and an older `chosen.txt` is there, Hop reads that list.

The menu is **About Hop**, the other Mac’s name when it is ready, **Other Mac is not running Hop** when it is not, the code when that Mac is new, **On this Mac** for a device you can share, **Shared**, and **Quit Hop**. A checkmark is a device on this Mac. Choosing **Connect** connects that device here.

macOS asks for Bluetooth access so Hop can read the paired devices. Local Network access comes when the two Macs link.

## Build and run on a Mac

Hop tracks the same GPUI revision as Stand. That currently means Rust 1.98.1 (see `rust-toolchain.toml`). Install [Xcode](https://developer.apple.com/xcode/) and the command line tools, then:

```sh
xcode-select --install
cargo run --release
```

The first build compiles GPUI from the Zed repository. This repo does not fork Zed.
