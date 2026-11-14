"""Extract Aquascope blocks and ownership-heavy listings from the rust-book clone.
usage: python3 -I extract.py <rust-book-dir> <out-dir>"""
import json, os, re, sys, glob

book, out = sys.argv[1], sys.argv[2]
os.makedirs(out + "/aq", exist_ok=True)
os.makedirs(out + "/ls", exist_ok=True)
MARK = re.compile(r"`\[[^\]]*\]`|`\([^)]*\)`|`\{[^}]*\}`")

def strip(code):
    lines = []
    for line in code.split("\n"):
        if line.startswith("#"):
            line = line[1:]
        elif line.startswith("\\#"):
            line = "#" + line[2:]
        else:
            line = MARK.sub("", line)
        lines.append(line)
    return "\n".join(lines).strip() + "\n"

items = []
for md in sorted(glob.glob(book + "/src/*.md")):
    text = open(md).read()
    name = os.path.basename(md)[:-3]
    pos = 0; n = 0
    while True:
        i = text.find("```aquascope", pos)
        if i < 0: break
        nl = text.index("\n", i)
        header = text[i+3:nl]
        j = text.index("```", nl)
        body = text[nl+1:j]
        pos = j + 3
        n += 1
        commented = text[max(0, i-5):i].strip().endswith("<!--")
        flags = header.split(",")[1:]
        fid = f"{name}__aq{n:02d}"
        open(f"{out}/aq/{fid}.rs", "w").write(strip(body))
        items.append(dict(id=fid, kind="aquascope", chapter=name[:4], file=name, flags=flags,
                          should_fail="shouldFail" in flags, commented=commented,
                          line=text[:i].count("\n") + 1))

CH = ["ch04", "ch05", "ch06", "ch08", "ch10", "ch13", "ch15", "ch16"]
srcs = {os.path.basename(m)[:-3]: open(m).read() for m in glob.glob(book + "/src/*.md")}
for ch in CH:
    for d in sorted(glob.glob(f"{book}/listings/{ch}-*/*/")):
        lid = os.path.basename(d.rstrip("/"))
        rs = [p for p in glob.glob(d + "src/**/*.rs", recursive=True)]
        main = d + "src/main.rs" if os.path.exists(d + "src/main.rs") else (d + "src/lib.rs" if os.path.exists(d + "src/lib.rs") else None)
        cargo = open(d + "Cargo.toml").read() if os.path.exists(d + "Cargo.toml") else ""
        deps = re.findall(r"^\s*([A-Za-z0-9_-]+)\s*=", cargo.split("[dependencies]")[1], re.M) if "[dependencies]" in cargo else []
        outp = open(d + "output.txt").read() if os.path.exists(d + "output.txt") else ""
        refd = [k for k, v in srcs.items() if f"/{lid}/" in v and f"listings/{os.path.basename(os.path.dirname(d.rstrip('/')))}" in v]
        fence = ""
        for k, v in srcs.items():
            ls_ = v.split("\n")
            for idx, line in enumerate(ls_):
                if f"/{lid}/" in line and "listings/" in line:
                    for back in range(1, 4):
                        if idx-back >= 0 and ls_[idx-back].startswith("```"):
                            fence = ls_[idx-back][3:]; break
                    break
            if fence: break
        if main is None:
            continue
        code = open(main).read()
        code = "\n".join(l for l in code.split("\n") if not re.match(r"\s*// ANCHOR(_END)?:", l))
        fid = f"{ch}__{lid}"
        open(f"{out}/ls/{fid}.rs", "w").write(code)
        items.append(dict(id=fid, kind="listing", chapter=ch, file=lid, n_rs=len(rs), deps=deps,
                          book_says_error=("error[" in outp or "error:" in outp) and "panicked" not in outp,
                          book_panics="panicked" in outp, referenced=sorted(refd),
                          is_lib=main.endswith("lib.rs"), fence=fence))
json.dump(items, open(out + "/items.json", "w"), indent=1)
print(len([i for i in items if i["kind"] == "aquascope"]), "aquascope;", len([i for i in items if i["kind"] == "listing"]), "listings")
