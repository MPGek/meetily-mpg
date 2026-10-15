"""One row per source: WER vs references, hallucinations, loops, Latin share, domain terms.

usage: python compare2.py <meeting_dir>   (same argument as compare.py, which it imports)
"""
import contextlib, io, re
import jiwer
with contextlib.redirect_stdout(io.StringIO()):
    import compare as C

FILLERS = {"э", "эм", "ээ", "эээ", "мм", "м"}
LOOP = re.compile(r"\b(\w+(?:\s+\w+){0,3})(?:\s+\1\b){3,}")  # 1-4 gram repeated 4+ times in a row


def strip(t):
    return " ".join(w for w in t.split() if w not in FILLERS)


def raw_all(name):
    return C.sources[name]


refs = [r for r in ("granola", "gigaam-v3-e2e-rnnt") if r in C.sources]
print(f"{'source':30s} {'words':>6s} {'hall':>5s} {'loop':>5s} {'latin%':>7s} {'terms':>6s} "
      + "".join(f"{'WER/'+r[:7]:>13s}" for r in refs))
for n in C.names:
    segs = raw_all(n)
    hall = sum(1 for _, t in segs if C.HALLUC.search(t))
    loops = sum(1 for _, t in segs if LOOP.search(C.norm(t)))
    words = C.texts[(n, "all")].split()
    latin = sum(1 for w in words if re.fullmatch(r"[a-z][a-z0-9]*", w)) / max(1, len(words)) * 100
    terms = sum(len(re.findall(rx, C.texts[(n, "all")])) for rx in C.TERMS.values())
    row = f"{n:30s} {len(words):6d} {hall:5d} {loops:5d} {latin:7.2f} {terms:6d} "
    for r in refs:
        row += f"{'-':>13s}" if r == n else f"{jiwer.wer(strip(C.texts[(r,'System')]), strip(C.texts[(n,'System')]))*100:13.1f}"
    print(row)
