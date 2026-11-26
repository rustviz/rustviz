#!/usr/bin/env python3
"""run.py <plugin-root> <outdir> [programs-dir]: render each program with today's plugin (timeline + code SVG)
and dump the RV3 MIR facts (RV3_FACTS) from the same run."""
import glob, json, os, subprocess, sys, shutil, tempfile
from concurrent.futures import ThreadPoolExecutor
root, out = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
here = os.path.dirname(os.path.abspath(__file__))
progs = sorted(glob.glob(os.path.join(sys.argv[3] if len(sys.argv) > 3 else os.path.join(here, "programs"), "*.rs")))
repo = os.path.dirname(os.path.dirname(here))
toolchain = open(os.path.join(repo, "rust-toolchain.toml")).read()
os.makedirs(out, exist_ok=True)
work = tempfile.mkdtemp(prefix="rv3w-")
home = os.path.join(work, "home"); os.makedirs(home); os.symlink(os.path.join(root, "bin"), os.path.join(home, "bin"))
def run(path):
    key = os.path.basename(path)[:-3]
    proj = os.path.join(work, key); os.makedirs(os.path.join(proj, "src"))
    open(os.path.join(proj, "Cargo.toml"), "w").write('[package]\nname = "user-code"\nversion = "0.1.0"\nedition = "2024"\n')
    open(os.path.join(proj, "rust-toolchain.toml"), "w").write(toolchain)
    shutil.copy(path, os.path.join(proj, "src", "lib.rs"))
    env = {k: v for k, v in os.environ.items() if k not in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR")}
    env.update(CARGO_HOME=home, CARGO_TERM_COLOR="never", RV3_FACTS=os.path.join(out, key + ".facts.json"))
    p = subprocess.run(["cargo", "rv-plugin"], cwd=proj, env=env, capture_output=True, text=True)
    if p.returncode != 0:
        open(os.path.join(out, key + ".err"), "w").write(p.stderr)
        return key + ": FAIL"
    try:
        o = json.loads(p.stdout)
        open(os.path.join(out, key + ".today.svg"), "w").write(o["timeline_panel"])
        open(os.path.join(out, key + ".code.svg"), "w").write(o["code_panel"])
    except Exception as e:
        open(os.path.join(out, key + ".err"), "w").write(str(e) + "\n" + p.stdout[:2000] + p.stderr)
        return key + ": BAD OUTPUT"
    shutil.copy(path, os.path.join(out, key + ".rs"))
    env2 = dict(env, RV3_RENDER="1", RV3_FACTS="")
    import time; time.sleep(1.1); os.utime(os.path.join(proj, "src", "lib.rs"))  # cargo's check cache
    q = subprocess.run(["cargo", "rv-plugin"], cwd=proj, env=env2, capture_output=True, text=True)
    try:
        o = json.loads(q.stdout)
        open(os.path.join(out, key + ".rv3.svg"), "w").write(o["timeline_panel"])
        open(os.path.join(out, key + ".rv3code.svg"), "w").write(o["code_panel"])
    except Exception as e:
        open(os.path.join(out, key + ".rv3.err"), "w").write(str(e) + "\n" + q.stderr[-3000:])
        return key + ": ok (rv3 render FAILED)"
    return key + ": ok"
with ThreadPoolExecutor(8) as ex:
    for r in ex.map(run, progs):
        print(r)
