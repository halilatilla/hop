<p align="center">
  <img src="assets/AppIcon.png" width="96" alt="Hop icon">
</p>

<h1 align="center">Hop</h1>

<p align="center">
  Move Bluetooth devices between two Macs without pairing them again.
</p>

Hop lives in the menu bar. A keyboard, mouse, or pair of AirPods stays paired in Bluetooth on both Macs. Hop only changes which Mac the device is connected to.

There is no Apple Account, iCloud, or other account. Trust is local: you compare a short code on the two Macs and choose **Codes match**.

## Does this Mac work?

Apple silicon (M1 or later) and macOS 11 or later. Use the same Hop version on both Macs. Both Macs need to be awake, on the same local network, with Hop running.

Pair each device once in **System Settings → Bluetooth**, on both Macs. Hop does not pair devices for you. macOS asks for Bluetooth access, and for the local network when the Macs find each other.

## Install

1. Download `Hop-<version>-macos-arm64.zip` from [Releases](https://github.com/halilatilla/hop/releases). The current file is [Hop-0.2.2-macos-arm64.zip](https://github.com/halilatilla/hop/releases/download/v0.2.2/Hop-0.2.2-macos-arm64.zip).
2. Unzip it and move Hop into Applications. Replace the copy that is already there.
3. Open Hop. This build is ad-hoc signed, not notarized, so macOS says it could not verify the app. Click Done, then open **System Settings → Privacy & Security** and click **Open Anyway**.

If **Open Anyway** is not there, run this in Terminal, then open Hop:

```sh
xattr -dr com.apple.quarantine /Applications/Hop.app
```

Quit the old Hop from its menu bar before opening the new one. There is no automatic update and no Homebrew install. Install the same zip on both Macs.

## Trust the other Mac

1. Install and open Hop on both Macs.
2. Leave both Macs awake, on the same local network.
3. Each menu shows a short code next to the other Mac.
4. Compare the codes. The same two Macs show the same code.
5. Choose **Codes match** only when the codes are the same.
6. Hop remembers that Mac. The two Macs can then move devices.

The menu bar reads **Allow** until this is done. Choosing **Codes match** only trusts that Mac on this one. Do it on both Macs.

**Forget this Mac**, under that Mac’s name, removes the trust on this computer. Compare the codes again before the next move.

## Move a device

A checkmark means the device is **Connected here**.

On that Mac, choose **Let** the other Mac **move it**. That allows the other Mac to take the device. It does not move the device by itself.

On the other Mac, the row says **Connected to** the Mac that has the device. Choose **Move here**. That Mac asks for the device.

The row says **Moving…** while the handoff is in progress. The menu bar says **Moving**. Hop does not say the device has moved until that Mac has it.

**Connected here** means this Mac has the device. **Connected to** a Mac means the other one has it.

If the move fails, Hop keeps the device connected to the Mac that had it.

**Keep on this Mac**, inside the checkmark’s row, leaves the device connected here and takes it off the list the other Mac can move.

**Other Mac** is the other computer’s name. When Hop cannot see that Mac, the line says **Hop isn't running**. A device that Mac was using then says **Unavailable**.

## If something goes wrong

Both Macs should be awake, on the same local network, with Hop running and Bluetooth available. Compare the codes before you choose **Codes match**.

**Hop isn't running.** Hop cannot see the other Mac. The devices stayed connected here.

**Couldn't reach the other Mac.** The connection could not be opened, or it dropped. Check that both Macs are awake, on the same local network, and running Hop. The devices stayed connected here.

**The other Mac did not answer.** The other Mac did not respond in time. The devices stayed connected here.

**The other Mac's reply could not be used.** Hop got a response it could not safely use. The devices stayed connected here.

**Stopped waiting for the other Mac.** This Mac stopped waiting before the move finished. The devices stayed connected here.

**Hop could not read Bluetooth.** macOS did not let Hop read the device list, so the menu shows this instead of devices. That is different from having no paired devices. When Bluetooth can be read again, the list comes back on its own.

For a bug, open a [GitHub issue](https://github.com/halilatilla/hop/issues). Include the macOS version and the sentence Hop showed. Do not paste Bluetooth addresses, pairing keys, or anything from `~/Library/Application Support/Hop`. For a security problem, use a private advisory as described in [SECURITY.md](SECURITY.md).

## Privacy

The two Macs talk on the local network. You trust a Mac by matching the codes. Messages between the Macs are signed. Hop has no account, no cloud service, and no telemetry.

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

## License

[Apache-2.0](LICENSE). To build, see [CONTRIBUTING.md](CONTRIBUTING.md).
