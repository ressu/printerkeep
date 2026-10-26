#!/usr/bin/env bash
# Run printerkeep on this computer, on a snapshot of a printer's config folder. Nothing on the printer changes,
# and nothing goes to a real repository.
#
# Usage:
#   scripts/try-local.sh --from <ssh host>     take a new snapshot from the printer, and start again
#   scripts/try-local.sh --reset               start again from the snapshot
#   scripts/try-local.sh [printerkeep command and arguments...]
#
# The command is given to printerkeep as one command. The default is "sync". Edit the files in
# target/try-local/printer_data/config between runs, as Fluidd or SAVE_CONFIG would.
#
# What it uses, all in target/try-local:
#   snapshot/       the printer's config folder, as --from copied it. Never changed after that.
#   printer_data/   a copy of the snapshot, and printerkeep's history and log for it.
#   repository.git  the test repository: an empty git repository at --from and --reset.
#   home/, run/     HOME and XDG_RUNTIME_DIR for printerkeep, so nothing goes into your own.
#
# Safety:
#   - --from only reads the printer (tar over SSH). It skips *.secrets files: printerkeep never backs them up.
#   - The repository: line of printerkeep.conf points to repository.git. git may use only local
#     repositories (GIT_ALLOW_PROTOCOL=file), so a URL to a real repository fails instead of sending.

set -euo pipefail

usage() {
  sed -n '2,/^$/p' "$0" | sed -e '/^$/d' -e 's/^# \{0,1\}//'
  exit "$1"
}

from=
reset=0
while [ $# -gt 0 ]; do
  case "$1" in
    --from)
      [ $# -ge 2 ] || usage 1
      from=$2
      shift
      ;;
    --reset) reset=1 ;;
    -h | --help) usage 0 ;;
    -*)
      echo "Unknown option: $1" >&2
      usage 1
      ;;
    *) break ;;
  esac
  shift
done
if [ $# -eq 0 ] && [ -z "$from" ] && [ "$reset" = 0 ]; then
  set -- sync
fi

root=$(cd "$(dirname "$0")/.." && pwd)
work=$root/target/try-local
snapshot=$work/snapshot
printer_data=$work/printer_data
repository=$work/repository.git

if [ -n "$from" ]; then
  echo "==> Copying the config folder of $from (read only)"
  rm -rf "$snapshot.new"
  mkdir -p "$snapshot.new"
  # The "~" is expanded by the shell on the printer.
  # shellcheck disable=SC2029
  ssh "$from" "tar -C ${PRINTER_DATA:-~/printer_data} --exclude='*.secrets' -cf - config" | tar -C "$snapshot.new" -xf -
  rm -rf "$snapshot"
  mv "$snapshot.new/config" "$snapshot"
  rmdir "$snapshot.new"
  reset=1
fi

if [ ! -d "$snapshot" ]; then
  echo "There's no snapshot yet. To take one from a printer, run:" >&2
  echo "  scripts/try-local.sh --from <ssh host>" >&2
  exit 1
fi

if [ "$reset" = 1 ] || [ ! -d "$printer_data/config" ]; then
  rm -rf "$printer_data" "$repository" "${work:?}/home" "${work:?}/run"
  mkdir -p "$printer_data"
  cp -a "$snapshot" "$printer_data/config"
  git init --quiet --bare "$repository"
  echo "==> Started again from the snapshot, with an empty test repository"
fi

# Point printerkeep.conf to the test repository, also after an edit or a connect.
settings=$printer_data/config/printerkeep.conf
if [ -f "$settings" ] && grep -Eq '^repository[[:space:]]*[:=]' "$settings"; then
  sed -E -i "s|^repository[[:space:]]*[:=].*|repository: $repository|" "$settings"
fi

if [ $# -eq 0 ]; then
  exit 0
fi

echo "==> Building"
cargo build --quiet --manifest-path "$root/Cargo.toml"

mkdir -p "$work/home" "$work/run"
chmod 700 "$work/run"
echo "==> printerkeep $*"
status=0
env HOME="$work/home" XDG_RUNTIME_DIR="$work/run" XDG_CONFIG_HOME="$work/home/.config" \
  GIT_ALLOW_PROTOCOL=file \
  "$root/target/debug/printerkeep" --printer-data "$printer_data" "$@" || status=$?

echo "==> Test repository ($repository)"
branches=$(git --git-dir="$repository" for-each-ref --format='  %(refname:short): %(subject)' refs/heads)
echo "${branches:-  empty}"
exit "$status"
