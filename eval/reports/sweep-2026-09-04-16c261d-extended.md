# Diarization clustering-parameter sweep — 2026-09-05 (git 16c261d)

- Grid axes: cluster_threshold={0.55,0.6}, cluster_ceiling={64,128}, gap_merge_secs={0.0,0.3}
- Tuning datasets: ru-synthetic, ru-youtube, voxconverse-dev
- Held-out validation datasets: voxconverse, msdwild
- Candidates: 8; validation evaluated for top-2 by mean tuning DER

## Tuning results

| # | cluster_threshold | cluster_ceiling | gap_merge_secs | ru-synthetic files | ru-synthetic DER % | ru-synthetic FA | Miss | Conf | ru-youtube files | ru-youtube DER % | ru-youtube FA | Miss | Conf | voxconverse-dev files | voxconverse-dev DER % | voxconverse-dev FA | Miss | Conf | mean DER |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.55 | 64 | 0.0 | 2000 | 53.71 | 2.57 | 43.10 | 8.05 | 6 | 65.45 | 25.40 | 4.65 | 35.41 | 215 | 28.94 | 2.47 | 2.67 | 23.80 | 49.37 |
| 2 | 0.55 | 64 | 0.3 | 2000 | 53.56 | 2.57 | 42.93 | 8.07 | 6 | 65.42 | 25.63 | 4.23 | 35.56 | 215 | 28.90 | 2.52 | 2.55 | 23.83 | 49.29 |
| 3 | 0.55 | 128 | 0.0 | 2000 | 53.71 | 2.57 | 43.10 | 8.05 | 6 | 65.64 | 25.40 | 4.65 | 35.59 | 215 | 27.02 | 2.47 | 2.67 | 21.88 | 48.79 |
| 4 | 0.55 | 128 | 0.3 | 2000 | 53.56 | 2.57 | 42.93 | 8.07 | 6 | 65.60 | 25.62 | 4.23 | 35.75 | 215 | 26.97 | 2.51 | 2.56 | 21.90 | 48.71 |
| 5 | 0.6 | 64 | 0.0 | 2000 | 54.23 | 2.57 | 43.10 | 8.57 | 6 | 65.46 | 25.40 | 4.65 | 35.41 | 215 | 26.36 | 2.47 | 2.67 | 21.22 | 48.68 |
| 6 | 0.6 | 64 | 0.3 | 2000 | 54.09 | 2.57 | 42.93 | 8.59 | 6 | 65.42 | 25.63 | 4.23 | 35.57 | 215 | 26.31 | 2.52 | 2.56 | 21.24 | 48.61 |
| 7 | 0.6 | 128 | 0.0 | 2000 | 54.23 | 2.57 | 43.10 | 8.57 | 6 | 65.66 | 25.40 | 4.65 | 35.62 | 215 | 23.66 | 2.47 | 2.67 | 18.52 | 47.85 |
| 8 | 0.6 | 128 | 0.3 | 2000 | 54.09 | 2.57 | 42.93 | 8.59 | 6 | 65.63 | 25.62 | 4.23 | 35.77 | 215 | 23.60 | 2.51 | 2.56 | 18.53 | 47.77 |

## Held-out validation (top candidates only)

| # | cluster_threshold | cluster_ceiling | gap_merge_secs | voxconverse DER % | voxconverse FA | Miss | Conf | msdwild DER % | msdwild FA | Miss | Conf | mean held-out DER |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 0.6 | 128 | 0.3 | 29.61 | 4.62 | 3.53 | 21.45 | 42.08 | 5.95 | 7.28 | 28.84 | 35.84 |
| 7 | 0.6 | 128 | 0.0 | 29.70 | 4.58 | 3.70 | 21.42 | 42.01 | 5.77 | 7.44 | 28.80 | 35.85 |
