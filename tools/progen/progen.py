#!/usr/bin/env python3
"""Generate feature-combination programs, keep the ones rustc accepts, run
RustViz's corpus checks on them, shrink each failure and bucket the
failures by (check, signature). See README.md.

    progen.py run --count N --seed S --out DIR [--time-limit SEC]
    progen.py gen --count N --seed S --out DIR     (just write programs)
    progen.py check DIR                             (checks on a dir of .rs)

The checks are `corpus_invariants_hold` in rustviz-lib/tests/corpus.rs
(ownership invariants + geometry), run on our files through
RV_INVARIANTS_EXTRA / RV_INVARIANTS_ONLY; plugin panics and other plugin
errors come back as that test's "Snippets the plugin failed on".
`--oracle` (on `run` and `check`) adds `corpus_oracle_holds`: rustc's
borrow checker as ground truth for each variable's drawn state
(rustviz-lib/tests/common/oracle.rs).
"""
import argparse
import concurrent.futures as cf
import copy
import functools
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gen  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
JOBS = max(2, (os.cpu_count() or 4))
# `--oracle`: also run the rustc oracle (`corpus_oracle_holds`).
ORACLE = False


@functools.lru_cache(None)
def toolchain():
    with open(os.path.join(REPO, "rust-toolchain.toml")) as f:
        t = f.read()
    return re.search(r'channel\s*=\s*"([^"]+)"', t).group(1)


# ─── rustc filter ─────────────────────────────────────────────────────

def compiles(src, workdir):
    """(ok, first error code or message) for `src` as an edition-2024 lib."""
    fd, path = tempfile.mkstemp(suffix=".rs", dir=workdir)
    with os.fdopen(fd, "w") as f:
        f.write(src)
    try:
        p = subprocess.run(["rustc", "--edition", "2024", "--crate-type", "lib", "--emit=metadata",
                            "--out-dir", workdir, "-A", "warnings", "--crate-name", "progen_" + os.path.basename(path)[:-3].replace("-", "_"), path],
                           env=dict(os.environ, RUSTUP_TOOLCHAIN=toolchain()), capture_output=True, text=True)
    finally:
        os.unlink(path)
        for ext in (".rmeta",):
            m = os.path.join(workdir, "libprogen_" + os.path.basename(path)[:-3].replace("-", "_") + ext)
            if os.path.exists(m):
                os.unlink(m)
    if p.returncode == 0:
        return True, ""
    m = re.search(r"error\[(E\d+)\]", p.stderr)
    return False, m.group(1) if m else (p.stderr.strip().splitlines() or ["?"])[0][:80]


# ─── the checks ───────────────────────────────────────────────────────

def test_binary():
    """Build rustviz-lib's corpus test binary once; its path."""
    exe = os.environ.get("PROGEN_TEST_BIN")
    if exe:
        return exe
    p = subprocess.run(["cargo", "test", "-p", "rustviz-lib", "--test", "corpus", "--locked", "--no-run",
                        "--message-format=json"], cwd=REPO, capture_output=True, text=True)
    exe = None
    for line in p.stdout.splitlines():
        try:
            j = json.loads(line)
        except ValueError:
            continue
        if j.get("executable") and "corpus" in os.path.basename(j["executable"]):
            exe = j["executable"]
    if not exe:
        sys.exit("couldn't build the corpus test binary:\n" + p.stderr[-3000:])
    return exe


ID = re.compile(r"(?P<var>[^\s@:]+)@L\d+(?:-\d+)?(?::(?P<kind>[a-z_]+)(?::(?P<detail>[A-Za-z_+]+))?)?(?:#\d+)?(?:/\w+)?")


def norm_var(name, types):
    out = []
    for part in name.split(">"):
        base, dot, field = part.partition(".")
        base = base.rstrip("₀₁₂₃₄₅₆₇₈₉")  # a shadowed name (`s₂`)
        star = ""
        while base.startswith("*"):
            star += "*"
            base = base[1:]
        t = types.get(base)
        out.append(star + (t if t else base) + (("." + field) if dot else ""))
    return ">".join(out)


GEOMETRIC = {"line-starts-nowhere", "line-ends-nowhere", "line-through-fork", "arrows-overlap",
             "marks-on-one-spot", "arrow-misses-target", "outside-figure", "fork-without-dot",
             "heads-or-logos-overlap", "shaft-through-mark", "arrows-end-to-end"}


