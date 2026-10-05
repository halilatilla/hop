#!/bin/sh
# Notarize dist/Hop.app when a Developer ID certificate is available.
# With no certificate in the environment, this leaves the ad-hoc signature in place.
# Do not commit the certificate or the app-specific password.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
app="${1:-$root/dist/Hop.app}"

cert="${APPLE_CERTIFICATE_BASE64:-}"
password="${APPLE_CERTIFICATE_PASSWORD:-}"
apple_id="${APPLE_ID:-}"
team="${APPLE_TEAM_ID:-}"
app_password="${APPLE_APP_PASSWORD:-}"

if [ -z "$cert$password$apple_id$team$app_password" ]; then
  echo "notarize: no Developer ID secrets set; leaving the current signature"
  exit 0
fi
if [ -z "$cert" ] || [ -z "$password" ] || [ -z "$apple_id" ] || [ -z "$team" ] || [ -z "$app_password" ]; then
  echo "notarize: set APPLE_CERTIFICATE_BASE64, APPLE_CERTIFICATE_PASSWORD, APPLE_ID, APPLE_TEAM_ID, and APPLE_APP_PASSWORD together" >&2
  exit 1
fi

keychain="$root/dist/hop-signing.keychain"
p12="$root/dist/hop-signing.p12"
trap 'security delete-keychain "$keychain" >/dev/null 2>&1 || true; rm -f "$p12"' EXIT

printf '%s' "$cert" | base64 --decode > "$p12"
security create-keychain -p hop "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p hop "$keychain"
security import "$p12" -k "$keychain" -P "$password" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k hop "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

identity=$(security find-identity -v -p codesigning "$keychain" | awk -F '"' '/Developer ID Application/ { print $2; exit }')
if [ -z "$identity" ]; then
  echo "notarize: the certificate has no Developer ID Application identity" >&2
  exit 1
fi
APPLE_SIGNING_IDENTITY="$identity" "$root/script/package-macos.sh" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

zip="$root/dist/Hop-notarize.zip"
ditto -c -k --keepParent "$app" "$zip"
xcrun notarytool submit "$zip" --apple-id "$apple_id" --team-id "$team" --password "$app_password" --wait
xcrun stapler staple "$app"
spctl --assess --type execute --verbose=2 "$app"
rm -f "$zip"
