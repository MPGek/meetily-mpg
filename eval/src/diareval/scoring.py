"""DER scoring with pyannote.metrics (Full setup: collar=0, overlap counted)."""

from __future__ import annotations

import json
from pathlib import Path

from pyannote.core import Annotation, Segment, Timeline
from pyannote.database.util import load_rttm
from pyannote.metrics.diarization import DiarizationErrorRate

from .manifests import load_manifest
from .paths import DATA_DIR, OUT_DIR


def _load_uem(path: Path) -> dict[str, Timeline]:
    uem: dict[str, Timeline] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        f = line.split()
        uri = f[0]
        start, end = float(f[2]), float(f[3])
        uem.setdefault(uri, Timeline(uri=uri)).add(Segment(start, end))
    return uem


def score_dataset(dataset: str, run_id: str = "latest", files: list[str] | None = None) -> dict:
    data = DATA_DIR / dataset
    ref_path = data / "rttm" / "ref.rttm"
    uem_path = data / "uem" / "ref.uem"
    hyp_dir = OUT_DIR / dataset / run_id
    if not ref_path.is_file() or not uem_path.is_file():
        raise SystemExit(f"dataset '{dataset}': missing {ref_path} or {uem_path}")
    if not hyp_dir.is_dir():
        raise SystemExit(f"no hypothesis run at {hyp_dir} — run `run --dataset {dataset}` first")

    refs = dict(load_rttm(str(ref_path)))
    if files is not None:
        wanted = set(files)
        missing_refs = wanted - set(refs)
        if missing_refs:
            raise SystemExit(f"{dataset}: references missing for {sorted(missing_refs)[:5]}")
        refs = {uri: ann for uri, ann in refs.items() if uri in wanted}
    uem = _load_uem(uem_path)

    hyps: dict[str, object] = {}
    missing: list[str] = []
    for uri in refs:
        hyp_path = hyp_dir / f"{uri}.rttm"
        if not hyp_path.is_file():
            missing.append(uri)
            continue
        if hyp_path.stat().st_size == 0:
            # silence-only recording: valid empty hypothesis
            hyps[uri] = Annotation(uri=uri)
            continue
        anns = load_rttm(str(hyp_path))
        if uri in anns:
            hyps[uri] = anns[uri]
        elif len(anns) == 1:
            hyps[uri] = next(iter(anns.values()))
        else:
            hyps[uri] = Annotation(uri=uri)
    if missing:
        raise SystemExit(
            f"{len(missing)} recording(s) lack hypotheses "
            f"(run the dataset first or check --run-id): {sorted(missing)[:10]}"
        )

    metric = DiarizationErrorRate(collar=0.0, skip_overlap=False)
    total_ref = 0.0
    comp = {"missed": 0.0, "false_alarm": 0.0, "confusion": 0.0}
    for uri, ref in refs.items():
        detail = metric(ref, hyps[uri], uem=uem.get(uri), detailed=True)
        ref_secs = detail["total"]
        total_ref += ref_secs
        comp["missed"] += detail["missed detection"]
        comp["false_alarm"] += detail["false alarm"]
        comp["confusion"] += detail["confusion"]

    if total_ref <= 0:
        raise SystemExit(f"{dataset}: no scored speech in UEM-annotated regions")
    der = (comp["missed"] + comp["false_alarm"] + comp["confusion"]) / total_ref

    try:
        baseline = load_manifest(dataset).baseline_der
    except Exception:
        baseline = None
    result = {
        "dataset": dataset,
        "run_id": run_id,
        "files": len(refs),
        "scored_hours": total_ref / 3600.0,
        "der": der * 100.0,
        "fa": comp["false_alarm"] / total_ref * 100.0,
        "miss": comp["missed"] / total_ref * 100.0,
        "conf": comp["confusion"] / total_ref * 100.0,
        "baseline_der": baseline,
    }
    out = hyp_dir / "score.json"
    out.write_text(json.dumps(result, indent=1), encoding="utf-8")
    print(
        f"{dataset}: DER {result['der']:.2f}% "
        f"(FA {result['fa']:.2f} / Miss {result['miss']:.2f} / Conf {result['conf']:.2f}) "
        f"over {result['files']} files / {result['scored_hours']:.2f} h"
        + (f" | baseline {baseline:.1f}%" if baseline is not None else "")
    )
    return result


def score(args) -> int:
    score_dataset(args.dataset, args.run_id)
    return 0