def signature(inv, what, types):
    """A failure's bucket key within its check. Ownership checks: the
    elements named, with variable names replaced by their types and line
    numbers dropped (`RS:dot:static_die`). Geometric checks: just the
    kinds of elements (`branch_line / dot`), since one layout bug shows up
    on whichever columns happen to be there. Crashes: file:line."""
    if inv == "crash":
        m = re.match(r"[^:\s]+:\d+", what)
        return m.group(0) if m else what
    if inv in ("plugin-error", "timeout"):
        return re.sub(r"`[^`]*`|\d+", "_", what)[:100]
    if inv == "invalid-state":
        return " ".join(sorted({norm_var(m.group("var"), types) for m in ID.finditer(what)}))
    if inv.startswith("oracle-"):
        # `s after L5 `...`: drawn <state> ("<tooltip>"); rustc: error[E0502]: ...` / `rustc accepts ...`
        m = re.match(r"(\S+) after L\d+ .*?: drawn ([^(]+?) \(", what)
        code = re.search(r"; rustc(?: accepts|: error\[(E\d+)\])", what)
        if not m:
            return re.sub(r"`[^`]*`|\d+", "_", what)[:100]
        return "%s %s / %s" % (norm_var(m.group(1), types), m.group(2),
                               (code.group(1) or "accepts") if code else "?")
    if inv == "outside-figure":
        m = re.search(r": (\w+) = ", what)
        return m.group(1) if m else ""
    ids = []
    for m in ID.finditer(what):
        if inv in GEOMETRIC:
            s = m.group("kind") or ""
        else:
            s = norm_var(m.group("var"), types)
            if m.group("kind"):
                s += ":" + m.group("kind")
            if m.group("detail"):
                s += ":" + m.group("detail")
        if s not in ids:
            ids.append(s)
    return " / ".join(ids[:3])


def program_keys(fails):
    """One (check, signature) per check a program fails: the smallest
    signature, so a program with one bug shows up in one bucket per check
    rather than one per column it touches."""
    by = defaultdict(set)
    for inv, sig in fails:
        by[inv].add(sig)
    return sorted((inv, min(sigs)) for inv, sigs in by.items())


def unescape_debug(s):
    try:
        return json.loads('"' + s + '"')
    except ValueError:
        return s.replace('\\"', '"').replace("\\n", "\n")


def parse_check_output(out, key=lambda n: n):
    """{program: [(check, what)]} from `corpus_invariants_hold`'s output:
    its NEW paragraphs and its list of snippets the plugin failed on (a
    panic becomes ("crash", "file.rs:line: message"))."""
    found = defaultdict(list)
    cur = None
    for line in out.splitlines():
        m = re.match(r"^NEW  (\S+): ([a-z-]+) \(", line)
        if m:
            cur = (key(m.group(1)), m.group(2))
            continue
        if cur and line.startswith("       "):
            found[cur[0]].append((cur[1], line.strip()))
            continue
        m = re.match(r'^    "(.*)",$', line)
        if m:
            msg = unescape_debug(m.group(1))
            name, _, err = msg.partition(" failed: ")
            pm = re.search(r"internal error at (.+?)\)\. Please report", err, re.S)
            if pm:
                found[key(name)].append(("crash", pm.group(1)))
            elif "timed out" in err.lower():
                found[key(name)].append(("timeout", "timeout"))
            else:
                first = [l for l in err.splitlines() if l.strip()]
                found[key(name)].append(("plugin-error", (first[0] if first else "?")[:160]))
        cur = None
    return dict(found)


def run_checks(paths, exe, plugin_env, workdir):
    """Run the corpus checks on `paths` (.rs files in one directory).
    Returns {stem path: [(inv, what)]}; crashes and plugin errors appear
    as ("crash", "file.rs:line") / ("plugin-error", first line)."""
    d = tempfile.mkdtemp(prefix="chk-", dir=workdir)
    for p in paths:
        with open(p) as src, open(os.path.join(d, os.path.basename(p)), "w") as dst:
            dst.write(src.read())
    d = os.path.realpath(d)
    env = dict(os.environ, **plugin_env, RV_INVARIANTS_EXTRA=d, RV_INVARIANTS_ONLY=d, RV_RUNNER="local")
    # the test names a file by its canonical path, without `.rs`
    copies = {os.path.join(d, os.path.basename(p))[:-3]: p[:-3] for p in paths}
    found = defaultdict(list)
    tests = ["corpus_invariants_hold"] + (["corpus_oracle_holds"] if ORACLE else [])
    for test in tests:
        p = subprocess.run([exe, test, "--exact", "--nocapture", "--test-threads=1"],
                           env=dict(env, RV_ORACLE="1"), capture_output=True, text=True, cwd=REPO)
        out = p.stdout + p.stderr
        if "running 0 tests" in out:
            raise RuntimeError("%s isn't in %s (a stale PROGEN_TEST_BIN?)" % (test, exe))
        got = parse_check_output(out, lambda name: copies.get(name, name))
        if p.returncode != 0 and not got and "violations differ" not in out and "Snippets the plugin" not in out:
            raise RuntimeError("check run failed:\n" + out[-3000:])
        for k, v in got.items():
            # a crash shows up in both tests' lists
            found[k].extend(x for x in v if x not in found[k])
    return dict(found)


