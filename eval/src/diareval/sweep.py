"""Parameter sweep driver (diarization-param-tuning D5).

Runs the harness over a cross-product grid of clustering parameters on
designated tuning datasets, scores every candidate under an isolated run-id,
optionally evaluates the top-2 candidates on held-out validation datasets, and
writes a Markdown sweep report with tuning vs validation columns kept separate.
"""

from __future__ import annotations

import itertools
import json
import re
import subprocess
from datetime import date
from pathlib import Path

from .manifests import list_manifests, load_manifest
from .paths import OUT_DIR, REPORTS_DIR
from .runner import run_dataset
from .scoring import score_dataset

# config field -> (harness CLI flag, value formatter)
PARAM_FLAGS: dict[str, tuple[str, type]] = {
    "cluster_threshold": ("--cluster-threshold", float),
    "cluster_ceiling": ("--max-clusters", int),
    "gap_merge_secs": ("--gap-merge", float),
}

# Unique short prefixes for run-ids (first-word slugs of cluster_* collide).
PARAM_SLUGS = {"cluster_threshold": "thr", "cluster_ceiling": "ceil", "gap_merge_secs": "gap"}

VALIDATION_DATASETS = ("voxconverse", "msdwild")


def parse_grid(specs: list[str]) -> dict[str, list]:
    """Parse `--grid param=v1,v2,...` axes into {param: [values]}."""
    axes: dict[str, list] = {}
    for spec in specs:
        if "=" not in spec:
            raise SystemExit(f"sweep: bad --grid '{spec}' (expected param=v1,v2,...)")
        param, _, raw = spec.partition("=")
        if param not in PARAM_FLAGS:
            raise SystemExit(
                f"sweep: unknown grid param '{param}' (known: {sorted(PARAM_FLAGS)})"
            )
        caster = PARAM_FLAGS[param][1]
        try:
            values = [caster(v) for v in raw.split(",") if v.strip()]
        except ValueError as exc:
            raise SystemExit(f"sweep: bad value in --grid '{spec}': {exc}")
        if not values:
            raise SystemExit(f"sweep: --grid '{spec}' has no values")
        axes[param] = values
    return axes


def candidates(axes: dict[str, list]) -> list[dict]:
    keys = list(axes)
    return [dict(zip(keys, combo)) for combo in itertools.product(*(axes[k] for k in keys))]


def harness_args(candidate: dict) -> list[str]:
    return [f"{PARAM_FLAGS[k][0]}={v}" for k, v in candidate.items()]


def run_id_for(prefix: str, index: int, candidate: dict) -> str:
    slug = "-".join(
        f"{PARAM_SLUGS[k]}{str(v).replace('.', 'p')}" for k, v in candidate.items()
    )
    slug = re.sub(r"[^A-Za-z0-9._-]", "", slug)
    return f"{prefix}-{index:03d}-{slug}"


def _git_rev() -> str:
    try:
        return (
            subprocess.run(
                ["git", "rev-parse", "--short", "HEAD"],
                capture_output=True, text=True, check=True,
                cwd=REPORTS_DIR.parent,
            )
            .stdout.strip()
            or "unknown"
        )
    except Exception:
        return "unknown"


def _score_file(dataset: str, run_id: str) -> dict | None:
    p = OUT_DIR / dataset / run_id / "score.json"
    return json.loads(p.read_text(encoding="utf-8")) if p.is_file() else None


def _failed_uris(dataset: str, run_id: str) -> set[str]:
    p = OUT_DIR / dataset / run_id / "failed.txt"
    if not p.is_file():
        return set()
    return {Path(ln).stem for ln in p.read_text(encoding="utf-8").splitlines() if ln}


def tuning_datasets() -> list[str]:
    """Manifests marked `tuning: true` (voxconverse-dev, ru-youtube, ...)."""
    return [m.name for m in list_manifests() if m.tuning]


