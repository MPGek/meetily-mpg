---
sub_files:
  - CODEBASE_MAP_ARCHITECTURE.md
  - CODEBASE_MAP_CONVENTIONS.md
  - CODEBASE_MAP_OPERATIONS.md
---

# Meetily Codebase Map

> Hand-maintained, deliberately thin. This map covers only what changes slowly
> (system overview, conventions, build/run commands). For file-, module- and
> symbol-level navigation use the graphify knowledge graph, which is regenerated
> from the source on every `graphify update .`.

Meetily is a **privacy-first AI meeting assistant** desktop application built with Tauri v2 (Rust + Next.js). It captures, transcribes, and summarizes meetings entirely on local infrastructure — no cloud dependency required.

### Core Capabilities
- **Audio Capture**: Microphone + system audio (stereo left=mic/right=sys) with per-channel Silero v6 VAD, RNNoise, HPF, adaptive resampling
- **Diarization**: Enhanced-only Polyvoice (segmentation-3.0 + TitaNet-Large 192-d + AHC MinClusterSize=2), ffmpeg streaming decode, fixed ONNX pool `min(8, 75% cores)`, offline + online (Efficient/Fast) with `PrototypeStore`; **word-level CTC alignment** (`word_alignment/`, wav2vec2 XLS-R 56 + constrained Viterbi) refines per-token timestamps live at block finalization + as offline/stop-time repair, feeding the token N-way split
- **Transcription**: Whisper.cpp (local) / Parakeet ONNX streaming / provider abstraction, provider-aware readiness gate, Enhance re-transcription, import pipeline
- **Speaker Registry**: Global `speakers` + `speaker_embeddings` (voiceprints with provenance, two-owner CHECK, 64-cap), `meeting_speakers` (centroids + exemplars ≤32) + `meeting_expected_speakers` allowlists, cosine matcher τ=0.7, live `assign_live_speaker`/`rematch_meeting_speakers`
- **Summarization**: Multi-provider AI (Ollama, Claude, Groq, OpenAI, OpenRouter + built-in llama-helper) with chunked processing, template system, English-cache, debug logging
- **Storage**: SQLite via sqlx (WAL) + local filesystem audio, meeting folders `YYYY-MM-DD_HH-MM`, checkpoint recovery
- **Playback**: FFmpeg WAV transcode to `44100Hz` temp cache, streaming `AudioPlayer`/`useAudioPlayer`, clip indicator per segment

---

## Map Sections

| Section | File | Description |
|---------|------|-------------|
| Architecture | [CODEBASE_MAP_ARCHITECTURE.md](CODEBASE_MAP_ARCHITECTURE.md) | System overview, architecture diagram, layer descriptions |
| Conventions | [CODEBASE_MAP_CONVENTIONS.md](CODEBASE_MAP_CONVENTIONS.md) | Patterns, naming standards, architectural principles |
| Operations | [CODEBASE_MAP_OPERATIONS.md](CODEBASE_MAP_OPERATIONS.md) | Build, test, run commands, gotchas |

## File- and Module-Level Navigation

Use graphify instead of a hand-written module listing:

- `graphify query "<question>"`, `graphify explain "<concept>"`, `graphify path "<A>" "<B>"` — scoped subgraphs of `graphify-out/graph.json`
- `graphify-out/wiki/index.md` — agent-crawlable wiki (one article per community); generated on demand with `graphify export wiki`
- `graphify-out/GRAPH_REPORT.md` — god nodes and community structure, for broad architecture review

`graphify-out/` is gitignored; regenerate it locally with `graphify update .` (graph) and then `graphify export wiki` (wiki).

## Changelog

For what has shipped, see `openspec/changes/archive/` (browse newest-first) and the current specs in `openspec/specs/`.
