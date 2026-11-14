"""usage: python3 -I prose.py <src-dir> : ch04 compiling blocks with the prose that follows"""
import re, glob, os, sys
for md in sorted(glob.glob(sys.argv[1] + '/ch04-0*.md')):
    t = open(md).read(); n = 0
    for m in re.finditer(r'(?ms)^```(aquascope[^\n]*|rust[^\n]*)\n(.*?)^```', t):
        hdr = m.group(1)
        if hdr.startswith('aquascope'): n += 1
        if 'shouldFail' in hdr or 'does_not_compile' in hdr: continue
        if hdr.startswith('rust') and 'ignore' in hdr: continue
        after = t[m.end():m.end()+500].replace('\n', ' ')
        L = 'L-ref' if re.search(r'\bL\d\b', after) else ''
        lid = os.path.basename(md)[:7] + ('#%02d' % n if hdr.startswith('aquascope') else '@%d' % (t[:m.start()].count('\n')+1))
        body = ' | '.join(x.strip() for x in m.group(2).strip().split('\n') if x.strip())
        print(lid, hdr[:50], L, '\n   CODE:', body[:200], '\n   NEXT:', after[:260])
