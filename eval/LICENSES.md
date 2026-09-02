# Dataset Licenses

All datasets in this harness are used **for evaluation only**: no redistribution, no
model training or fine-tuning on this data (see the table below; several licenses
forbid training outright), and results are reported only as aggregate metrics.

| Dataset | Source | License | Notes |
| --- | --- | --- | --- |
| VoxConverse (test) | Oxford VGG (voxconverse) | Research use, non-commercial | YouTube-sourced; original YouTube ToS apply to raw audio. |
| AMI (SDM + headset mix) | AMI corpus via EDAP/LDC (manual, gated) | CC BY 4.0 after signed EULA | Meetings audio + BUTSpeechFIT pyannote-fork reference RTTM/UEM lists. |
| MSDWild | Oxford VGG (manual/registration) | Research use, non-commercial | Music-show "in the wild" audio, YouTube-sourced. |
| DIHARD-3 | LDC (LDC2022S03, manual, gated) | LDC User Agreement | Evaluation-only clause; no redistribution. |
| niobures/synthetic-speech-diarization-ru | Hugging Face | MIT | Synthetic Russian TTS speech, 2000 tracks, 16 kHz. |
| leshinsky/ru-youtube-diarization | Hugging Face | Apache-2.0 | Real Russian YouTube clips with Label Studio annotations. |

## Eval-only usage statement

- Normalized data lives in the local DVC remote and is never committed to git or
  redistributed beyond machines of this project's authors.
- Gated corpora (AMI, DIHARD-3) must be acquired by each user under their own
  license; only the `.dvc` pointers are shared in the repo.
- Reports contain aggregate DER metrics only, never transcripts or audio.
