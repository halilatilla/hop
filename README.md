<p align="center">
  <img src="assets/AppIcon.png" width="96" alt="Hop icon">
</p>

<h1 align="center">Hop</h1>

<p align="center">
  Move Bluetooth devices between two Macs from the menu bar.<br>
  Stop disconnecting and reconnecting your keyboard, mouse, and AirPods manually.
</p>

Hop sits in the menu bar. Your devices stay paired to both Macs. Hop only moves which Mac they are connected to.

Open Hop on both Macs. They need the same local network, and each device paired in Bluetooth on both. macOS asks for Bluetooth and local network access.

A current menu capture is not in the repo yet. It should show **Connected here** and **Move here**.

```text
Mac A                         Mac B
  Hop ←—— local network ——→ Hop
   │                         │
Bluetooth                 Bluetooth
```

A move asks the Mac that has the device before anything disconnects. If the other Mac does not take it, Hop puts the device back on the Mac that had it.

## Install

Apple silicon, macOS 11 or later. Use the same version on both Macs.

1. Download `Hop-<version>-macos-arm64.zip` from [Releases](https://github.com/halilatilla/hop/releases).
2. Unzip it and move Hop into Applications.
3. Open Hop. The current builds are ad-hoc signed, not notarized. macOS says it could not verify the app. Go to **System Settings → Privacy & Security** and click **Open Anyway**.

If that button is not there, run this in Terminal, then open Hop:

```sh
xattr -dr com.apple.quarantine /Applications/Hop.app
```

Quit the old Hop from its menu before you open the new one.

## First time

Open Hop on both Macs. Each menu shows the same short code. Choose **Codes match** when the codes are the same. Hop remembers that Mac after that.

Pair each device once in **System Settings → Bluetooth**, on both Macs. Hop does not ask you to pair again.

macOS will ask for Bluetooth access, and for the local network when the two Macs find each other.

## Move a device

A checkmark means **Connected here**.

On that Mac, choose **Let** the other Mac **move it**. On the other Mac, the row says **Connected to** the Mac that has it. Choose **Move here**.

The row says **Moving…** until that Mac has the device. The menu bar says **Moving**.

If the other Mac does not take it, the device stays where it was. If it already left, Hop puts it back.

**Keep on this Mac**, inside the checkmark’s row, leaves the device connected here.

**Other Mac** is the other computer’s name. When Hop is not open there, the line says **Hop isn't running**. A device that Mac was using says **Unavailable**.

The menu bar icon stays quiet. It reads **Allow** while a new Mac is waiting.

## If a move does not finish

Hop asks the other Mac before anything disconnects. If that Mac does not answer, the device stays where it is. If it does leave and the other Mac does not take it, Hop puts it back.

The other Mac needs to be awake, on the same network, and running Hop.

## Build

[Xcode](https://developer.apple.com/xcode/) and the command line tools. Rust 1.98.1, from `rust-toolchain.toml`.

```sh
xcode-select --install
cargo fmt --check
cargo check
cargo test
cargo run --release
```

The first build compiles GPUI from the Zed repository. This repo does not fork Zed.

Hop keeps the shared list in `~/Library/Application Support/Hop`. Set `HOP_CONFIG_DIR` to use another folder.

## Privacy

Hop has no account and no cloud service. The two Macs talk on the local network. Hop does not collect telemetry.

## License

[Apache-2.0](LICENSE). To build or report a problem, see [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
