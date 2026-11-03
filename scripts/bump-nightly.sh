#!/usr/bin/env bash
# Move RustViz to a new pinned nightly + matching rustc_plugin/rustc_utils.
#
# rustc_plugin and rustc_utils publish one release per supported nightly,
# with the nightly baked into the version string (e.g.
# `0.15.2-nightly-YYYY-MM-DD`). Pass that version; the nightly is derived
# from it so the two can't disagree.
#
# Usage:
#   scripts/bump-nightly.sh X.Y.Z-nightly-YYYY-MM-DD
#   scripts/bump-nightly.sh --no-lock X.Y.Z-nightly-YYYY-MM-DD   # skip Cargo.lock
#
# What it edits: every git-tracked file that mentions the current nightly
# (read from rust-toolchain.toml), i.e. the three rust-toolchain.toml
# copies, both playground Dockerfiles, playground/runner/rv-runner.sh, the
# READMEs and doc comments; plus the rustc_plugin/rustc_utils versions in
# rustviz-plugin/Cargo.toml, then refreshes Cargo.lock with
# `cargo update -p rustc_plugin -p rustc_utils` (needs network; rustup
# installs the new nightly on first use).
#
# This only rewrites pins. The compiler-API port and the verification
# recipe are in CLAUDE.md, "Bumping the toolchain".

set -euo pipefail
cd "$(dirname "$0")/.."

update_lock=1
if [ "${1:-}" = "--no-lock" ]; then
    update_lock=0
    shift
fi

if [ $# -ne 1 ]; then
    echo "usage: $0 [--no-lock] RUSTC_PLUGIN_VERSION   (e.g. 0.15.2-nightly-YYYY-MM-DD)" >&2
    exit 2
fi

NEW_VERSION="$1"
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+-(nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})$ ]]; then
    echo "error: expected a rustc_plugin version like 0.15.2-nightly-YYYY-MM-DD (got: $NEW_VERSION)" >&2
    exit 2
fi
NEW_NIGHTLY="${BASH_REMATCH[1]}"

OLD_NIGHTLY=$(sed -nE 's/^channel = "(nightly-[0-9-]+)"$/\1/p' rust-toolchain.toml)
if [ -z "$OLD_NIGHTLY" ]; then
    echo "error: couldn't read the current nightly from rust-toolchain.toml" >&2
    exit 1
fi
OLD_VERSION=$(sed -nE 's/^rustc_plugin = \{ *version = "([^"]+)" *\}$/\1/p' rustviz-plugin/Cargo.toml)
if [ -z "$OLD_VERSION" ]; then
    echo "error: couldn't read the rustc_plugin version from rustviz-plugin/Cargo.toml" >&2
    exit 1
fi

echo "nightly:      $OLD_NIGHTLY -> $NEW_NIGHTLY"
echo "rustc_plugin: $OLD_VERSION -> $NEW_VERSION"
echo

# 1. rustc_plugin / rustc_utils dependency versions (before the generic
#    nightly rewrite, which would otherwise half-update them).
python3 - "$OLD_VERSION" "$NEW_VERSION" <<'PY'
import pathlib, re, sys
old, new = sys.argv[1], sys.argv[2]
p = pathlib.Path("rustviz-plugin/Cargo.toml")
text = p.read_text()
for dep in ("rustc_utils", "rustc_plugin"):
    text, n = re.subn(
        r'(?m)^(%s = \{ *version = ")%s(" *\})' % (dep, re.escape(old)),
        lambda m: m.group(1) + new + m.group(2),
        text,
    )
    if n != 1:
        sys.exit(f"error: didn't find `{dep} = {{version = \"{old}\"}}` in {p}")
p.write_text(text)
print(f"  {p}: rustc_utils, rustc_plugin -> {new}")
PY

# 2. Every other mention of the old nightly in tracked files. Cargo.lock
#    is left to cargo. Mentions of *older* nightlies (history in comments)
#    don't match and are left alone.
if [ "$OLD_NIGHTLY" != "$NEW_NIGHTLY" ]; then
    git grep -lF "$OLD_NIGHTLY" -- ':!Cargo.lock' | while read -r f; do
        python3 - "$f" "$OLD_NIGHTLY" "$NEW_NIGHTLY" <<'PY'
import pathlib, sys
f, old, new = sys.argv[1:]
p = pathlib.Path(f)
text = p.read_text()
p.write_text(text.replace(old, new))
print(f"  {f}: {text.count(old)} x {old} -> {new}")
PY
    done
fi

if [ "$update_lock" = 1 ]; then
    echo
    echo "Refreshing Cargo.lock (rustc_plugin, rustc_utils)..."
    cargo update -p rustc_plugin -p rustc_utils
fi

echo
echo "Left over (should be empty, or only historical comments):"
git grep -nF "$OLD_NIGHTLY" -- ':!Cargo.lock' || echo "  (none)"
echo
echo "Next: port the compiler-API breakage and verify; see CLAUDE.md,"
echo "\"Bumping the toolchain\"."