# ─── shrinking ────────────────────────────────────────────────────────

def stmt_slots(block, out):
    """Every (block, index) in pre-order."""
    for i, s in enumerate(block.stmts):
        out.append((block, i))
        for c in s.blocks():
            stmt_slots(c, out)
    return out


def tree_candidates(prog):
    """Programs one edit smaller than `prog`."""
    n = len(stmt_slots(prog.body, []))
    out = []
    for k in range(n):
        s = stmt_slots(prog.body, [])[k]
        stmt = s[0].stmts[s[1]]
        ops = [("del", None)]
        for j, b in enumerate(stmt.blocks()):
            if b.tail is None:
                ops.append(("hoist", j))
        for j in range(len(stmt.alts)):
            ops.append(("alt", j))
        if stmt.inline:
            ops.append(("noinline", None))
        for op, j in ops:
            c = copy.deepcopy(prog)
            blk, i = stmt_slots(c.body, [])[k]
            st = blk.stmts[i]
            if op == "del":
                del blk.stmts[i]
            elif op == "hoist":
                blk.stmts[i:i + 1] = st.blocks()[j].stmts
            elif op == "alt":
                blk.stmts[i] = st.alts[j]
            else:
                st.inline = False
                continue  # same size; not a reduction
            out.append(c)
    for i in range(len(prog.params)):
        c = copy.deepcopy(prog)
        del c.params[i]
        out.append(c)
    if prog.fn_tail and prog.fn_tail != "Some(0)":
        c = copy.deepcopy(prog)
        c.fn_tail = "Some(0)"
        out.append(c)
    return out


def line_candidates(text):
    lines = text.split("\n")
    out = []
    for i, l in enumerate(lines):
        st = l.strip()
        if not st or st.startswith("fn ") or st == "}" and l == "}":
            continue
        out.append("\n".join(lines[:i] + lines[i + 1:]))
        # a call's or literal's arguments: drop one `, x`
        for m in re.finditer(r", [^,()]+(?=\))", l):
            out.append("\n".join(lines[:i] + [l[:m.start()] + l[m.end():]] + lines[i + 1:]))
    # whole helper items that nothing names any more
    return out


def size(text):
    return (text.count("\n"), len(text))


class Checker:
    def __init__(self, exe, plugin_env, workdir):
        self.exe, self.plugin_env, self.workdir = exe, plugin_env, workdir
        self.n = 0

    def evaluate(self, texts, types):
        """For each text: None if it doesn't compile, else its failures."""
        d = tempfile.mkdtemp(prefix="cand-", dir=self.workdir)
        with cf.ThreadPoolExecutor(JOBS) as ex:
            oks = list(ex.map(lambda t: compiles(t, d)[0], texts))
        paths = []
        for k, (t, ok) in enumerate(zip(texts, oks)):
            if ok:
                p = os.path.join(d, "c%03d.rs" % k)
                open(p, "w").write(t)
                paths.append(p)
        found = run_checks(paths, self.exe, self.plugin_env, self.workdir) if paths else {}
        res = []
        for k, (t, ok) in enumerate(zip(texts, oks)):
            if not ok:
                res.append(None)
            else:
                stem = os.path.join(d, "c%03d" % k)
                res.append([(inv, signature(inv, w, types)) for inv, w in found.get(stem, [])])
        return res


def shrink(prog, target, checker, deadline):
    """Smallest program (tree edits, then line edits) that still compiles
    and still fails with `target` = (check, signature)."""
    types = prog.types
    best_tree, best = prog, prog.render()
    rounds = 0
    while time.time() < deadline:
        cands = tree_candidates(best_tree)
        if not cands:
            break
        texts = [c.render() for c in cands]
        res = checker.evaluate(texts, types)
        good = [(size(t), i) for i, (t, r) in enumerate(zip(texts, res)) if r is not None and target in r]
        rounds += 1
        if not good:
            break
        _, i = min(good)
        if size(texts[i]) >= size(best):
            break
        best_tree, best = cands[i], texts[i]
    # line phase
    while time.time() < deadline:
        cands = [t for t in line_candidates(best) if size(t) < size(best)]
        cands = sorted(set(cands), key=size)[:120]
        if not cands:
            break
        res = checker.evaluate(cands, types)
        good = [t for t, r in zip(cands, res) if r is not None and target in r]
        rounds += 1
        if not good:
            break
        best = min(good, key=size)
    return best, rounds


