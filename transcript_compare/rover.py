"""ROVER: word-level majority vote over several transcripts, scored like score.py.

usage: python rover.py <meeting_dir> <source> <source> <source> [...]
The first source is the pivot and breaks ties; sources are names from score.py's table.
Combination happens per reference fragment on the normalized words, so this measures what
voting could gain, not a transcript the app could ship as is.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from prepare import load_config  # noqa: E402
from score import WILD, align, hyp_words, load_ref, load_sources  # noqa: E402


def add(network, hyp, k):
    """Align hyp to the confusion network (list of {system: word}) and add it as system k."""
    m, n = len(network), len(hyp)
    D = [[j for j in range(n + 1)]] + [[i] + [0] * n for i in range(1, m + 1)]
    for i in range(1, m + 1):
        words = set(network[i - 1].values())
        for j in range(1, n + 1):
            D[i][j] = min(D[i - 1][j - 1] + (hyp[j - 1] not in words), D[i - 1][j] + 1, D[i][j - 1] + 1)
    out, i, j = [], m, n
    while i or j:
        if i and j and D[i][j] == D[i - 1][j - 1] + (hyp[j - 1] not in set(network[i - 1].values())):
            out.append({**network[i - 1], k: hyp[j - 1]}); i -= 1; j -= 1
        elif i and D[i][j] == D[i - 1][j] + 1:
            out.append({**network[i - 1], k: ""}); i -= 1
        else:
            out.append({**{s: "" for s in range(k)}, k: hyp[j - 1]}); j -= 1
    return out[::-1]


def vote(network, nsys):
    words = []
    for slot in network:
        counts = {}
        for s in range(nsys):
            w = slot.get(s, "")
            counts[w] = counts.get(w, 0) + 1
        best = max(counts.values())
        w = next(slot.get(s, "") for s in range(nsys) if counts[slot.get(s, "")] == best)
        if w:
            words.append(w)
    return words


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    meeting, names = sys.argv[1], sys.argv[2:]
    src = load_sources(meeting)
    frags = [(f["id"], f["start"], f["end"], f["channel"]) for f in load_config(meeting)["fragments"]
             if os.path.exists(os.path.join(meeting, "reference", f"ref_{f['id']:02d}.txt"))]
    tot, N = [0, 0, 0], 0
    for fid, a, b, ch in frags:
        ref = load_ref(meeting, fid)
        network = []
        for k, name in enumerate(names):
            network = add(network, hyp_words(src[name], ch, a, b), k)
        S, D, I, _ = align(ref, vote(network, len(names)))
        tot = [tot[0] + S, tot[1] + D, tot[2] + I]
        N += sum(w != WILD for w in ref)
    print(f"rover({'+'.join(names)}): WER {sum(tot) / N * 100:.1f}  S {tot[0]} D {tot[1]} I {tot[2]}  N {N}")


if __name__ == "__main__":
    main()
