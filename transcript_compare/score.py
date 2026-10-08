"""Score every transcript of a meeting against its hand-corrected reference/ref_*.txt fragments.

usage: python score.py <meeting_dir>                                 # WER table
       python score.py <meeting_dir> --show <source> <fragment_id>   # aligned diff for one source/fragment

Sources: <meeting>/transcripts*.json (Meetily), granola.txt, asr_runs/asr_*.json.
"""
import glob, json, os, re, sys
from num2words import num2words

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from prepare import load_config  # noqa: E402

FILLERS = {"э", "эм", "ээ", "эээ", "мм", "м", "ммм", "хм", "а-а", "э-э"}
WILD = "<unk>"
HALLUC = re.compile(r"продолжение следует|субтитр|спасибо за просмотр|dimatorzok", re.I)


def norm(text):
    t = text.lower().replace("ё", "е").replace("[неразб]", f" {WILD} ")
    t = re.sub(r"(\d)[\s.,](?=\d{3}\b)", r"\1", t)          # 31 535 / 31.535 -> 31535
    t = re.sub(r"\d+", lambda m: " " + num2words(int(m.group()), lang="ru") + " ", t)
    t = re.sub(r"[^\w<>]+", " ", t.replace("-", " "))
    return [w for w in t.split() if w not in FILLERS and w not in {"э", "м"}]


_TRANSLIT = [("sh", "ш"), ("ch", "ч"), ("th", "т"), ("ph", "ф"), ("oo", "у"), ("ee", "и"), ("ea", "и"), ("ou", "ау"),
             ("ai", "ей"), ("ay", "ей"), ("igh", "ай"), ("x", "кс"), ("j", "дж"), ("c", "к"), ("q", "к"), ("w", "в"),
             ("y", "и"), ("a", "а"), ("b", "б"), ("d", "д"), ("e", "е"), ("f", "ф"), ("g", "г"), ("h", "х"), ("i", "и"),
             ("k", "к"), ("l", "л"), ("m", "м"), ("n", "н"), ("o", "о"), ("p", "п"), ("r", "р"), ("s", "с"), ("t", "т"),
             ("u", "у"), ("v", "в"), ("z", "з")]


def translit_hits(ops):
    """Substitutions of a Latin reference word by its Cyrillic spelling (hotfix→хотфикс): right word, other script."""
    from rapidfuzz.fuzz import ratio
    n = 0
    for r, h in ops:
        if r and h and re.fullmatch(r"[a-z]+", r) and not re.fullmatch(r"[a-z0-9]+", h):
            t = r
            for a, b in _TRANSLIT:
                t = t.replace(a, b)
            n += ratio(t, h) >= 60
    return n


def load_ref(meeting, fid):
    words = []
    for ln in open(os.path.join(meeting, "reference", f"ref_{fid:02d}.txt"), encoding="utf-8"):
        if ln.lstrip().startswith("#"):
            continue
        words += norm(re.sub(r"^\s*\[\d\d:\d\d\]", "", ln))
    return words


def load_sources(meeting):
    src = {}
    for f in sorted(glob.glob(os.path.join(meeting, "transcripts*.json"))):   # transcripts_online.json -> "online"
        segs = json.load(open(f, encoding="utf-8"))["segments"]
        name = os.path.basename(f)[:-5].replace("transcripts_", "").replace("transcripts", "meetily")
        src[name] = [(s["audio_start_time"], s["audio_end_time"], s["source_device"], s["text"]) for s in segs]
    if os.path.exists(os.path.join(meeting, "granola.txt")):
        g = open(os.path.join(meeting, "granola.txt"), encoding="utf-8").read().split("Transcript:", 1)[1]
        src["granola"] = [(None, None, "Microphone" if ln.split(":", 1)[0].strip() == "Me" else "System",
                           ln.split(":", 1)[1]) for ln in g.splitlines() if ":" in ln]
    for f in sorted(glob.glob(os.path.join(meeting, "asr_runs", "asr_*.json"))):
        d = json.load(open(f, encoding="utf-8"))
        src[os.path.basename(f)[4:-5]] = [(s["start"], s["end"], s["source"], s["text"]) for s in d["segments"]]
    return src