# ─── the pipeline ─────────────────────────────────────────────────────

def plugin_env():
    """Environment that makes `cargo rv-plugin` the one we mean: with
    PROGEN_PLUGIN_ROOT (a `cargo install --root`), that root's bin first
    on PATH and a CARGO_HOME without a stale plugin in it."""
    root = os.environ.get("PROGEN_PLUGIN_ROOT")
    if not root:
        return {}
    ch = os.path.join(os.path.abspath(root), "progen-cargo-home")
    os.makedirs(ch, exist_ok=True)
    return {"PATH": os.path.join(os.path.abspath(root), "bin") + os.pathsep + os.environ.get("PATH", ""),
            "CARGO_HOME": ch}


def cmd_gen(a):
    os.makedirs(a.out, exist_ok=True)
    for i in range(a.count):
        p = gen.generate(a.seed, i)
        open(os.path.join(a.out, "p%s_%05d.rs" % (a.seed, i)), "w").write(p.render())


def cmd_check(a):
    exe = test_binary()
    paths = sorted(os.path.join(a.dir, f) for f in os.listdir(a.dir) if f.endswith(".rs"))
    found = run_checks(paths, exe, plugin_env(), tempfile.mkdtemp())
    for stem, fs in sorted(found.items()):
        for inv, w in fs:
            print("%s: %s: %s" % (os.path.basename(stem), inv, w))