def run_sweep(
    datasets: list[str] | None,
    grid_specs: list[str],
    run_prefix: str = "sweep",
    workers: int | None = None,
    files: dict[str, list[str]] | None = None,
    validate_top: int = 2,
    validation_datasets: list[str] | None = None,
    report_path: Path | None = None,
) -> Path:
    axes = parse_grid(grid_specs)
    cands = candidates(axes)
    tuning = datasets if datasets is not None else tuning_datasets()
    if not tuning:
        raise SystemExit("sweep: no tuning datasets (mark a manifest with `tuning: true`)")
    for ds in tuning:
        load_manifest(ds)
    validation = (
        validation_datasets
        if validation_datasets is not None
        else [d for d in VALIDATION_DATASETS if (OUT_DIR.parent / "manifests" / f"{d}.yml").is_file()]
    )

    print(f"sweep: {len(cands)} candidates x {len(tuning)} tuning dataset(s)")
    tuning_scores: list[dict[str, dict]] = []
    for idx, cand in enumerate(cands):
        rid = run_id_for(run_prefix, idx, cand)
        args = harness_args(cand)
        row: dict[str, dict] = {"params": cand, "run_id": rid}
        for ds in tuning:
            ds_files = (files or {}).get(ds)
            run_dataset(
                ds, run_id=rid, workers=workers, files=ds_files,
                harness_args=args, skip_failures=True,
            )
            row[ds] = score_dataset(
                ds, run_id=rid, files=ds_files, exclude=_failed_uris(ds, rid)
            )
        tuning_scores.append(row)
        print(f"  candidate {idx + 1}/{len(cands)} ({rid}) done")

    # Selection rule (D5): top-N by mean tuning DER get held-out evaluation.
    def mean_der(row: dict) -> float:
        return sum(row[ds]["der"] for ds in tuning) / len(tuning)

    ranked = sorted(range(len(cands)), key=lambda i: mean_der(tuning_scores[i]))
    top = ranked[: max(0, validate_top)]
    val_scores: dict[int, dict[str, dict]] = {}
    for i in top:
        rid = tuning_scores[i]["run_id"]
        args = harness_args(cands[i])
        vrow: dict[str, dict] = {}
        for ds in validation:
            run_dataset(ds, run_id=rid, workers=workers, harness_args=args, skip_failures=True)
            vrow[ds] = score_dataset(ds, run_id=rid, exclude=_failed_uris(ds, rid))
        val_scores[i] = vrow

    rev = _git_rev()
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    out = report_path or REPORTS_DIR / f"sweep-{date.today().isoformat()}-{rev}.md"
    lines = [
        f"# Diarization clustering-parameter sweep — {date.today().isoformat()} (git {rev})",
        "",
        f"- Grid axes: " + ", ".join(f"{k}={{{','.join(map(str, v))}}}" for k, v in axes.items()),
        f"- Tuning datasets: {', '.join(tuning)}",
        f"- Held-out validation datasets: {', '.join(validation) or '(none)'}",
        f"- Candidates: {len(cands)}; validation evaluated for top-{len(top)} by mean tuning DER",
        "",
        "## Tuning results",
        "",
    ]
    header = "| # | " + " | ".join(axes) + " |"
    for ds in tuning:
        header += f" {ds} files | {ds} DER % | {ds} FA | Miss | Conf |"
    header += " mean DER |"
    sep = "| --- |" + " ---: |" * (len(axes) + 5 * len(tuning) + 1)
    lines += [header, sep]
    for i, row in enumerate(tuning_scores):
        line = f"| {i + 1} | " + " | ".join(str(row["params"][k]) for k in axes) + " |"
        for ds in tuning:
            s = row[ds]
            line += f" {s['files']} | {s['der']:.2f} | {s['fa']:.2f} | {s['miss']:.2f} | {s['conf']:.2f} |"
        line += f" {mean_der(row):.2f} |"
        lines.append(line)

    lines += ["", "## Held-out validation (top candidates only)", ""]
    vheader = "| # | " + " | ".join(axes) + " |"
    for ds in validation:
        vheader += f" {ds} DER % | {ds} FA | Miss | Conf |"
    vheader += " mean held-out DER |"
    lines += [
        vheader,
        "| --- |" + " ---: |" * (len(axes) + 4 * len(validation) + 1),
    ]
    for rank, i in enumerate(top):
        row = tuning_scores[i]
        line = f"| {i + 1} | " + " | ".join(str(row["params"][k]) for k in axes) + " |"
        for ds in validation:
            s = val_scores[i][ds]
            line += f" {s['der']:.2f} | {s['fa']:.2f} | {s['miss']:.2f} | {s['conf']:.2f} |"
        if validation:
            mv = sum(val_scores[i][ds]["der"] for ds in validation) / len(validation)
            line += f" {mv:.2f} |"
        else:
            line += " n/a |"
        lines.append(line)
    lines.append("")
    out.write_text("\n".join(lines), encoding="utf-8")
    try:
        shown = out.relative_to(REPORTS_DIR.parent)
    except ValueError:
        shown = out
    print(f"sweep report written: {shown}")
    return out


def sweep(args) -> int:
    files: dict[str, list[str]] = {}
    if args.files:
        for spec in args.files:
            if "=" not in spec:
                raise SystemExit(f"sweep: bad --files '{spec}' (expected dataset:f1,f2)")
            ds, _, raw = spec.partition("=")
            files[ds] = [f for f in raw.split(",") if f]
    run_sweep(
        datasets=args.dataset,
        grid_specs=args.grid,
        run_prefix=args.run_prefix,
        workers=args.workers,
        files=files or None,
        validate_top=args.validate_top,
        validation_datasets=args.validation,
        report_path=Path(args.report).resolve() if args.report else None,
    )
    return 0
