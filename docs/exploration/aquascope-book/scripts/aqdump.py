"""usage: python3 -I aqdump.py page.html [n] -> list aquascope embeds; dump structure of embed n"""
import sys, re, html, json
h = open(sys.argv[1]).read()
embeds = re.findall(r'<div class="aquascope-embed"([^>]*)>', h)
def attrs(a):
    return {k: json.loads(html.unescape(v)) for k, v in re.findall(r'data-([a-z-]+)="([^"]*)"', a)}
for i, a in enumerate(embeds):
    d = attrs(a); print(i, d["operations"], d.get("config"), d["code"].splitlines()[1][:50] if len(d["code"].splitlines())>1 else "")
if len(sys.argv) > 2:
    d = attrs(embeds[int(sys.argv[2])])
    def shape(x, depth=0):
        if isinstance(x, dict): return {k: shape(v, depth+1) for k, v in list(x.items())[:12]} if depth < 7 else "{..}"
        if isinstance(x, list): return [shape(x[0], depth+1), f"...x{len(x)}"] if x else []
        return x
    print(json.dumps(shape(d["responses"]), indent=1)[:5000])
    print(json.dumps(d["annotations"]))
