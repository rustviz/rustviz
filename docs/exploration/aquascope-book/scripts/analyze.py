"""usage: python3 -I analyze.py <cov-dir>  -> writes table.md, table.json"""
import json, sys, re, collections, os
cov = sys.argv[1]
items = {i["id"]: i for i in json.load(open(cov + "/items.json"))}
res = dict(l.split() for l in open(cov + "/results.txt"))
rows = []
for iid, it in items.items():
    code = int(res[iid]); err = open(f"{cov}/err/{iid}.txt").read()
    if code == 0:
        h = open(f"{cov}/html/{iid}.html").read()
        n = h.count("data-tooltip-text=")
        st = "renders"; msg = f"{n} tooltips"
    elif "doesn't compile" in err:
        first = next((l for l in err.splitlines() if l.startswith("error")), "")
        st = "compile-error"; msg = first
    else:
        st = "refused"; msg = err.strip().replace("Error: ", "", 1).splitlines()[0] if err.strip() else "(no message)"
    expected_fail = it.get("should_fail") or it.get("book_says_error") or "does_not_compile" in it.get("fence", "")
    if st == "compile-error":
        if expected_fail: st = "compile-error (intended)"
        elif it["kind"] == "listing" and (it.get("deps") or it.get("n_rs", 1) > 1):
            st = "compile-error (needs deps/modules)"
        elif it["kind"] == "listing" and "ignore" in it.get("fence", ""):
            st = "compile-error (fragment)"
        else: st = "compile-error (other)"
    rows.append(dict(it, status=st, msg=msg))
json.dump(rows, open(cov + "/table.json", "w"), indent=1)
if len(sys.argv) > 2: rows = [r for r in rows if r["kind"] == "aquascope" or r["referenced"]]
by = collections.defaultdict(collections.Counter)
for r in rows:
    by[(r["kind"], r["chapter"])][r["status"]] += 1
cols = ["renders", "refused", "compile-error (intended)", "compile-error (fragment)", "compile-error (needs deps/modules)", "compile-error (other)"]
print("| kind | chapter | total | " + " | ".join(cols) + " |")
print("|---|---|---|" + "---|" * len(cols))
tot = collections.Counter()
for (k, ch), c in sorted(by.items()):
    print(f"| {k} | {ch} | {sum(c.values())} | " + " | ".join(str(c[x]) for x in cols) + " |")
    for x in cols: tot[(k, x)] += c[x]
for k in ["aquascope", "listing"]:
    print(f"| **{k} total** | | {sum(tot[(k,x)] for x in cols)} | " + " | ".join(str(tot[(k, x)]) for x in cols) + " |")
print()
for r in rows:
    if r["status"] not in ("renders", "compile-error (intended)"):
        print(f"- {r['status']}: {r['id']}  -- {r['msg']}")
