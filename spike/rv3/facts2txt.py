#!/usr/bin/env python3
"""facts2txt.py <out-dir> <key>: per-row text view of a program's RV3 facts next to its source."""
import json, sys, os
d, key = sys.argv[1], sys.argv[2]
src = open(os.path.join(d, key + ".rs")).read().splitlines()
bodies = json.load(open(os.path.join(d, key + ".facts.json")))
def fact(f):
    e = f["ev"]
    if e == "Init": return "%s %s <- %s%s" % ("REASSIGN" if f["reassign"] else "INIT", f["path"], f["from"], " (maybe had none)" if f["maybe_had"] and f["reassign"] else "")
    if e == "Move": return "MOVE %s -> %s" % (f["path"], f["to"])
    if e == "Copy": return "COPY %s -> %s" % (f["path"], f["to"])
    if e == "Borrow": return "BORROW L%d %s %s -> %s" % (f["loan"], f["kind"], f["lender"], f["holder"])
    if e == "BorrowEnd": return "END L%d (%s->%s) %s, path continues at row %d" % (f["loan"], f["lender"], f["holder"], f["why"], f["to_line"])
    if e == "Repoint": return "REPOINT %s: loans %s -> %s" % (f["holder"], f["old"], f["new"])
    if e == "Drop": return "DROP %s [%s]%s" % (f["path"], f["how"], " at reassign" if f["at_reassign"] else "")
    if e == "ScopeEnd": return "SCOPE-END %s (%s)" % (f["path"], f["note"])
    if e == "TempDrop": return "TEMP-DROP %s" % f["what"]
    return str(f)
for b in bodies:
    print("== fn %s (rows %d-%d)" % (b["func"], b["first_line"], b["last_line"]))
    print("   vars: " + ", ".join("%s:%s%s%s" % (v["name"], v["kind"], ",copy" if v["copy"] else "", ",drop" if v["needs_drop"] else "") for v in b["vars"]))
    rows = {r["line"]: r for r in b["rows"]}
    for ln in range(b["first_line"], b["last_line"] + 1):
        print("%3d | %s" % (ln, src[ln - 1] if ln <= len(src) else ""))
        r = rows.get(ln)
        if not r: continue
        st = " ".join("%s=%s" % kv for kv in r["state"].items())
        if st: print("    state: " + st)
        for k, v in r["lent"].items(): print("    lent : %s by %s" % (k, ", ".join(v)))
        for f in r["facts"]: print("    * " + fact(f))
    for n in b["notes"]: print("   note: " + n)
