"""Segmentation experiment: Meetily's decode params + Meetily text cleaner, different ways of cutting audio.

usage: seg_exp.py <meeting_dir> <model_tag> <port> <mode> [<mode> ...]
modes: meetily-offline | meetily-live | pack-meetily | firered-offline | pack-firered | whispercpp-vad[-mc0]
Reads mic.wav / sys.wav / vad_*.json and writes asr_seg-<model_tag>-<mode>.json rows {start,end,text,source}
(on the real timeline) in <meeting_dir>/asr_runs/.
"""
import io, json, os, sys, time, wave, re
from collections import Counter
import numpy as np, soundfile, requests

DATA, TAG, PORT, MODES = os.path.join(sys.argv[1], "asr_runs"), sys.argv[2], sys.argv[3], sys.argv[4:]
SR = 16000
CH = {"Microphone": "mic.wav", "System": "sys.wav"}
BASE = {"beam_size": 2, "temperature": 0.2, "temperature_inc": 0.2, "entropy_thold": 2.4, "logprob_thold": -1.0,
        "max_context": -1, "prompt": "", "language": "ru", "suppress_non_speech": "true",
        "response_format": "verbose_json"}
SEP_S, PACK_MAX_S = 0.3, 25.0
audio = {ch: soundfile.read(os.path.join(DATA, f), dtype="float32")[0] for ch, f in CH.items()}


# ---------- Meetily text cleaner (whisper_engine.rs clean_repetitive_text) ----------
MEANINGLESS = ["thank you for watching", "thanks for watching", "like and subscribe", "music playing",
               "applause", "laughter", "um um um", "uh uh uh", "ah ah ah"]


NOCLEAN = os.environ.get("NOCLEAN") == "1"


