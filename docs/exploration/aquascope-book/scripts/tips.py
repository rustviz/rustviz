"""usage: python3 -I tips.py file.html -> sorted unique tooltip texts (tags stripped)"""
import sys, re, html
h = open(sys.argv[1]).read()
seen = []
for m in re.finditer(r'data-tooltip-text="([^"]*)"', h):
    t = re.sub(r"<[^>]+>", "", html.unescape(m.group(1)))
    if t not in seen: seen.append(t)
print("\n".join("  - " + t for t in seen))
