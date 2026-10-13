#!/bin/sh
# Download printerkeep for this printer, check it, and run "printerkeep install".
# Run it as the printer's user. It does not use sudo.
#
# Variables:
#   VERSION        the release to install, for example v0.1.0 (default: the latest release)
#   PRERELEASE=1   install the latest pre-release
#   BINARY_SHA256  the SHA-256 checksum of the program, from the release page on GitHub. The installer then
#                  does not download the checksum file. "sha256:<hex>" and "<hex>" are both accepted.
#   PRINTER_DATA   the printer_data folder (default: ~/printer_data)
#   PRINTER_NAME   the printer name, the name of its branch in the repository (default: main). Give each
#                  printer its own name when more printers use the same repository.
#
# "printerkeep install" does the rest, in the home folder of the user. See README.md.
set -eu

REPO="ressu/printerkeep"
VERSION="${VERSION:-}"
PRERELEASE="${PRERELEASE:-0}"
BINARY_SHA256="${BINARY_SHA256:-}"
PRINTER_DATA="${PRINTER_DATA:-}"

fail() {
  printf 'printerkeep installer: %s\n' "$*" >&2
  exit 1
}

[ "$(id -u)" != 0 ] || fail "run the installer as the printer's user, not as root. It needs no root access."
command -v curl >/dev/null 2>&1 || fail "the installer needs curl."
command -v sha256sum >/dev/null 2>&1 || fail "the installer needs sha256sum."

case "$(uname -m)" in
  aarch64 | arm64) ASSET=printerkeep-linux-aarch64 ;;
  armv7* | armv8l) ASSET=printerkeep-linux-armv7 ;;
  armv6*) ASSET=printerkeep-linux-armv6 ;;
  x86_64 | amd64) ASSET=printerkeep-linux-x86_64 ;;
  *) fail "this CPU is not supported: $(uname -m)" ;;
esac

if [ -z "$VERSION" ]; then
  if [ "$PRERELEASE" = 1 ]; then
    api="https://api.github.com/repos/$REPO/releases?per_page=1"
  else
    api="https://api.github.com/repos/$REPO/releases/latest"
  fi
  VERSION=$(curl -fsSL "$api" | grep '"tag_name"' | head -n 1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  [ -n "$VERSION" ] || fail "cannot find the latest release. Set VERSION, for example VERSION=v0.1.0."
fi

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
BASE_URL="https://github.com/$REPO/releases/download/$VERSION"

echo "Downloading printerkeep $VERSION ($ASSET)"
curl -fsSL "$BASE_URL/$ASSET" -o "$TMP_DIR/$ASSET" || fail "cannot download $BASE_URL/$ASSET"

cd "$TMP_DIR"
if [ -n "$BINARY_SHA256" ]; then
  expected="${BINARY_SHA256#sha256:}"
  computed=$(sha256sum "$ASSET" | cut -d ' ' -f 1)
  [ "$computed" = "$expected" ] ||
    fail "the checksum of $ASSET is not correct (expected $expected, computed $computed). Nothing was installed."
else
  curl -fsSL "$BASE_URL/printerkeep-checksums.txt" -o checksums.txt || fail "cannot download the checksum file"
  grep " $ASSET\$" checksums.txt | sha256sum -c - >/dev/null ||
    fail "the checksum of $ASSET is not correct. Nothing was installed."
fi
chmod 755 "$ASSET"

set -- install
if [ -n "${PRINTER_NAME:-}" ]; then
  set -- install --name "$PRINTER_NAME"
fi
if [ -n "$PRINTER_DATA" ]; then
  "./$ASSET" --printer-data "$PRINTER_DATA" "$@"
else
  "./$ASSET" "$@"
fi
