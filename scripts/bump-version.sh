#!/usr/bin/env bash
# Bump the synchronized version of all four published RustViz crates.
#
# Per the project's release scheme, the four published crates share a
# `v2.Y.0` version (Z always 0). One tag publishes them all from
# `.github/workflows/publish.yml`, so the four Cargo.toml manifests
# must be in lockstep before tagging — the publish workflow rejects
# the tag otherwise.
#
# Usage:
#   scripts/bump-version.sh           # auto-increment Y: 2.3.0 -> 2.4.0
#   scripts/bump-version.sh 5         # set explicit Y: -> 2.5.0
#
# What it edits:
#   - rustviz-plugin/Cargo.toml             (package version)
#   - rustviz-lib/Cargo.toml                (package version)
#   - rustviz-cli/Cargo.toml                (package version + rustviz-lib path-dep pin)
#   - mdbook-rustviz/Cargo.toml             (package version + rustviz-lib path-dep pin)
#   - Cargo.lock                            (refreshed via `cargo update -w`)
#
# After this runs, review the diff, commit, push to main, then once
# merged tag `v2.Y.0` and push the tag.

set -euo pipefail
cd "$(dirname "$0")/.."

CURRENT=$(grep -m1 '^version = ' rustviz-lib/Cargo.toml | sed -E 's/^version = "(.+)"/\1/')
CURRENT_Y=$(echo "$CURRENT" | awk -F. '{print $2}')

if [ $# -ge 1 ]; then
    NEW_Y="$1"
else
    NEW_Y=$((CURRENT_Y + 1))
fi

# Guard against typos like `bump-version.sh v5` or non-integer args.
if ! [[ "$NEW_Y" =~ ^[0-9]+$ ]]; then
    echo "error: minor version must be a non-negative integer (got: $NEW_Y)" >&2
    exit 2
fi

NEW="2.${NEW_Y}.0"

if [ "$NEW" = "$CURRENT" ]; then
    echo "current version is already ${NEW}; nothing to do" >&2
    exit 0
fi

echo "Bumping ${CURRENT} -> ${NEW}"
echo

python3 - "$NEW" <<'PY'
import re
import sys
import pathlib

new = sys.argv[1]
crates = ['rustviz-plugin', 'rustviz-lib', 'rustviz-cli', 'mdbook-rustviz']

# 1. Bump [package] version in each manifest. The first `^version = "X"`
#    line in each file is the package version (others, like
#    `[dependencies.clap]\nversion = "..."`, are dep specs and we
#    don't touch them; the path-dep on rustviz-lib gets handled
#    separately below).
for c in crates:
    p = pathlib.Path(c) / 'Cargo.toml'
    text = p.read_text()
    new_text, n = re.subn(
        r'(?m)^(version = ")[^"]+(")',
        lambda m: m.group(1) + new + m.group(2),
        text,
        count=1,
    )
    if n != 1:
        print(f"error: didn't find ^version line in {p}", file=sys.stderr)
        sys.exit(1)
    p.write_text(new_text)
    print(f"  {c}: package version -> {new}")

# 2. Crates that depend on rustviz-lib via `path = "..", version = ".."`
#    pin the version so cargo publish accepts the manifest. Bump that
#    pin to the new release version.
for c in ['rustviz-cli', 'mdbook-rustviz']:
    p = pathlib.Path(c) / 'Cargo.toml'
    text = p.read_text()
    new_text, n = re.subn(
        r'(rustviz-lib\s*=\s*\{[^}]*version\s*=\s*")[^"]+(")',
        lambda m: m.group(1) + new + m.group(2),
        text,
    )
    if n != 1:
        print(f"error: didn't find rustviz-lib version pin in {p}", file=sys.stderr)
        sys.exit(1)
    p.write_text(new_text)
    print(f"  {c}: rustviz-lib path-dep pin -> {new}")
PY

echo
echo "Refreshing Cargo.lock..."
cargo update -w >/dev/null

echo
echo "Done. Review the diff, then:"
echo "  git add -A && git commit -m 'Bump RustViz to v${NEW}'"
echo "  # push the commit, get it merged to main, then:"
echo "  git tag v${NEW} && git push origin v${NEW}"
echo "  # the v${NEW} tag triggers .github/workflows/publish.yml,"
echo "  # which publishes all four crates to crates.io in dep order."