def hyp_words(segs, ch, a, b, margin=None):
    # SCORE_MARGIN=0 keeps only rows whose own time span overlaps the fragment (checks timestamp mapping)
    margin = float(os.environ.get("SCORE_MARGIN", 150)) if margin is None else margin
    out = []
    for s, e, c, t in sorted(segs, key=lambda x: x[0] or 0):
        if c != ch or HALLUC.search(t):
            continue
        if s is not None and (e < a - margin or s > b + margin):
            continue
        out += norm(t)
    return out


def align(ref, hyp):
    """Semi-global edit distance: the ref must be fully consumed, hyp start/end are free."""
    m, n = len(ref), len(hyp)
    INF = 10 ** 9
    D = [[0] * (n + 1)] + [[INF] * (n + 1) for _ in range(m)]
    B = [[None] * (n + 1) for _ in range(m + 1)]
    for i in range(1, m + 1):
        wild = ref[i - 1] == WILD
        D[i][0], B[i][0] = D[i - 1][0] + (0 if wild else 1), "D"
        Di, Dp, Bi = D[i], D[i - 1], B[i]
        for j in range(1, n + 1):
            sub = Dp[j - 1] + (0 if wild or ref[i - 1] == hyp[j - 1] else 1)
            dele = Dp[j] + (0 if wild else 1)
            ins = Di[j - 1] + 1
            if sub <= dele and sub <= ins:
                Di[j], Bi[j] = sub, "S"
            elif dele <= ins:
                Di[j], Bi[j] = dele, "D"
            else:
                Di[j], Bi[j] = ins, "I"
    j = min(range(n + 1), key=lambda k: D[m][k])
    i, S, Dl, I, ops = m, 0, 0, 0, []
    while i > 0:
        op = B[i][j]
        if op == "S":
            if ref[i - 1] != hyp[j - 1] and ref[i - 1] != WILD:
                S += 1; ops.append((ref[i - 1], hyp[j - 1]))
            else:
                ops.append((ref[i - 1], None))
            i, j = i - 1, j - 1
        elif op == "D":
            if ref[i - 1] != WILD:
                Dl += 1
            ops.append((ref[i - 1], ""))
            i -= 1
        else:
            I += 1; ops.append(("", hyp[j - 1])); j -= 1
    return S, Dl, I, ops[::-1]


def main():
    if len(sys.argv) not in (2, 5) or (len(sys.argv) == 5 and sys.argv[2] != "--show"):
        raise SystemExit(__doc__)
    meeting = sys.argv[1]
    src = load_sources(meeting)
    frags = [(f["id"], f["start"], f["end"], f["channel"]) for f in load_config(meeting)["fragments"]
             if os.path.exists(os.path.join(meeting, "reference", f"ref_{f['id']:02d}.txt"))]
    refs = {fid: load_ref(meeting, fid) for fid, *_ in frags}

    if len(sys.argv) == 5:
        name, fid = sys.argv[3], int(sys.argv[4])
        _, a, b, ch = next(f for f in frags if f[0] == fid)
        for r, h in align(refs[fid], hyp_words(src[name], ch, a, b))[3]:
            print(r if h is None else f"{r or '·'}  →  {h or '∅'}" if (r != h) else r)
        return

    print(f"{'source':30s}" + "".join(f"{'#'+str(f[0]):>7s}" for f in frags)
          + f"{'WER':>7s}{'lenient':>9s}{'S':>6s}{'D':>6s}{'I':>6s}")
    rows = []
    for name, segs in src.items():
        tot, cells, tr = [0, 0, 0], [], 0
        for fid, a, b, ch in frags:
            S, D, I, ops = align(refs[fid], hyp_words(segs, ch, a, b))
            n = sum(w != WILD for w in refs[fid])
            cells.append((S + D + I) / n * 100)
            tot = [tot[0] + S, tot[1] + D, tot[2] + I]
            tr += translit_hits(ops)
        N = sum(sum(w != WILD for w in refs[f[0]]) for f in frags)
        rows.append((sum(tot) / N * 100, (sum(tot) - tr) / N * 100, name, cells, tot))
    for wer, lenient, name, cells, (S, D, I) in sorted(rows):
        print(f"{name:30s}" + "".join(f"{c:7.1f}" for c in cells) + f"{wer:7.1f}{lenient:9.1f}{S:6d}{D:6d}{I:6d}")
    print("\nlenient = WER where a Latin reference term written in Cyrillic (hotfix→хотфикс) counts as correct")
    print(f"\nreference words: " + ", ".join(f"#{f[0]}={sum(w != WILD for w in refs[f[0]])}" for f in frags))


if __name__ == "__main__":
    main()
