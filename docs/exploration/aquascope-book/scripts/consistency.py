"""Compare Aquascope (interpreter steps, permission steps) with RustViz on the shared programs.
usage: python3 -I consistency.py <aqbook/book> <cov-dir>  -> prints per-program findings + summary"""
import re, html, json, glob, os, sys, collections
book, cov = sys.argv[1], sys.argv[2]

def attrs(a):
    return {k: json.loads(html.unescape(v)) for k, v in re.findall(r'data-([a-z-]+)="([^"]*)"', a)}

# ---------- Aquascope ----------
def embeds():
    for f in sorted(glob.glob(book + "/ch0*.html")) + [book + "/experiment-intro.html"]:
        h = open(f).read()
        ids = re.findall(r'<h2 id="[^"]*"><a class="header" href="[^"]*">([^<]*)</a>', h)
        ems = re.findall(r'<div class="aquascope-embed"([^>]*)>', h)
        for i, a in zip(ids, ems):
            yield html.unescape(i).strip(), attrs(a)

def line_starts(code):
    s = [0]
    for i, c in enumerate(code):
        if c == "\n": s.append(i + 1)
    return s

def interp_marks(d):
    code = d["code"]; st = line_starts(code)
    pos = lambda p: st[p["line"]] + p["column"]
    lineof = lambda idx: max(i for i, s in enumerate(st) if s <= idx)
    steps = d["responses"]["interpreter"]["Ok"]["steps"]
    out = []
    for idx in d["annotations"]["interp"]["state_locations"]:
        for j in range(len(steps) - 1, -1, -1):
            fr = steps[j]["stack"]["frames"][-1]
            if pos(fr["body_span"]["start"]) <= idx <= pos(fr["body_span"]["end"]) and idx > pos(fr["location"]["start"]):
                l = lineof(idx)
                out.append(dict(step=j, line=l + 1, before=code[st[l]:idx].strip() == "", state=steps[j]))
                break
    out.sort(key=lambda m: m["step"])
    for i, m in enumerate(out): m["label"] = f"L{i+1}"
    return out

def find(v, t, acc):
    if isinstance(v, dict):
        if v.get("type") == t: acc.append(v)
        for x in v.values(): find(x, t, acc)
    elif isinstance(v, list):
        for x in v: find(x, t, acc)
    return acc

def local_facts(loc):
    v = loc["value"]
    ptrs = find(v, "Pointer", [])
    kinds = set()
    for p in ptrs:
        seg = p["value"]["path"]["segment"]
        kinds.add("heap" if seg["type"] == "Heap" else "stack:" + str(seg["value"].get("local")))
    dangling = bool(find(v, "Unallocated", []))
    is_ref_like = v.get("type") == "Pointer"
    return dict(moved=any(len(p) == 0 for p in loc["moved_paths"]), ptr=sorted(kinds), dangling=dangling, is_ref=is_ref_like)

# permissions: per line, per path, which loans/moves started (High) or ended (Low)
def perm_events(d):
    ev = []
    for res in d["responses"]["permissions"]:
        ok = res.get("Ok")
        if not ok: continue
        for s in ok["steps"]:
            line = s["location"]["start"]["line"] + 1
            for tbl in s["state"]:
                for path, st in tbl["state"]:
                    for k in ("loan_read_refined", "loan_write_refined", "loan_drop_refined"):
                        if st[k]["type"] in ("High", "Low"):
                            ev.append((line, path, k.split("_")[1], "borrowed" if st[k]["type"] == "High" else "released"))
                    if st["path_moved"]["type"] == "High": ev.append((line, path, "move", "moved"))
                    if st["is_live"]["type"] == "Low": ev.append((line, path, "live", "dead"))
    return ev

# ---------- RustViz ----------
TAG = re.compile(r"<[^>]+>")
import xml.etree.ElementTree as ET
NS = "{http://www.w3.org/2000/svg}"
def rv_events(rid):
    h = open(f"{cov}/html/{rid}.html").read()
    i = h.find('id="tl_vis"'); i = h.rfind("<svg", 0, i); j = h.find("</svg>", i) + 6
    root = ET.fromstring(h[i:j])
    # row y -> source line, from the code panel (the plugin inserts unnumbered
    # rows, e.g. for events between two lines; they belong to the line above)
    rows = []; cur = 0
    for m in re.finditer(r'<text class="code" x="\d+" y="(\d+)"> <tspan fill="#AAA">([^<]*)<', h):
        num = m.group(2).strip()
        if num: cur = int(num)
        rows.append((float(m.group(1)) - 5, cur))
    def line_of(y):
        return min(rows, key=lambda r: abs(r[0] - y))[1] if rows else None
    ev = []
    def ycoord(el):
        tag = el.tag.replace(NS, "")
        if tag == "circle": return float(el.get("cy"))
        if tag in ("polyline", "polygon"):
            pts = [float(v) for v in re.split(r"[ ,]+", el.get("points").strip())]
            ys = pts[1::2]
            return sum(ys) / len(ys) if max(ys) - min(ys) < 1 else None  # horizontal arrows only
        if tag == "text" and el.get("y"): return float(el.get("y")) - 5
        if tag == "use" and el.get("y"): return float(el.get("y"))
        if tag == "g":
            for c in el.iter():
                if c is el: continue
                y = ycoord(c)
                if y is not None: return y
        return None
    for el in root.iter():
        t = el.get("data-tooltip-text")
        if not t: continue
        y = ycoord(el)
        if y is None: continue
        line = int((y - 85) / 30 + 1.0001 + 0.5) if False else None
        # rows are 30px apart, row n centred at 85+30(n-1); an event at a row
        # boundary (y = centre+15) belongs to the line above (the end of it)
        line = line_of(y)
        txt = TAG.sub("", html.unescape(t))
        for part in [p.strip() for p in txt.split("\u2022") if p.strip()]:
            ev.append((line, part))
    return ev

