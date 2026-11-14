"""usage: python3 -I markup.py <proto-src-dir> : add RustViz fence flags to ch04 (from pristine upstream copies)"""
import re, sys, os
src = sys.argv[1]
# (file, aquascope block index | rust fence line) -> flag
AQ = {
    ("ch04-01-what-is-ownership.md", 4): "rustviz",
    ("ch04-01-what-is-ownership.md", 6): "rustviz",
    ("ch04-01-what-is-ownership.md", 7): "rustviz",
    ("ch04-02-references-and-borrowing.md", 2): "rustviz-only",
    ("ch04-02-references-and-borrowing.md", 3): "rustviz",
    ("ch04-02-references-and-borrowing.md", 10): "rustviz-default",
    ("ch04-02-references-and-borrowing.md", 15): "rustviz",
    ("ch04-04-slices.md", 1): "rustviz",
    ("ch04-04-slices.md", 4): "rustviz-default",
}
RS = {
    ("ch04-03-fixing-ownership-errors.md", 169): "rustviz",
    ("ch04-03-fixing-ownership-errors.md", 184): "rustviz",
    ("ch04-03-fixing-ownership-errors.md", 198): "rustviz",
    ("ch04-04-slices.md", 24): "rustviz",
    ("ch04-04-slices.md", 221): "rustviz-default",
    ("ch04-05-ownership-recap.md", 52): "rustviz-only",
}
for f in sorted({k[0] for k in list(AQ) + list(RS)}):
    p = os.path.join(src, f)
    lines = open(p).read().split("\n")
    n = 0
    for i, l in enumerate(lines):
        if l.startswith("```aquascope"):
            n += 1
            if (f, n) in AQ:
                lines[i] = l + "," + AQ[(f, n)]
        elif l.startswith("```rust") and (f, i + 1) in RS:
            lines[i] = l + "," + RS[(f, i + 1)]
    open(p, "w").write("\n".join(lines))
    print(f, "done")
