#!/usr/bin/env bash
# Zero-output-change check for refactors: run two plugin builds over every
# fixture and Book snippet and list the snippets whose output differs.
#
# Usage:
#   scripts/probe-diff.sh <old-plugin-root> <new-plugin-root>
#
# A plugin root is the `--root` of `cargo install --locked --path
# rustviz-plugin --root <dir>` (it holds bin/cargo-rv-plugin and
# bin/rv-plugin-driver). Inputs: rustviz-plugin/tests/*.rs,
# rustviz-plugin/tests/*/*.rs (edition2024/) and rustviz-plugin/tests/book/*.rs.
#
# For each snippet and each build it records, per snippet:
#   * the sorted `data-tooltip-text` attributes of the timeline SVG (order
#     ignored),
#   * a hash of the timeline SVG with those attributes removed (geometry,
#     colours, arrows) and a hash of the code-panel SVG,
#   * or, if the plugin fails, the exit code and the last stderr line, so a
#     new crash or a changed compile error also shows up.
# Prints the snippets that differ (with a tooltip diff when the tooltips
# changed) and exits 1; prints "identical" and exits 0 otherwise.
#
# Env: JOBS (parallel runs, default 4), KEEP=1 (keep the work dir and print
# its path; it holds the per-snippet records under old/ and new/).

set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <old-plugin-root> <new-plugin-root>" >&2
    exit 2
fi
repo="$(cd "$(dirname "$0")/.." && pwd)"
export RV_REPO="$repo"
export RV_OLD="$(cd "$1" && pwd)"
export RV_NEW="$(cd "$2" && pwd)"
for r in "$RV_OLD" "$RV_NEW"; do
    [ -x "$r/bin/cargo-rv-plugin" ] || { echo "$r/bin/cargo-rv-plugin not found" >&2; exit 2; }
done

work="$(mktemp -d "${TMPDIR:-/tmp}/probe-diff.XXXXXX")"
export RV_WORK="$work"
if [ -z "${KEEP:-}" ]; then trap 'rm -rf "$work"' EXIT; else echo "work dir: $work" >&2; fi

python3 -I - "${JOBS:-4}" <<'PY'
import difflib, glob, hashlib, json, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

repo, work = os.environ["RV_REPO"], os.environ["RV_WORK"]
builds = {"old": os.environ["RV_OLD"], "new": os.environ["RV_NEW"]}
tests = os.path.join(repo, "rustviz-plugin", "tests")
snippets = sorted(glob.glob(tests + "/*.rs") + glob.glob(tests + "/*/*.rs"))
toolchain = open(os.path.join(repo, "rust-toolchain.toml")).read()
TT = re.compile(r'data-tooltip-text="([^"]*)"')

# cargo looks for `cargo-rv-plugin` in $CARGO_HOME/bin before PATH, so give
# each build a private CARGO_HOME whose bin is the build under test.
homes = {}
for tag, root in builds.items():
    home = os.path.join(work, "home-" + tag)
    os.makedirs(home)
    os.symlink(os.path.join(root, "bin"), os.path.join(home, "bin"))
    homes[tag] = home

def h(s):
    return hashlib.sha256(s.encode()).hexdigest()[:16]

def run(tag, path):
    rel = os.path.relpath(path, tests)
    proj = os.path.join(work, "proj-" + tag, rel[:-3].replace("/", "__"))
    os.makedirs(os.path.join(proj, "src"))
    open(os.path.join(proj, "Cargo.toml"), "w").write(
        '[package]\nname = "user-code"\nversion = "0.1.0"\nedition = "2024"\n')
    open(os.path.join(proj, "rust-toolchain.toml"), "w").write(toolchain)
    open(os.path.join(proj, "src", "lib.rs"), "w").write(open(path).read())
    env = dict(os.environ, CARGO_HOME=homes[tag], CARGO_TERM_COLOR="never")
    p = subprocess.run(["cargo", "rv-plugin"], cwd=proj, env=env,
                       capture_output=True, text=True)
    if p.returncode != 0:
        last = (p.stderr.strip().splitlines() or [""])[-1]
        rec = {"fail": "exit %d: %s" % (p.returncode, last), "tooltips": []}
    else:
        try:
            out = json.loads(p.stdout)
            tl = out["timeline_panel"]
            rec = {"tooltips": sorted(TT.findall(tl)),
                   "timeline": h(TT.sub("", tl)), "code": h(out["code_panel"])}
        except Exception as e:
            rec = {"fail": "unparseable output: %s" % e, "tooltips": []}
    return rel, rec

jobs = int(sys.argv[1])
res = {}
with ThreadPoolExecutor(jobs) as ex:
    for tag in builds:
        for rel, rec in ex.map(lambda p: run(tag, p), snippets):
            res.setdefault(rel, {})[tag] = rec
for tag in builds:
    os.makedirs(os.path.join(work, tag), exist_ok=True)
for rel, r in res.items():
    for tag in builds:
        json.dump(r[tag], open(os.path.join(work, tag, rel.replace("/", "__") + ".json"), "w"), indent=1)

diffs = [(rel, r) for rel, r in sorted(res.items()) if r["old"] != r["new"]]
print("%d snippets, %s" % (len(res), "identical" if not diffs else "%d differ" % len(diffs)))
for rel, r in diffs:
    o, n = r["old"], r["new"]
    kinds = []
    if o.get("fail") != n.get("fail"):
        kinds.append("failure: %r -> %r" % (o.get("fail"), n.get("fail")))
    if o["tooltips"] != n["tooltips"]:
        kinds.append("tooltips")
    if o.get("timeline") != n.get("timeline") and o["tooltips"] == n["tooltips"]:
        kinds.append("timeline geometry")
    elif o.get("timeline") != n.get("timeline"):
        kinds.append("timeline geometry too")
    if o.get("code") != n.get("code"):
        kinds.append("code panel")
    print("DIFF %s: %s" % (rel, "; ".join(kinds)))
    for line in difflib.unified_diff(o["tooltips"], n["tooltips"], "old", "new", lineterm="", n=0):
        if not line.startswith(("---", "+++", "@@")):
            print("    " + line)
sys.exit(1 if diffs else 0)
PY
