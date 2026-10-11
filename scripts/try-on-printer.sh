#!/usr/bin/env bash
# Build printerkeep for a printer, copy it there, and run it on a COPY of the config folder.
#
# Usage:
#   scripts/try-on-printer.sh [--reset] [--copy] <ssh host> [printerkeep command and arguments...]
#
# Options:
#   --reset   remove the test history, the log and the test copy (also its printerkeep.conf) first.
#             The next copy is made from the real config folder.
#   --copy    copy the real config folder again. The printerkeep.conf of the test copy is kept.
#             Without --copy, the test copy is used as it is (with your edits), and a copy is made
#             only if there is no test copy.
#
# The command after the host is given to printerkeep as one command. The default is "backup".
#
# Examples:
#   scripts/try-on-printer.sh pi@printer-1                    # runs "printerkeep backup"
#   scripts/try-on-printer.sh pi@printer-1 status
#   scripts/try-on-printer.sh --copy pi@printer-1             # take in new changes from the real config
#   scripts/try-on-printer.sh --reset pi@printer-1            # start again with an empty history
#   scripts/try-on-printer.sh pi@printer-1 add-repository https://github.com/me/printerkeep-test.git
#
# Repository tests: use a NEW, EMPTY test repository. If the repository already has a branch with the
# printer's name (for example from klipper-backup), printerkeep continues that branch and adds commits to it.
#
# Safety:
#   - Everything goes into ~/printerkeep-test on the printer: the binary, a copy of the config folder,
#     and printerkeep's own data and log. printerkeep runs with --printer-data pointed at that copy.
#   - The real printer_data folder is only read (to make the copy). Nothing in it is changed.
#   - Remove all traces with: ssh <host> rm -rf ~/printerkeep-test
#
# Variables:
#   PRINTER_DATA  the real printer_data folder on the printer (default: ~/printer_data)

set -euo pipefail

usage() {
  sed -n '2,/^$/p' "$0" | sed -e '/^$/d' -e 's/^# \{0,1\}//'
  exit "$1"
}

reset=0
copy=0
while [ $# -gt 0 ]; do
  case "$1" in
    --reset) reset=1 ;;
    --copy) copy=1 ;;
    -h | --help) usage 0 ;;
    -*)
      echo "Unknown option: $1" >&2
      usage 1
      ;;
    *) break ;;
  esac
  shift
done
if [ $# -lt 1 ]; then
  usage 1
fi

host=$1
shift
if [ $# -eq 0 ]; then
  set -- backup
fi
# Quote each argument for the shell on the printer.
command=$(printf '%q ' "$@")

# The "~" is expanded by the shell on the printer, not here.
# shellcheck disable=SC2088
printer_data=${PRINTER_DATA:-'~/printer_data'}
# shellcheck disable=SC2088
test_dir='~/printerkeep-test'

# One SSH connection for the whole run, so the password is asked only once.
# The socket is in /tmp because socket paths must be short; $TMPDIR can be long (for example in Nix shells).
control_dir=$(mktemp -d /tmp/printerkeep-ssh.XXXXXX)
control="$control_dir/socket"
# shellcheck disable=SC2329 # Called by the trap below.
cleanup() {
  command ssh -o ControlPath="$control" -O exit "$host" 2>/dev/null || true
  rm -rf "$control_dir"
}
trap cleanup EXIT

echo "==> Connecting to $host"
command ssh -o ControlMaster=yes -o ControlPath="$control" -o ControlPersist=yes -f -N "$host"
ssh() { command ssh -o ControlPath="$control" "$@"; }
scp() { command scp -o ControlPath="$control" "$@"; }

arch=$(ssh "$host" uname -m)
case "$arch" in
  aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
  armv7*) target=armv7-unknown-linux-musleabihf ;;
  armv6*) target=arm-unknown-linux-musleabihf ;;
  x86_64) target=x86_64-unknown-linux-musl ;;
  *)
    echo "Unsupported CPU on $host: $arch" >&2
    exit 1
    ;;
esac

echo "==> Building for $arch ($target)"
cargo zigbuild --release --target "$target"
binary="target/$target/release/printerkeep"

echo "==> Copying printerkeep to $host:$test_dir/bin/"
# shellcheck disable=SC2029 # The variables are meant to expand here, on this computer.
ssh "$host" "mkdir -p $test_dir/bin"
scp -q "$binary" "$host:printerkeep-test/bin/printerkeep"

echo "==> Preparing the test copy of the config folder"
# shellcheck disable=SC2029 # The variables are meant to expand here, on this computer.
ssh "$host" "set -e
  test -d $printer_data/config || { echo 'No config folder: $printer_data/config' >&2; exit 1; }
  mkdir -p $test_dir/printer_data
  copy=$copy
  if [ $reset = 1 ]; then
    rm -rf $test_dir/printer_data/printerkeep $test_dir/printer_data/logs $test_dir/printer_data/config
    echo 'Removed the test history, the log and the test copy.'
  fi
  if [ ! -d $test_dir/printer_data/config ]; then
    copy=1
  fi
  if [ \$copy = 1 ]; then
    # Keep the test copy's printerkeep.conf (for example the repository from add-repository).
    keep=$test_dir/printer_data/printerkeep.conf.keep
    rm -f \$keep
    if [ -f $test_dir/printer_data/config/printerkeep.conf ]; then
      cp $test_dir/printer_data/config/printerkeep.conf \$keep
    fi
    rm -rf $test_dir/printer_data/config
    cp -a $printer_data/config $test_dir/printer_data/config
    echo \"Copied $printer_data/config to $test_dir/printer_data/config\"
    if [ -f \$keep ]; then
      mv \$keep $test_dir/printer_data/config/printerkeep.conf
      echo 'Kept the printerkeep.conf of the test copy.'
    fi
  else
    echo 'Using the test copy as it is. Use --copy to copy the real config folder again.'
  fi"

# With a terminal, -t lets printerkeep hide the token when you type it.
terminal=()
if [ -t 0 ]; then
  terminal=(-t)
fi
echo "==> printerkeep $*"
status=0
# shellcheck disable=SC2029 # The variables are meant to expand here, on this computer.
ssh "${terminal[@]}" "$host" "$test_dir/bin/printerkeep --printer-data $test_dir/printer_data $command" || status=$?

echo
echo "Test files are in $host:$test_dir. Remove them with: ssh $host rm -rf $test_dir"
exit "$status"
