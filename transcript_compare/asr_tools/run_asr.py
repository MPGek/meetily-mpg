"""Run one ASR model over mic.wav and sys.wav, write asr_<model>.json with timed segments.

usage: run_asr.py <meeting_dir> <model>   (fw-<faster-whisper model> or an onnx-asr model name)
Reads and writes <meeting_dir>/asr_runs/.
"""
import json, os, sys, time, glob, site

# Put pip-installed CUDA DLLs (cublas, cudnn) on the DLL search path.
for sp in site.getsitepackages():
    for d in glob.glob(os.path.join(sp, "nvidia", "*", "bin")):
        os.add_dll_directory(d)
        os.environ["PATH"] = d + os.pathsep + os.environ["PATH"]

HERE = os.path.join(sys.argv[1], "asr_runs")   # mic.wav / sys.wav in, asr_<model>.json out
name = sys.argv[2]
CHANNELS = {"Microphone": "mic.wav", "System": "sys.wav"}


def run_faster_whisper(model_id):
    from faster_whisper import WhisperModel
    m = WhisperModel(model_id, device="cuda", compute_type="float16")
    out = []
    for ch, f in CHANNELS.items():
        import soundfile
        audio, _ = soundfile.read(os.path.join(HERE, f), dtype="float32")
        segs, _ = m.transcribe(audio, language="ru", beam_size=5,
                               vad_filter=True, condition_on_previous_text=False)
        for s in segs:
            out.append({"start": s.start, "end": s.end, "text": s.text.strip(), "source": ch})
    return out


def run_onnx_asr(model_id):
    import onnx_asr
    m = onnx_asr.load_model(model_id, providers=["CUDAExecutionProvider", "CPUExecutionProvider"])
    vad = onnx_asr.load_vad("silero", providers=["CPUExecutionProvider"])
    out = []
    for ch, f in CHANNELS.items():
        for s in m.with_vad(vad, batch_size=8).recognize(os.path.join(HERE, f)):
            out.append({"start": float(s.start), "end": float(s.end), "text": s.text.strip(), "source": ch})
    return out


t0 = time.time()
if name.startswith("fw-"):
    segs = run_faster_whisper(name[3:])
else:
    segs = run_onnx_asr(name)
segs = [s for s in segs if s["text"]]
segs.sort(key=lambda s: s["start"])
json.dump({"model": name, "elapsed": time.time() - t0, "segments": segs},
          open(os.path.join(HERE, f"asr_{name.replace('/', '_')}.json"), "w", encoding="utf-8"),
          ensure_ascii=False, indent=1)
print(name, "done", len(segs), "segments", round(time.time() - t0), "s")