def clean(text):
    if NOCLEAN:
        return text
    if not text:
        return ""
    tl = text.lower()
    if any(p in tl for p in MEANINGLESS) or (len(set(text)) <= 3 and len(text) > 10):
        return ""
    w = text.split()
    if len(w) < 3:
        return text
    a, i = [], 0
    while i < len(w):
        j = i
        while j + 1 < len(w) and w[j + 1] == w[i]:
            j += 1
        a.append(w[i]); i = j + 1
    if len(a) >= 4:
        b, i = [], 0
        while i < len(a):
            for L in range(2, min(5, (len(a) - i) // 2) + 1):
                if i + 2 * L <= len(a) and a[i:i + L] == a[i + L:i + 2 * L]:
                    b += a[i:i + L]; i += 2 * L; break
            else:
                b.append(a[i]); i += 1
        a = b
    t = " ".join(a)
    ww = t.split()
    if len(ww) >= 4:
        c = Counter(x.lower() for x in ww)
        if sum(v - 1 for v in c.values()) / len(ww) > 0.7:
            return ""
    return t


# ---------- Meetily segment ops (vad.rs merge_segments / split_at_silence_gaps) ----------
class Seg:
    def __init__(self, s_ms, e_ms, x):
        self.s, self.e, self.x = s_ms, e_ms, x


def split(seg, maxs):
    x = seg.x
    if len(x) <= maxs:
        return [seg]
    w = int(0.2 * SR)
    if len(x) < 3 * w:
        mid = len(x) // 2
    else:
        margin = len(x) // 10
        scan_end = max(0, len(x) - (w + margin))
        if scan_end <= margin:
            mid = len(x) // 2
        else:
            sq = x.astype(np.float64) ** 2
            cs = np.concatenate([[0], np.cumsum(sq)])
            step = w // 4
            pos = np.arange(margin, scan_end, step)
            en = (cs[pos + w] - cs[pos]) / w
            k = int(np.argmin(en))
            mid = int(pos[k] + (w // 2 if k > 0 else 0))
    l = Seg(seg.s, seg.s + mid / 16.0, x[:mid]); r = Seg(l.e, seg.e, x[mid:])
    return split(l, maxs) + split(r, maxs)


def merge(segs, gap_ms, maxs):
    if not segs:
        return []
    out, cur = [], Seg(segs[0].s, segs[0].e, segs[0].x)
    for n in segs[1:]:
        g = n.s - cur.e
        if g < gap_ms:
            cur = Seg(cur.s, n.e, np.concatenate([cur.x, np.zeros(max(0, int(g / 1000 * SR)), np.float32), n.x]))
        else:
            out.append(cur); cur = Seg(n.s, n.e, n.x)
    out.append(cur)
    return [p for s in out for p in split(s, maxs)]


def vad_segments(kind, ch):
    if kind == "meetily":
        v = json.load(open(os.path.join(DATA, "vad_meetily.json")))[ch]
        return [Seg(s, e, audio[ch][off:off + n]) for s, e, n, off in v]
    if kind == "fwsilero":   # faster-whisper Silero (min_silence 500 ms, pad 200 ms) used in the first runs
        v = json.load(open(os.path.join(DATA, "vad_chunks.json")))[ch]
        return [Seg(a / 16.0, b / 16.0, audio[ch][a:b]) for a, b in v]
    v = json.load(open(os.path.join(DATA, "vad_firered.json")))[ch]
    pad = 0.15
    return [Seg(max(0, s - pad * 1000), e + pad * 1000,
                audio[ch][int(max(0, s / 1000 - pad) * SR):int((e / 1000 + pad) * SR)]) for s, e in v]


def wav_bytes(x):
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype(np.int16).tobytes())
    return buf.getvalue()


def decode(x, **over):
    r = requests.post(f"http://127.0.0.1:{PORT}/inference", files={"file": ("c.wav", wav_bytes(x), "audio/wav")},
                      data={k: str(v) for k, v in {**BASE, **over}.items()}, timeout=3600)
    return r.json()


def one_row_per_window(windows, ch):
    rows = []
    for s in windows:
        t = clean(" ".join(decode(s.x).get("text", "").split()))
        if t:
            rows.append({"start": s.s / 1000, "end": s.e / 1000, "text": t, "source": ch})
    return rows


def packed(segs, ch, stats):
    """Pack consecutive VAD pieces (<=25 s, 0.3 s separators), map whisper segments back by midpoint."""
    pieces = [p for s in segs for p in split(s, int(PACK_MAX_S * SR))]
    sep = np.zeros(int(SEP_S * SR), np.float32)
    rows, i = [], 0
    while i < len(pieces):
        grp, tot = [], 0
        while i < len(pieces) and (not grp or tot + len(pieces[i].x) <= PACK_MAX_S * SR):
            grp.append(pieces[i]); tot += len(pieces[i].x); i += 1
        x, table, off = [], [], 0.0      # table: (packed_start_s, packed_end_s, piece)
        for p in grp:
            x += [p.x, sep]; d = len(p.x) / SR
            table.append((off, off + d, p)); off += d + SEP_S
        res = decode(np.concatenate(x))
        texts = {id(p): [] for p in grp}
        for sg in res.get("segments", []):
            a, b, t = float(sg["start"]), float(sg["end"]), sg["text"]   # raw: may start mid-word
            if not t.strip():
                continue
            m = (a + b) / 2
            best = min(table, key=lambda r: 0 if r[0] <= m <= r[1] else min(abs(m - r[0]), abs(m - r[1])))
            texts[id(best[2])].append(t)
            stats["segs"] += 1
            stats["cross"] += sum(1 for r in table if min(b, r[1]) - max(a, r[0]) > 0.3) > 1
        for _, _, p in table:
            t = clean(" ".join("".join(texts[id(p)]).split()))
            if t:
                rows.append({"start": p.s / 1000, "end": p.e / 1000, "text": t, "source": ch})
    return rows


def whispercpp_vad(ch, mc):
    res = decode(audio[ch], max_context=mc)
    rows = []
    raw = []
    for sg in res.get("segments", []):
        if raw and not sg["text"].startswith(" "):          # segment continues the previous word
            raw[-1]["text"] += sg["text"]; raw[-1]["end"] = float(sg["end"])
        else:
            raw.append({"start": float(sg["start"]), "end": float(sg["end"]), "text": sg["text"], "source": ch})
    for r in raw:
        r["text"] = clean(" ".join(r["text"].split()))
    return [r for r in raw if r["text"]]


for mode in MODES:
    t0, rows, stats = time.time(), [], {"segs": 0, "cross": 0}
    for ch in CH:
        if mode == "meetily-offline":
            rows += one_row_per_window(merge(vad_segments("meetily", ch), 2000, 25 * SR), ch)
        elif mode == "meetily-live":
            w = [s for s in merge(vad_segments("meetily", ch), 500, 25 * SR) if len(s.x) >= 1600]
            rows += one_row_per_window(w, ch)
        elif mode == "firered-offline":
            rows += one_row_per_window(merge(vad_segments("firered", ch), 2000, 25 * SR), ch)
        elif mode == "pack-meetily":
            rows += packed(vad_segments("meetily", ch), ch, stats)
        elif mode == "pack-fwsilero":
            rows += packed(vad_segments("fwsilero", ch), ch, stats)
        elif mode == "fwsilero-offline":
            rows += one_row_per_window(merge(vad_segments("fwsilero", ch), 2000, 25 * SR), ch)
        elif mode == "pack-firered":
            rows += packed(vad_segments("firered", ch), ch, stats)
        elif mode.startswith("whispercpp-vad"):
            rows += whispercpp_vad(ch, 0 if mode.endswith("-mc0") else -1)
    rows.sort(key=lambda r: r["start"])
    name = f"seg-{TAG}-{mode}" + ("-noclean" if NOCLEAN else "")
    json.dump({"model": name, "elapsed": time.time() - t0, "pack_stats": stats, "segments": rows},
              open(os.path.join(DATA, f"asr_{name}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    print(name, "rows", len(rows), "sec", round(time.time() - t0), "pack", stats, flush=True)
