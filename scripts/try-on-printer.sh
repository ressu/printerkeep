#!/usr/bin/env bash
# Build printerkeep for a printer, copy it there, and run it on a COPY of the config folder.
#
# Usage:
#   scripts/try-on-printer.sh [--reset] [--copy] <ssh host> [printerkeep command and arguments...]
#   scripts/try-on-printer.sh --disconnect <ssh host>
#
# Options:
#   --reset   remove the test history, the log and the test copy (also its printerkeep.conf) first.
#             The next copy is made from the real config folder.
#   --copy    copy the real config folder again. The printerkeep.conf of the test copy is kept.
#             Without --copy, the test copy is used as it is (with your edits), and a copy is made
#             only if there is no test copy.
#   --disconnect  close the SSH connection. Without it, the connection closes 10 minutes after the last run.
#
# The command after the host is given to printerkeep as one command. The default is "backup".
# The binary is built and copied only if it changed.
#
# Examples:
#   scripts/try-on-printer.sh pi@printer-1                    # runs "printerkeep backup"
#   scripts/try-on-printer.sh pi@printer-1 status
#   scripts/try-on-printer.sh --copy pi@printer-1             # take in new changes from the real config
#   scripts/try-on-printer.sh --reset pi@printer-1            # start again with an empty history
#   scripts/try-on-printer.sh pi@printer-1 connect https://github.com/me/printerkeep-test.git
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
disconnect=0
while [ $# -gt 0 ]; do
  case "$1" in
    --reset) reset=1 ;;
    --copy) copy=1 ;;
    --disconnect) disconnect=1 ;;
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
label=$(printf '%q' "==> printerkeep $*")

# The "~" is expanded by the shell on the printer, not here.
# shellcheck disable=SC2088
printer_data=${PRINTER_DATA:-'~/printer_data'}
# shellcheck disable=SC2088
test_dir='~/printerkeep-test'

# One SSH connection, kept open for 10 minutes after the last use, so the password is asked only once
# and later runs connect at once. Close it earlier with --disconnect.
# The socket is in /tmp because socket paths must be short; $TMPDIR can be long (for example in Nix shells).
# %C is a hash of the user, host and port.
control_dir="/tmp/printerkeep-ssh-$(id -u)"
mkdir -p "$control_dir"
chmod 700 "$control_dir"
ssh_options=(-o ControlMaster=auto -o ControlPath="$control_dir/%C" -o ControlPersist=10m)
ssh() { command ssh "${ssh_options[@]}" "$@"; }
scp() { command scp "${ssh_options[@]}" "$@"; }

if [ "$disconnect" = 1 ]; then
  ssh -O exit "$host" 2>/dev/null || true
  echo "Closed the SSH connection to $host."
  exit 0
fi

# One call gets the CPU and the checksum of the binary that is on the printer now.
# shellcheck disable=SC2029 # The variables are meant to expand here, on this computer.
info=$(ssh "$host" "mkdir -p $test_dir/bin && uname -m && { sha256sum $test_dir/bin/printerkeep 2>/dev/null || true; }")
arch=$(sed -n 1p <<<"$info")
remote_sum=$(sed -n '2s/ .*//p' <<<"$info")
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

# cargo builds again only if the source changed.
echo "==> Building for $arch ($target)"
cargo build --quiet --release --target "$target"
binary="target/$target/release/printerkeep"

if [ "$(sha256sum "$binary" | sed 's/ .*//')" = "$remote_sum" ]; then
  echo "==> printerkeep on $host is up to date"
else
  echo "==> Copying printerkeep to $host:$test_dir/bin/"
  scp -q "$binary" "$host:printerkeep-test/bin/printerkeep"
fi

# With a terminal, -t lets printerkeep hide the token when you type it.
terminal=()
if [ -t 0 ]; then
  terminal=(-t)
fi
# One call prepares the test copy of the config folder and runs printerkeep.
# shellcheck disable=SC2029 # The variables are meant to expand here, on this computer.
ssh "${terminal[@]}" "$host" "set -e
  test -d $printer_data/config || { echo 'No config folder: $printer_data/config' >&2; exit 1; }
  mkdir -p $test_dir/printer_data
  copy=$copy
  if [ $reset = 1 ]; then
    rm -rf $test_dir/printer_data/printerkeep $test_dir/printer_data/logs $test_dir/printer_data/config
    echo '==> Removed the test history, the log and the test copy'
  fi
  if [ ! -d $test_dir/printer_data/config ]; then
    copy=1
  fi
  if [ \$copy = 1 ]; then
    # Keep the test copy's printerkeep.conf (for example the repository from connect).
    keep=$test_dir/printer_data/printerkeep.conf.keep
    rm -f \$keep
    if [ -f $test_dir/printer_data/config/printerkeep.conf ]; then
      cp $test_dir/printer_data/config/printerkeep.conf \$keep
    fi
    rm -rf $test_dir/printer_data/config
    cp -a $printer_data/config $test_dir/printer_data/config
    echo \"==> Copied $printer_data/config to $test_dir/printer_data/config\"
    if [ -f \$keep ]; then
      mv \$keep $test_dir/printer_data/config/printerkeep.conf
      echo '    Kept the printerkeep.conf of the test copy.'
    fi
  fi
  echo $label
  exec $test_dir/bin/printerkeep --printer-data $test_dir/printer_data $command"
