"""Qwen3-ASR over the same packed VAD windows as wcpp-*-merged (<=25 s speech, silence removed).

usage: run_qwen.py <meeting_dir> <model_id> <tag> [glossary]
"glossary" prompts with <meeting_dir>/glossary.json (prompt_terms + prompt_names).
"""
import json, os, sys, time
import numpy as np, soundfile, torch
from transformers import AutoModelForMultimodalLM, AutoProcessor
from glossary import prompt_glossary

HERE = os.path.join(sys.argv[1], "asr_runs")   # mic.wav / sys.wav / vad_chunks.json in, asr_*.json out
SR = 16000
CHANNELS = {"Microphone": "mic.wav", "System": "sys.wav"}
model_id, tag = sys.argv[2], sys.argv[3]
use_glossary = len(sys.argv) > 4 and sys.argv[4] == "glossary"
# Same glossary as wcpp "v4domah": terms/names a user would plausibly configure for this team.
GLOSSARY = "Vocabulary: " + prompt_glossary(sys.argv[1], "the glossary prompt") if use_glossary else None
processor = AutoProcessor.from_pretrained(model_id)
model = AutoModelForMultimodalLM.from_pretrained(model_id, device_map="cuda", dtype=torch.bfloat16)

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
        kw = {"audio": x, "language": "Russian"}
        if use_glossary:
            kw["prompt"] = GLOSSARY
        inputs = processor.apply_transcription_request(**kw).to(model.device, model.dtype)
        with torch.inference_mode():
            out = model.generate(**inputs, max_new_tokens=512)
        text = processor.decode(out[:, inputs["input_ids"].shape[1]:], return_format="transcription_only")[0]
        text = " ".join(text.split())
        if text:
            segs.append({"start": g[0][0] / SR, "end": g[-1][1] / SR, "text": text, "source": ch})
segs.sort(key=lambda s: s["start"])
name = f"{tag}{'-glossary' if use_glossary else ''}"
json.dump({"model": name, "elapsed": time.time() - t0, "segments": segs},
          open(os.path.join(HERE, f"asr_{name}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print(name, "done", len(segs), "segments", round(time.time() - t0), "s")
