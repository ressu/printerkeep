#!/bin/sh
# Prints the release notes for one version: its section in CHANGELOG.md, and a link to the whole changelog.
# The release workflow uses them as the text of the GitHub release, which Fluidd and Mainsail link to.
# Fails when the section is missing or empty, so that a release can't go out without notes.
#
# Usage: scripts/release-notes.sh <version> [changelog file]
# Example: scripts/release-notes.sh 0.7.0
set -eu

version=${1:?Usage: scripts/release-notes.sh <version> [changelog file]}
changelog=${2:-CHANGELOG.md}

# The section starts at "## <version>" (the date may follow) and ends at the next "## " heading.
notes=$(awk -v version="$version" '
  /^## / { in_section = ($2 == version); next }
  in_section { print }
' "$changelog")

# Blank lines alone are not notes.
if ! printf '%s\n' "$notes" | grep -q '[^[:space:]]'; then
  echo "$changelog has no notes for $version. Add a \"## $version\" section with the changes." >&2
  exit 1
fi

printf '%s\n' "$notes" | sed -e '/./,$!d'
printf '\n%s\n' "All changes: [CHANGELOG.md](https://github.com/ressu/printerkeep/blob/main/CHANGELOG.md)"
