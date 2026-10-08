"""Pairwise comparison of all transcripts of a meeting (system channel / mic channel / all).

usage: python compare.py <meeting_dir>
Domain-term hits use count_terms from <meeting>/glossary.json.
"""
import json, re, glob, os, sys, itertools, collections
import jiwer
from glossary import load_glossary

if len(sys.argv) < 2:
    raise SystemExit(__doc__)
TC = sys.argv[1]   # the meeting folder

HALLUC = re.compile(r"продолжение следует|субтитр|спасибо за просмотр|dimatorzok|до новых встреч", re.I)


def norm(t):
    t = t.lower().replace("ё", "е")
    t = re.sub(r"[^\w\s]", " ", t)
    return " ".join(t.split())


def meetily(f):
    segs = json.load(open(os.path.join(TC, f), encoding="utf-8"))["segments"]
    segs = sorted(segs, key=lambda s: s["audio_start_time"])
    return [(s["source_device"], s["text"]) for s in segs]


def granola():
    out = []
    lines = open(os.path.join(TC, "granola.txt"), encoding="utf-8").read().split("Transcript:", 1)[1]
    for ln in lines.splitlines():
        if ":" not in ln:
            continue
        who, text = ln.split(":", 1)
        out.append(("Microphone" if who.strip() == "Me" else "System", text))
    return out


def asr(f):
    d = json.load(open(f, encoding="utf-8"))
    return [(s["source"], s["text"]) for s in d["segments"]]


TERMS = load_glossary(TC, "count_terms", "domain term hits")

sources = {}
for name, f in (("online", "transcripts_online.json"), ("offline", "transcripts_offline.json"),
                ("meetily", "transcripts.json")):
    if os.path.exists(os.path.join(TC, f)):
        sources[name] = meetily(f)
if os.path.exists(os.path.join(TC, "granola.txt")):
    sources["granola"] = granola()
for f in sorted(glob.glob(os.path.join(TC, "asr_runs", "asr_*.json"))):
    sources[os.path.basename(f)[4:-5]] = asr(f)

print("== sizes / hallucination-like segments ==")
texts = {}
for name, segs in sources.items():
    h = sum(1 for _, t in segs if HALLUC.search(t))
    for ch in ("System", "Microphone"):
        texts[(name, ch)] = norm(" ".join(t for c, t in segs if c == ch and not HALLUC.search(t)))
    texts[(name, "all")] = norm(" ".join(t for _, t in segs if not HALLUC.search(t)))
    print(f"{name:32s} segs={len(segs):4d} words sys={len(texts[(name,'System')].split()):5d} "
          f"mic={len(texts[(name,'Microphone')].split()):4d} halluc={h}")

print("\n== domain term hits (all channels) ==")
names = list(sources)
print(f"{'term':10s}" + "".join(f"{n[:12]:>13s}" for n in names))
for term, rx in TERMS.items():
    print(f"{term:10s}" + "".join(f"{len(re.findall(rx, texts[(n,'all')])):13d}" for n in names))

for ch in ("System", "Microphone"):
    print(f"\n== pairwise WER, {ch} channel (row = reference) ==")
    print(" " * 32 + "".join(f"{n[:12]:>13s}" for n in names))
    mean = collections.defaultdict(list)
    for a in names:
        row = []
        for b in names:
            if a == b:
                row.append("     -")
                continue
            w = jiwer.wer(texts[(a, ch)], texts[(b, ch)])
            mean[b].append(w)
            row.append(f"{w*100:6.1f}")
        print(f"{a:32s}" + "".join(f"{r:>13s}" for r in row))
    print("mean WER vs all others (lower = closer to consensus):")
    for n in sorted(names, key=lambda n: sum(mean[n]) / len(mean[n])):
        print(f"   {n:32s} {sum(mean[n])/len(mean[n])*100:5.1f}")
