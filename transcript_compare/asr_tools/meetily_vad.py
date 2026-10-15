"""Line-by-line port of Meetily's ContinuousVadProcessor (frontend/src-tauri/src/audio/vad.rs) + merge_segments.

usage: meetily_vad.py <meeting_dir>

Writes <meeting_dir>/asr_runs/vad_meetily.json: {channel: {"segments": [[start_ms, end_ms, n_samples, sample_offset]], ...}}
The segment's audio = current_speech as Meetily builds it (history prepend + speech + trailing silence up to
redemption), so we store where in the channel those samples come from (sample_offset, n_samples).
"""
import json, os, sys
import numpy as np, soundfile, onnxruntime as ort

MODEL = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "frontend", "src-tauri", "models",
                     "silero_vad_v6.onnx")
DATA = os.path.join(sys.argv[1], "asr_runs")  # dir with mic.wav / sys.wav (16 kHz mono)
CFG = dict(threshold=0.50, neg_threshold=0.35, min_speech_ms=250, redemption_ms=200,
           pre_pad_ms=150, post_pad_ms=150, buffer_capacity=5120)  # live() == batch() except max_segment
SR, WIN, CTX = 16000, 512, 64


def run_vad(x):
    sess = ort.InferenceSession(MODEL, providers=["CPUExecutionProvider"])
    state = np.zeros((2, 1, 128), np.float32)
    ctx = np.zeros(CTX, np.float32)
    sr = np.array(SR, np.int64)
    red = int(SR * CFG["redemption_ms"] / 1000); mins = int(SR * CFG["min_speech_ms"] / 1000)
    pre = int(SR * CFG["pre_pad_ms"] / 1000); post = int(SR * CFG["post_pad_ms"] / 1000)
    processed = speech_start = silent = 0
    in_speech = redemption_passed = False
    cur_off = cur_len = 0          # current_speech as (offset into x, length) — contiguous by construction
    hist_len = 0                   # audio_history length (it always ends at `processed`)
    segs = []
    n = len(x) // WIN
    # flush() pads the tail chunk with zeros; approximate by padding x
    if len(x) % WIN:
        x = np.concatenate([x, np.zeros(WIN - len(x) % WIN, np.float32)]); n += 1
    for k in range(n):
        chunk = x[k * WIN:(k + 1) * WIN]
        inp = np.concatenate([ctx, chunk])[None, :]
        ctx = chunk[-CTX:].copy()
        out, state = sess.run(["output", "stateN"], {"input": inp, "state": state, "sr": sr})
        prob = float(out[0, 0])
        silent = silent + WIN if prob < CFG["neg_threshold"] else 0
        if not in_speech and prob >= CFG["threshold"]:
            in_speech, redemption_passed = True, False
            speech_start = max(0, processed - pre)
            prepend = min(hist_len, pre)
            cur_off, cur_len = processed - prepend, prepend + WIN
        elif in_speech:
            if not redemption_passed and processed + WIN - speech_start >= mins:
                redemption_passed = True
            if prob < CFG["neg_threshold"] and redemption_passed and silent >= red:
                end_sample = max(0, processed - silent) + post
                if cur_len:
                    segs.append([speech_start / 16.0, end_sample / 16.0, cur_len, cur_off])
                cur_len = 0; in_speech = redemption_passed = False
                processed += WIN
                continue                      # Meetily returns before updating audio_history
            cur_len += WIN
        hist_len = min(CFG["buffer_capacity"], hist_len + WIN)
        processed += WIN
    if in_speech and cur_len:
        segs.append([speech_start / 16.0, processed / 16.0, cur_len, cur_off])
    return segs


if __name__ == "__main__":
    out = {}
    for ch, f in (("Microphone", "mic.wav"), ("System", "sys.wav")):
        x = soundfile.read(os.path.join(DATA, f), dtype="float32")[0]
        out[ch] = run_vad(x)
        print(ch, len(out[ch]), "segments", flush=True)
    json.dump(out, open(os.path.join(DATA, "vad_meetily.json"), "w"))