def cmd_run(a):
    t0 = time.time()
    deadline = t0 + a.time_limit if a.time_limit else float("inf")
    gen_deadline = t0 + a.time_limit * 0.55 if a.time_limit else float("inf")
    out = os.path.abspath(a.out)
    work = os.path.join(out, "work")
    os.makedirs(work, exist_ok=True)
    exe = test_binary()
    penv = plugin_env()
    log = open(os.path.join(out, "log.txt"), "w")

    def say(*m):
        print(*m, flush=True)
        print(*m, file=log, flush=True)
    say("progen: seed %s, %d programs, toolchain %s" % (a.seed, a.count, toolchain()))

    stats = Counter()
    rejects = Counter()
    tags = Counter()
    progs = {}
    failures = {}  # stem -> [(inv, sig, what)]
    chunk = 300
    i = 0
    progdir = os.path.join(work, "programs")
    os.makedirs(progdir, exist_ok=True)
    while i < a.count and time.time() < gen_deadline:
        idx = list(range(i, min(a.count, i + chunk)))
        i = idx[-1] + 1
        batch = [(k, gen.generate(a.seed, k)) for k in idx]
        stats["generated"] += len(batch)
        texts = [p.render() for _, p in batch]
        with cf.ThreadPoolExecutor(JOBS) as ex:
            res = list(ex.map(lambda t: compiles(t, work), texts))
        paths = []
        for (k, p), t, (ok, why) in zip(batch, texts, res):
            if not ok:
                rejects[why] += 1
                continue
            stats["compiled"] += 1
            tags.update(set(p.tags()))
            path = os.path.join(progdir, "p%05d.rs" % k)
            open(path, "w").write(t)
            progs[path[:-3]] = p
            paths.append(path)
        found = run_checks(paths, exe, penv, work)
        for stem, fs in found.items():
            failures[stem] = [(inv, signature(inv, w, progs[stem].types), w) for inv, w in fs]
        stats["checked"] += len(paths)
        say("  %d generated, %d compiled, %d failing so far (%.0fs)"
            % (stats["generated"], stats["compiled"], len(failures), time.time() - t0))

    # Buckets: (check, signature) -> programs.
    buckets = defaultdict(list)
    for stem, fs in failures.items():
        for key in program_keys([(inv, sig) for inv, sig, _ in fs]):
            buckets[key].append(stem)
    order = sorted(buckets, key=lambda k: (-len(buckets[k]), k))
    with open(os.path.join(out, "failures.json"), "w") as f:
        json.dump({os.path.basename(s): [list(x) for x in fs] for s, fs in sorted(failures.items())}, f, indent=1)
    say("%d failing programs, %d buckets" % (len(failures), len(order)))

    # Shrink the smallest program of each bucket.
    checker = Checker(exe, penv, work)
    results = {}

    def do(key):
        stems = sorted(buckets[key], key=lambda s: size(progs[s].render()))
        stem = stems[0]
        if time.time() > deadline - 5:
            return key, progs[stem].render(), stem, 0
        best, rounds = shrink(progs[stem], key, checker, deadline)
        return key, best, stem, rounds
    with cf.ThreadPoolExecutor(a.shrink_jobs) as ex:
        for key, best, stem, rounds in ex.map(do, order):
            results[key] = (best, stem, rounds)
            say("  shrunk %s %s: %d -> %d lines (%d rounds)"
                % (key[0], key[1][:60], progs[stem].render().count("\n"), best.count("\n"), rounds))

    # Report.
    bdir = os.path.join(out, "buckets")
    os.makedirs(bdir, exist_ok=True)
    summary = []
    seen_min = {}
    for n, key in enumerate(order, 1):
        best, stem, rounds = results[key]
        name = "%02d-%s" % (n, key[0])
        d = os.path.join(bdir, name)
        os.makedirs(d, exist_ok=True)
        open(os.path.join(d, "min.rs"), "w").write(best)
        open(os.path.join(d, "original.rs"), "w").write(progs[stem].render())
        whats = [w for inv, sig, w in failures[stem] if (inv, sig) == key]
        open(os.path.join(d, "failure.txt"), "w").write(
            "check: %s\nsignature: %s\nprograms: %d\noriginal: %s (seed %s)\n\n%s\n"
            % (key[0], key[1], len(buckets[key]), os.path.basename(stem), progs[stem].seed, "\n".join(whats)))
        tagc = Counter(t for s in buckets[key] for t in set(progs[s].tags()))
        summary.append({"bucket": name, "check": key[0], "signature": key[1], "count": len(buckets[key]),
                        "min_lines": best.count("\n"), "repro": os.path.join("buckets", name, "min.rs"),
                        "programs": [os.path.basename(s) for s in sorted(buckets[key])[:20]],
                        "common_tags": [t for t, c in tagc.most_common(8)],
                        "same_repro_as": seen_min.get(best)})
        seen_min.setdefault(best, name)
    stats["failing"] = len(failures)
    stats["buckets"] = len(order)
    stats["seconds"] = int(time.time() - t0)
    report = {"seed": a.seed, "stats": dict(stats), "rejected": dict(rejects.most_common()),
              "tag_coverage": dict(sorted(tags.items())), "buckets": summary}
    json.dump(report, open(os.path.join(out, "summary.json"), "w"), indent=1)
    with open(os.path.join(out, "summary.md"), "w") as f:
        f.write("# progen run: seed %s\n\n" % a.seed)
        f.write("%(generated)d generated, %(compiled)d compiled (rustc accepted), %(failing)d failing, "
                "%(buckets)d buckets, %(seconds)ds.\n\n" % Counter(stats, **{k: 0 for k in ("failing", "buckets")}))
        if summary:
            f.write("| bucket | check | signature | programs | smallest repro |\n|---|---|---|---|---|\n")
            for b in summary:
                f.write("| %s | %s | `%s` | %d | %s (%d lines)%s |\n" % (
                    b["bucket"], b["check"], b["signature"].replace("|", "\\|"), b["count"], b["repro"],
                    b["min_lines"], ", same as " + b["same_repro_as"] if b["same_repro_as"] else ""))
            for b in summary:
                if b["same_repro_as"]:
                    continue
                f.write("\n## %s\n\n`%s` / `%s`, %d programs\n\n```rust\n%s```\n"
                        % (b["bucket"], b["check"], b["signature"], b["count"],
                           open(os.path.join(out, b["repro"])).read()))
        else:
            f.write("No failures.\n")
        f.write("\nRejected by rustc: %s\n" % ", ".join("%s %d" % kv for kv in rejects.most_common(12)))
    say("wrote %s/summary.md (%d buckets)" % (out, len(order)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--count", type=int, default=1000)
    r.add_argument("--seed", default="1")
    r.add_argument("--out", required=True)
    r.add_argument("--time-limit", type=float, default=0, help="seconds for the whole run (0 = none)")
    r.add_argument("--shrink-jobs", type=int, default=4)
    g = sub.add_parser("gen")
    g.add_argument("--count", type=int, default=10)
    g.add_argument("--seed", default="1")
    g.add_argument("--out", required=True)
    c = sub.add_parser("check")
    c.add_argument("dir")
    for x in (r, c):
        x.add_argument("--oracle", action="store_true",
                       help="also check each drawn state against rustc (corpus_oracle_holds)")
    a = ap.parse_args()
    global ORACLE
    ORACLE = getattr(a, "oracle", False)
    {"run": cmd_run, "gen": cmd_gen, "check": cmd_check}[a.cmd](a)


if __name__ == "__main__":
    main()
