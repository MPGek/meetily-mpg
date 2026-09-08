# Diarization AHC re-tune sweep — 2026-09-07 (pipeline-v2 task 6.2)

- Grid axes: clusterer={ahc} × cluster_threshold={0.52,0.60} × cluster_ceiling={128} × gap_merge_secs={0.0,0.3}
- Tuning dataset: voxconverse-dev (216 files / 19.65 h)
- Held-out validation: deferred to the `v2-ahc` full-set runs (voxconverse test, msdwild, ru-youtube, ru-synthetic)
- New-core binary with per-segment primary turns + single-cluster overlap coverage fix

## Tuning results

| # | thr | ceil | gap | files | DER % | FA | Miss | Conf |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 (`v2retune-000`) | 0.52 | 128 | 0.0 | 216 | 26.77 | 4.18 | 2.43 | 20.16 |
| 2 (`v2retune-001`) | 0.52 | 128 | 0.3 | 216 | 26.60 | 4.00 | 2.41 | 20.19 |
| 3 (`v2retune-002`) | 0.60 | 128 | 0.0 | 216 | 21.40 | 4.18 | 2.43 | 14.79 |
| 4 (`v2retune-003`) | 0.60 | 128 | 0.3 | 216 | 21.23 | 4.00 | 2.43 | 14.80 |

Reference points (same binary family): old-core AHC probe thr0.52/ceil64/gap0.0
Conf 18.53; v2 + AHC thr0.52/ceil64/gap0.0 (`v2fix-ahc`) Conf 20.14 —
ceiling 64 vs 128 is immaterial on dev.

## Selection

Candidate 4 (thr 0.60 / ceil 128 / gap 0.3, dev Conf 14.80, DER 21.23) —
identical to the already-shipped tuned numeric defaults. The 6.2 kind decision
(dev nmesc Conf 37.27 vs AHC 14.80) switches the built-in default kind to
`ahc` with unchanged numeric values. See `spike-2026-09-07-v2.md` for the
full evidence matrix.
