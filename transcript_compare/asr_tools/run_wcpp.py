"""Run whisper.cpp (whisper-server) per VAD chunk, Meetily-style, with prompt / anti-hallucination variants.

usage: run_wcpp.py <meeting_dir> <model_tag> <port> <config> [<config> ...]
Config "v4domah" prompts with <meeting_dir>/glossary.json (prompt_terms + prompt_names).
"""
import io, json, os, sys, time, wave
import numpy as np, soundfile, requests
from glossary import prompt_glossary

HERE = os.path.join(sys.argv[1], "asr_runs")   # mic.wav / sys.wav / vad_chunks.json in, asr_*.json out
CHANNELS = {"Microphone": "mic.wav", "System": "sys.wav"}
SR = 16000

PROMPT_V4 = ("This is a recording where a bilingual speaker uses Russian and English. English words and terms "
             "are preserved in Latin script. Example: «Мы задеплоили feature в production через CI/CD pipeline, "
             "используя для этого Claude Code».")

# Meetily Windows defaults (hardware_detector.rs): beam 2, temp 0.2, entropy 2.4, logprob -1, no_speech 0.55
BASE = {"beam_size": 2, "temperature": 0.2, "temperature_inc": 0.2, "entropy_thold": 2.4,
        "logprob_thold": -1.0, "max_context": -1, "prompt": "", "language": "ru",
        "suppress_non_speech": "true", "response_format": "json"}
CONFIGS = {
    "base": {},
    "v4": {"prompt": PROMPT_V4},
    "v4ah": {"prompt": PROMPT_V4, "entropy_thold": 2.8, "max_context": 128},
    "v4domah": {"entropy_thold": 2.8, "max_context": 128},   # + prompt: v4 + meeting glossary, set below
    "ah": {"entropy_thold": 2.8, "max_context": 128},
    "t0": {"temperature": 0.0},
    "t0b5": {"temperature": 0.0, "beam_size": 5},
    # 2026-10-08 ideas: habr 1066528 found language=auto better on code-switching; a filler-laden
    # Russian prompt should keep а/ну/вот (whisper's main error here is deletions); wider beam;
    # no_speech_thold 1.0 never drops a window as silence.
    "auto": {"language": "auto"},
    "fill": {"prompt": "Ну, вот, а там, значит, мы это... Не, ну да. Короче, смотри, тут такое дело."},
    "b5": {"beam_size": 5},
    "nth1": {"no_speech_thold": 1.0},
}


def vad_chunks():
    cache = os.path.join(HERE, "vad_chunks.json")
    if os.path.exists(cache):
        return json.load(open(cache))
    from faster_whisper.vad import get_speech_timestamps, VadOptions
    out = {}
    for ch, f in CHANNELS.items():
        audio, _ = soundfile.read(os.path.join(HERE, f), dtype="float32")
        ts = get_speech_timestamps(audio, VadOptions(min_silence_duration_ms=500, max_speech_duration_s=28,
                                                     speech_pad_ms=200))
        out[ch] = [(t["start"], t["end"]) for t in ts]
    json.dump(out, open(cache, "w"))
    return out


def wav_bytes(x):
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype(np.int16).tobytes())
    return buf.getvalue()


def packed(chunks, max_s=25.0):
    """Pack consecutive VAD chunks (silence removed) into windows up to max_s, like faster-whisper."""
    out = {}
    for ch, cs in chunks.items():
        groups, cur = [], []
        for a, b in cs:
            if cur and sum(y - x for x, y in cur) + (b - a) > max_s * SR:
                groups.append(cur); cur = []
            cur.append((a, b))
        if cur:
            groups.append(cur)
        out[ch] = groups
    return out


model_tag, port, cfgs = sys.argv[2], sys.argv[3], sys.argv[4:]
if "v4domah" in cfgs:   # v4 + meeting glossary (names/terms seen in this team's meetings)
    CONFIGS["v4domah"]["prompt"] = PROMPT_V4 + " Terms: " + prompt_glossary(sys.argv[1], "config v4domah")
MERGE = os.environ.get("MERGE") == "1"
chunks = vad_chunks()
if MERGE:
    model_tag += "-merged"
    chunks = packed(chunks)
else:
    chunks = {ch: [[(a, b)] for a, b in cs] for ch, cs in chunks.items()}
audio = {ch: soundfile.read(os.path.join(HERE, f), dtype="float32")[0] for ch, f in CHANNELS.items()}
for cfg in cfgs:
    form = {**BASE, **CONFIGS[cfg]}
    t0, segs = time.time(), []
    for ch, cs in chunks.items():
        for g in cs:
            a, b = g[0][0], g[-1][1]
            gap = np.zeros(int(0.2 * SR), dtype=np.float32)
            x = np.concatenate([np.concatenate([audio[ch][p:q], gap]) for p, q in g])
            r = requests.post(f"http://127.0.0.1:{port}/inference",
                              files={"file": ("c.wav", wav_bytes(x), "audio/wav")},
                              data={k: str(v) for k, v in form.items()}, timeout=300)
            text = r.json().get("text", "").strip()
            if text:
                segs.append({"start": a / SR, "end": b / SR, "text": " ".join(text.split()), "source": ch})
    segs.sort(key=lambda s: s["start"])
    name = f"wcpp-{model_tag}-{cfg}"
    json.dump({"model": name, "elapsed": time.time() - t0, "segments": segs},
              open(os.path.join(HERE, f"asr_{name}.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    print(name, "done", len(segs), "segments", round(time.time() - t0), "s", flush=True)
