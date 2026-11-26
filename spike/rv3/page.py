#!/usr/bin/env python3
"""page.py <out-dir> <key>: an HTML page showing a program three ways: today's drawing, today's renderer fed by
the MIR fact layer (straight-line bodies only), and the fact layer's per-row grid."""
import html, json, os, re, sys
d, key = sys.argv[1], sys.argv[2]
rd = lambda ext: open(os.path.join(d, key + ext)).read() if os.path.exists(os.path.join(d, key + ext)) else None
src = rd(".rs").splitlines(); facts = json.loads(rd(".facts.json"))
today, code, rv3, rv3code = rd(".today.svg"), rd(".code.svg"), rd(".rv3.svg"), rd(".rv3code.svg")
strip = lambda s: re.sub(r"<\?xml[^>]*\?>", "", s or "")
COL = {"own": "#2e7d32", "shared": "#64b5f6", "mut": "#e65100", "moved": "#bdbdbd", "maybe": "#9e9d24", "partly": "#8e24aa", "unassigned": "#eeeeee", "dropped": "#bdbdbd"}
def glyph(f):
    e = f["ev"]
    if e == "Init": return ("↺ " if f["reassign"] else "● ") + "%s ← %s" % (f["path"], f["from"]) + (" (old value maybe absent)" if f["reassign"] and f["maybe_had"] else "")
    if e == "Move": return "→ move %s ⇒ %s" % (f["path"], f["to"])
    if e == "Copy": return "⇢ copy %s ⇒ %s" % (f["path"], f["to"])
    if e == "Borrow": return "%s L%d %s lends to %s" % ("&mut" if f["kind"].startswith(("mut", "unique")) else "&", f["loan"], f["lender"], f["holder"] or "a call") + ("" if f["kind"] in ("shared", "mut") else " [%s]" % f["kind"])
    if e == "BorrowEnd": return "✕ L%d ends (%s → %s): %s%s" % (f["loan"], f["holder"] or "call", f["lender"], f["why"], "" if f["to_line"] == f.get("_row") else ", on the path to row %d" % f["to_line"])
    if e == "Repoint": return "⤳ %s re-pointed: loans %s → %s" % (f["holder"], f["old"], f["new"])
    if e == "Drop": return {"drops": "▼ drop %s", "drops_rest": "▼ drop rest of %s (fields moved)", "drops_if_init": "▽ drop %s only if it holds a value (drop flag)", "nothing_to_drop": "○ %s: nothing to drop"}[f["how"]] % f["path"] + (" (old value, at reassignment)" if f["at_reassign"] else "")
    if e == "ScopeEnd": return "○ %s out of scope (%s)" % (f["path"], f["note"])
    if e == "TempDrop": return "▼ temporary dropped: %s" % f["what"].split(" : ")[0]
    return str(f)
parts = []
for b in facts:
    owners = [v["name"] for v in b["vars"]]
    cols = []
    for r in b["rows"]:
        for p in r["state"]:
            if p not in cols and p.split(".")[0] in owners: cols.append(p)
    rows = {r["line"]: r for r in b["rows"]}
    t = ["<table class=g><tr><th>#</th><th>source</th>" + "".join("<th>%s</th>" % html.escape(c) for c in cols) + "<th>facts (MIR)</th></tr>"]
    for ln in range(b["first_line"], b["last_line"] + 1):
        r = rows.get(ln, {"state": {}, "lent": {}, "facts": []})
        cells = []
        for c in cols:
            s = r["state"].get(c)
            lent = [x for k, v in r["lent"].items() if k == c or k.startswith(c + ".") or c.startswith(k + ".") for x in v]
            if s == "own" and lent: s2 = "mut" if any(x.startswith("mut") for x in lent) else "shared"
            else: s2 = s
            title = (s or "") + ("; lent: " + ", ".join(lent) if lent else "")
            cells.append("<td title='%s'><span class='b' style='background:%s'></span>%s</td>" % (html.escape(title), COL.get(s2, "transparent") if s2 else "transparent", html.escape(s2 or "")))
        evs = "<br>".join(html.escape(glyph(dict(f, _row=ln))) for f in r["facts"])
        t.append("<tr><td class=n>%d</td><td class=src>%s</td>%s<td class=ev>%s</td></tr>" % (ln, html.escape(src[ln - 1]) if ln <= len(src) else "", "".join(cells), evs))
    t.append("</table>")
    parts.append("<h3>fn %s</h3>%s" % (html.escape(b["func"]), "".join(t)))
straight = rv3 and all(b.get("straight_line") for b in facts)
page = """<!doctype html><meta charset=utf-8><title>%s</title><style>
body{font:13px -apple-system,Helvetica,sans-serif;margin:16px;background:#fff;color:#111}
.row{display:flex;gap:24px;align-items:flex-start}.panel{border:1px solid #ddd;padding:8px}
.panel h2{font-size:14px;margin:0 0 6px}.pair{display:flex}
table.g{border-collapse:collapse}table.g td,table.g th{border:1px solid #e3e3e3;padding:2px 6px;vertical-align:top}
td.src{font:12px Menlo,monospace;white-space:pre}td.n{color:#888}td.ev{font:12px Menlo,monospace;color:#333}
span.b{display:inline-block;width:6px;height:14px;margin-right:4px;vertical-align:middle}
</style><h1 style='font-size:16px'>%s</h1><div class=row>
<div class=panel><h2>Today (HIR inference), main 5dd00b2d2</h2><div class=pair>%s%s</div></div>%s</div>
<div class=panel style='margin-top:16px'><h2>MIR fact layer (rv3_facts.rs): state after each row, per path; lent = loans in scope (NLL Borrows)</h2>%s</div>
""" % (key, key, strip(code), strip(today),
       ("<div class=panel><h2>Today's renderer, events synthesized from MIR facts (rv3_events.rs)</h2><div class=pair>%s%s</div></div>" % (strip(rv3code), strip(rv3))) if straight else "<div class=panel style='max-width:420px'><h2>MIR-fed render: not attempted</h2>This program has conditional control flow; the spike's event synthesizer does not emit <code>Branch</code> events, so only the fact grid below shows the MIR answer.</div>",
       "".join(parts))
open(os.path.join(d, key + ".html"), "w").write(page)
print(os.path.join(d, key + ".html"))
