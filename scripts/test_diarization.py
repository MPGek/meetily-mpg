"""
Test script for speaker diarization using sherpa-onnx Pyannote models.

Requirements (auto-installed via uv):
    sherpa-onnx, soundfile, numpy, scipy

Usage:
    uv run scripts/test_diarization.py <audio_file>

Example:
    uv run scripts/test_diarization.py "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-08-11_14-10-33_2026-08-11_11-10/audio.mp4"
"""

import sys
import os
import time
import subprocess
import tempfile
import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

DIARIZATION_SAMPLE_RATE = 16000

# Models directory — uses the same path the app uses
APP_DATA = os.path.join(os.environ.get("APPDATA", ""), "com.meetily.ai", "models")
SEG_MODEL = os.path.join(APP_DATA, "sherpa-onnx-pyannote-segmentation-3-0", "model.int8.onnx")
EMB_MODEL = os.path.join(APP_DATA, "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx")


def load_audio_to_16k(path: str) -> np.ndarray:
    """Load an audio file and decode to 16kHz mono float32 via ffmpeg."""
    print(f"Loading audio: {path}")

    with tempfile.TemporaryDirectory() as tmpdir:
        wav_path = os.path.join(tmpdir, "temp.wav")
        subprocess.run(
            ["ffmpeg", "-y", "-v", "error", "-i", path,
             "-ac", "1", "-ar", str(DIARIZATION_SAMPLE_RATE),
             "-sample_fmt", "s16",
             "-f", "wav", wav_path],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        samples, sr = sf.read(wav_path, dtype=np.float32)

    print(f"  Loaded: {len(samples)} samples, sr={sr}Hz, dtype={samples.dtype}")
    return samples


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        print("\nERROR: No audio file provided.")
        sys.exit(1)

    audio_path = sys.argv[1]

    if not os.path.exists(audio_path):
        print(f"ERROR: Audio file not found: {audio_path}")
        sys.exit(1)

    if not os.path.exists(SEG_MODEL):
        print(f"ERROR: Segmentation model not found: {SEG_MODEL}")
        print("Download it in the app's Settings → Diarization tab.")
        sys.exit(1)

    if not os.path.exists(EMB_MODEL):
        print(f"ERROR: Embedding model not found: {EMB_MODEL}")
        print("Download it in the app's Settings → Diarization tab.")
        sys.exit(1)

    print("=" * 60)
    print("  Speaker Diarization Test")
    print("=" * 60)
    print(f"  Audio:         {audio_path}")
    print(f"  Segmentation:  {SEG_MODEL}")
    print(f"  Embedding:     {EMB_MODEL}")
    print()

    # Load and prepare audio (ffmpeg handles resampling to 16kHz directly)
    t0 = time.time()
    samples = load_audio_to_16k(audio_path)
    load_time = time.time() - t0
    print(f"  Audio ready: {len(samples)} samples ({len(samples)/DIARIZATION_SAMPLE_RATE:.1f}s) in {load_time:.1f}s")
    print()

    # Import sherpa-onnx AFTER we've validated everything
    import sherpa_onnx

    # Configure diarization — matches Rust config exactly
    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(
                model=SEG_MODEL,
            ),
            num_threads=2,
            debug=False,
            provider="cpu",
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(
            model=EMB_MODEL,
            num_threads=2,
            debug=False,
            provider="cpu",
        ),
        clustering=sherpa_onnx.FastClusteringConfig(
            num_clusters=-1,
            threshold=0.5,
        ),
        min_duration_on=0.3,
        min_duration_off=0.5,
    )

    print("Creating diarizer...")
    diarizer = sherpa_onnx.OfflineSpeakerDiarization(config)

    print("Running diarization (this may take a while)...")
    t0 = time.time()
    try:
        result = diarizer.process(samples)
    except Exception as e:
        print(f"\nERROR during diarization: {e}")
        print("\nCommon causes:")
        print("  1. Audio sample rate mismatch (must be 16000 Hz, mono)")
        print("  2. Audio is too quiet or silent")
        print("  3. Model files are corrupted or incompatible")
        sys.exit(1)

    elapsed = time.time() - t0
    segments = result.sort_by_start_time()

    print(f"  Done in {elapsed:.1f}s")
    print(f"  Segments found: {len(segments)}")

    speakers = sorted(set(s.speaker for s in segments))
    print(f"  Unique speakers: {len(speakers)}")
    print()

    if not segments:
        print("  ❌ NO SPEAKERS FOUND — check model files and audio.")
        sys.exit(1)

    print("  > SUCCESS - Found speakers!")
    print(f"  Speaker IDs: {speakers}")
    print()

    # Print first 20 segments
    print(f"  {'Start':>8s}  {'End':>8s}  {'Dur':>7s}  Speaker")
    print(f"  {'-'*8}  {'-'*8}  {'-'*7}  {'-'*12}")
    for s in segments[:20]:
        dur = s.end - s.start
        print(f"  {s.start:8.2f}  {s.end:8.2f}  {dur:7.2f}  SPEAKER_{s.speaker:02d}")

    if len(segments) > 20:
        print(f"  ... and {len(segments) - 20} more segments")

    print()
    print("  Per-speaker summary:")
    for spk in speakers:
        segs = [s for s in segments if s.speaker == spk]
        total_dur = sum(s.end - s.start for s in segs)
        print(f"    SPEAKER_{spk:02d}: {len(segs)} segments, {total_dur:.1f}s total")


if __name__ == "__main__":
    main()
