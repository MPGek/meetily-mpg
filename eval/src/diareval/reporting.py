"""Markdown baseline-comparison report."""

from __future__ import annotations

import subprocess
from datetime import date
from pathlib import Path

from .manifests import ManifestError, load_manifest
from .paths import OUT_DIR, REPORTS_DIR


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


def write_report(datasets: list[str] | None, run_id: str = "latest") -> Path:
    if datasets is None:
        datasets = [
            p.parent.name
            for p in OUT_DIR.glob(f"*/{run_id}/score.json")
        ]
    if not datasets:
        raise SystemExit("no scored datasets found under eval/out")

    rows = []
    for ds in datasets:
        score_path = OUT_DIR / ds / run_id / "score.json"
        if not score_path.is_file():
            raise SystemExit(f"{ds}: no score.json — run `score --dataset {ds}` first")
        import json

        s = json.loads(score_path.read_text(encoding="utf-8"))
        try:
            baseline = load_manifest(ds).baseline_der
        except ManifestError:
            baseline = s.get("baseline_der")
        rows.append((s, baseline))

    rev = _git_rev()
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    out = REPORTS_DIR / f"{date.today().isoformat()}-{rev}.md"
    lines = [
        f"# Diarization evaluation report — {date.today().isoformat()} (git {rev})",
        "",
        "| Dataset | Files | Hours | DER % | FA % | Miss % | Conf % | Baseline % | Δ vs baseline |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for s, baseline in rows:
        delta = (
            f"{s['der'] - baseline:+.2f}"
            if baseline is not None
            else "n/a"
        )
        base_str = f"{baseline:.1f}" if baseline is not None else "n/a"
        lines.append(
            f"| {s['dataset']} | {s['files']} | {s['scored_hours']:.2f} | "
            f"{s['der']:.2f} | {s['fa']:.2f} | {s['miss']:.2f} | {s['conf']:.2f} | "
            f"{base_str} | {delta} |"
        )
    lines += [
        "",
        "Full setup: `DiarizationErrorRate(collar=0.0, skip_overlap=False)` over UEM-",
        "annotated regions; overlap counted. Baselines are published pyannote 3.1",
        "\"Full\" numbers declared per dataset in `eval/manifests/`.",
        "",
    ]
    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"report written: {out.relative_to(REPORTS_DIR.parent)}")
    return out
