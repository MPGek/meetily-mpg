"""Nemotron 3.5 ASR over the same packed VAD windows as wcpp-*-merged (<=25 s speech, silence removed).

usage: run_nemotron.py <meeting_dir> <lookahead_tokens: 13=1120ms | 6=560ms> [window_s]
"""
import json, os, sys, time
import numpy as np, soundfile, torch
from transformers import AutoModelForRNNT, AutoProcessor

HERE = os.path.join(sys.argv[1], "asr_runs")   # mic.wav / sys.wav / vad_chunks.json in, asr_*.json out
SR = 16000
CHANNELS = {"Microphone": "mic.wav", "System": "sys.wav"}
lookahead = int(sys.argv[2]) if len(sys.argv) > 2 else 13
window_s = float(sys.argv[3]) if len(sys.argv) > 3 else 25.0

model_id = "nvidia/nemotron-3.5-asr-streaming-0.6b"
processor = AutoProcessor.from_pretrained(model_id)
processor.set_num_lookahead_tokens(lookahead)
model = AutoModelForRNNT.from_pretrained(model_id, device_map="cuda", torch_dtype=torch.bfloat16)
print("latency ms:", processor.streaming_latency_ms, flush=True)

vad = json.load(open(os.path.join(HERE, "vad_chunks.json")))
t0, segs = time.time(), []
for ch, f in CHANNELS.items():
    audio = soundfile.read(os.path.join(HERE, f), dtype="float32")[0]
    groups, cur = [], []
    for a, b in vad[ch]:
        if cur and sum(y - x for x, y in cur) + (b - a) > window_s * SR:
            groups.append(cur); cur = []
        cur.append((a, b))
    if cur:
        groups.append(cur)
    gap = np.zeros(int(0.2 * SR), dtype=np.float32)
    for g in groups:
        x = np.concatenate([np.concatenate([audio[p:q], gap]) for p, q in g])
        inputs = processor(x, sampling_rate=SR, language="ru-RU")
        inputs.to(model.device, dtype=model.dtype)
        with torch.inference_mode():
            out = model.generate(**inputs, return_dict_in_generate=True)
        text = processor.decode(out.sequences, skip_special_tokens=True)
        text = " ".join((text[0] if isinstance(text, list) else text).split())
        if text:
            segs.append({"start": g[0][0] / SR, "end": g[-1][1] / SR, "text": text, "source": ch})
segs.sort(key=lambda s: s["start"])
name = f"nemotron35-la{lookahead}-w{int(window_s)}"
json.dump({"model": name, "elapsed": time.time() - t0, "segments": segs},
          open(os.path.join(HERE, f"asr_{name}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print(name, "done", len(segs), "segments", round(time.time() - t0), "s")
