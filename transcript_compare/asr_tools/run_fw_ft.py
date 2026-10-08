"""faster-whisper over the same packed VAD windows as wcpp-*-merged (<=25 s speech, silence removed).

usage: run_fw_ft.py <meeting_dir> <tag> <model: faster-whisper alias or HF repo with ct2_int8_float16/>
Settings follow the coriollon model cards: int8_float16, beam 5, language ru.
"""
import json, os, sys, time, glob, site

for sp in site.getsitepackages():
    for d in glob.glob(os.path.join(sp, "nvidia", "*", "bin")):
        os.add_dll_directory(d)
        os.environ["PATH"] = d + os.pathsep + os.environ["PATH"]

import numpy as np, soundfile
from faster_whisper import WhisperModel

HERE = os.path.join(sys.argv[1], "asr_runs")   # mic.wav / sys.wav / vad_chunks.json in, asr_<tag>.json out
SR = 16000
CHANNELS = {"Microphone": "mic.wav", "System": "sys.wav"}
tag, model_ref = sys.argv[2], sys.argv[3]

if "/" in model_ref:
    from huggingface_hub import snapshot_download
    path = os.path.join(snapshot_download(model_ref, allow_patterns=["ct2_int8_float16/*"]), "ct2_int8_float16")
else:
    path = model_ref
model = WhisperModel(path, device="cuda", compute_type="int8_float16")

vad = json.load(open(os.path.join(HERE, "vad_chunks.json")))
t0, segs = time.time(), []
for ch, f in CHANNELS.items():
    audio = soundfile.read(os.path.join(HERE, f), dtype="float32")[0]
    groups, cur = [], []
    for a, b in vad[ch]:
        if cur and sum(y - x for x, y in cur) + (b - a) > 25 * SR:
            groups.append(cur); cur = []
        cur.append((a, b))
    if cur:
        groups.append(cur)
    gap = np.zeros(int(0.2 * SR), dtype=np.float32)
    for g in groups:
        x = np.concatenate([np.concatenate([audio[p:q], gap]) for p, q in g])
        out, _ = model.transcribe(x, language="ru", task="transcribe", beam_size=5,
                                  condition_on_previous_text=False, vad_filter=False)
        text = " ".join(" ".join(s.text for s in out).split())
        if text:
            segs.append({"start": g[0][0] / SR, "end": g[-1][1] / SR, "text": text, "source": ch})
segs.sort(key=lambda s: s["start"])
json.dump({"model": tag, "elapsed": time.time() - t0, "segments": segs},
          open(os.path.join(HERE, f"asr_{tag}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print(tag, "done", len(segs), "segments", round(time.time() - t0), "s")