def rv_index(ev):
    moves, ends, drops, oos = collections.defaultdict(list), collections.defaultdict(list), collections.defaultdict(list), collections.defaultdict(list)
    starts = collections.defaultdict(list)
    for line, t in ev:
        m = re.match(r"Move from (\S+) to (\S+)", t)
        if m: moves[m.group(1)].append(line)
        m = re.match(r"(\S+)'s resource is moved", t)
        if m: moves[m.group(1)].append(line)
        m = re.match(r"(?:(\S+)'s (?:resource|value)|the (?:resource|value) (\S+) points to) is no longer (im)?mutably borrowed", t)
        if m: ends[m.group(1) or m.group(2)].append(line)
        m = re.match(r"Return (?:im)?mutably borrowed (?:resource|value) from (\S+) to (\S+)", t)
        if m: ends[m.group(2)].append(line); ends["ref:" + m.group(1)].append(line)
        m = re.match(r"(\S+)'s (im)?mutable borrow ends", t)
        if m: ends["ref:" + m.group(1)].append(line)
        m = re.match(r"(?:(\S+)'s (?:resource|value)|the (?:resource|value) (\S+) points to) is (im)?mutably borrowed", t)
        if m: starts[m.group(1) or m.group(2)].append(line)
        m = re.match(r"(\S+) goes out of scope\. Its resource is dropped", t)
        if m: drops[m.group(1)].append(line)
        m = re.match(r"(\S+) goes out of scope", t)
        if m: oos[m.group(1)].append(line)
    return dict(moves=moves, ends=ends, drops=drops, oos=oos, starts=starts)

# ---------- compare ----------
summary = collections.Counter()
for rid, d in embeds():
    if not os.path.exists(f"{cov}/html/{rid}.html"):
        print("!! no RustViz output for", rid); continue
    rv = rv_index(rv_events(rid))
    notes = []
    if "interpreter" in d["operations"]:
        for m in interp_marks(d):
            L = m["line"] - (1 if m["before"] else 0)  # last line executed at this snapshot (text order)
            for fi, fr in enumerate(m["state"]["stack"]["frames"]):
                lo, hi = fr["body_span"]["start"]["line"] + 1, fr["body_span"]["end"]["line"] + 1
                inb = lambda xs: [x for x in xs if lo <= x <= hi]
                if not (lo <= m["line"] <= hi):
                    continue  # only compare a frame at a mark inside that frame's body (text order == execution order there)
                for loc in fr["locals"]:
                    n = loc["name"]; f = local_facts(loc)
                    rv_moved = [x for x in inb(rv["moves"].get(n, [])) if x <= L]
                    if f["moved"] and not rv["moves"].get(n):
                        notes.append(f"{m['label']}@{m['line']}: Aquascope shows `{n}` moved, RustViz draws no move of `{n}`"); summary["move: aq-only"] += 1
                    if (not f["moved"]) and rv_moved and fi == len(m["state"]["stack"]["frames"]) - 1:
                        notes.append(f"{m['label']}@{m['line']}: RustViz moved `{n}` at line {rv_moved[0]}, Aquascope still shows it live"); summary["move: rv-only"] += 1
                    rend = [x for x in inb(rv["ends"].get("ref:" + n, [])) if x < m["line"]]
                    if f["is_ref"] and rend and not f["dangling"]:
                        notes.append(f"{m['label']}@{m['line']}: `{n}` still drawn as a live pointer ({','.join(f['ptr'])}); RustViz ended its borrow at line {rend[0]}"); summary["ref outlives borrow (runtime vs NLL)"] += 1
                    if f["dangling"] and not f["moved"]:
                        notes.append(f"{m['label']}@{m['line']}: `{n}` holds a dangling pointer (⦻); RustViz: borrow ended at line {(rv['ends'].get('ref:'+n) or ['?'])[0]}"); summary["dangling-but-dead pointer shown"] += 1
                    dr = [x for x in inb(rv["drops"].get(n, [])) if x < m["line"]]
                    if dr and "heap" in f["ptr"] and not f["moved"]:
                        notes.append(f"{m['label']}@{m['line']}: RustViz dropped `{n}` at line {dr[0]}, Aquascope still shows its heap data"); summary["drop timing"] += 1
    if "permissions" in d["operations"]:
        for line, path, kind, what in perm_events(d):
            if "." in path or "[" in path or path.startswith("*"): continue
            if what == "released" and kind == "write":
                r = rv["ends"].get(path, []) + [x for k, v in rv["ends"].items() if k.startswith(path + ".") for x in v]
                if line not in r:
                    notes.append(f"perm: `{path}` regains W after line {line}; RustViz ends the borrow of `{path}` at {r or 'no line'}"); summary["borrow end differs"] += 1
                else: summary["borrow end agrees"] += 1
            if what == "moved":
                r = rv["moves"].get(path, [])
                if line not in r:
                    notes.append(f"perm: `{path}` moved at line {line}; RustViz move lines {r or 'none'}"); summary["move line differs"] += 1
                else: summary["move line agrees"] += 1
    print(f"== {rid} [{'+'.join(d['operations'])}]" + ("" if notes else "  (no disagreement found)"))
    for x in dict.fromkeys(notes): print("   -", x)
print("\nSUMMARY", dict(summary))
