#!/bin/sh
# Build Hop.app for Apple silicon. The icon and About panel come from this bundle.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
cargo build --release
dest="${1:-$root/dist/Hop.app}"
rm -rf "$dest"
mkdir -p "$dest/Contents/MacOS" "$dest/Contents/Resources"
cp "$root/target/release/hop" "$dest/Contents/MacOS/hop"
chmod 755 "$dest/Contents/MacOS/hop"
cp "$root/assets/macos/Info.plist" "$dest/Contents/Info.plist"
cp "$root/assets/AppIcon.icns" "$dest/Contents/Resources/AppIcon.icns"
identity="${APPLE_SIGNING_IDENTITY:--}"
if [ "$identity" = "-" ]; then
  codesign --force --sign - --identifier com.halilatilla.hop "$dest"
else
  codesign --force --sign "$identity" --options runtime --timestamp --identifier com.halilatilla.hop "$dest"
fi
echo "$dest"
