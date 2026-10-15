"""Fast regression subset: ≤10 recordings, one command, per-source DER."""

from __future__ import annotations

from .manifests import list_manifests
from .paths import DATA_DIR


def subset_datasets() -> list:
    return [m for m in list_manifests() if m.subset and m.subset_files]


def prepare_once(manifest) -> None:
    """Download + normalize a subset dataset if not materialized yet."""
    wav_dir = DATA_DIR / manifest.name / "wav"
    if wav_dir.is_dir() and any(wav_dir.glob("*.wav")):
        return
    from . import downloads
    from .normalize import normalize_dataset

    for src in manifest.sources:
        if src.kind == "http":
            downloads.fetch_http(src, manifest.name)
        elif src.kind == "gdrive":
            downloads.fetch_gdrive(src, manifest.name)
        elif src.kind == "hf":
            downloads.fetch_hf(src, manifest.name)
    normalize_dataset(manifest)


def run_subset(
    force: bool = False,
    run_id: str = "subset",
    harness_args: list[str] | None = None,
) -> dict:
    manifests = subset_datasets()
    if not manifests:
        raise SystemExit(
            "no subset datasets — mark a manifest with `subset: true` and "
            "`subset_files: [...]`"
        )
    total_files = sum(len(m.subset_files) for m in manifests)
    if total_files > 10:
        raise SystemExit(
            f"subset declares {total_files} files across {len(manifests)} datasets "
            "(spec: at most 10)"
        )

    from .runner import run_dataset
    from .scoring import score_dataset

    results = {}
    online_results: dict[str, dict] = {}
    for m in manifests:
        prepare_once(m)
        run_dataset(
            m.name, run_id=run_id, force=force, files=list(m.subset_files),
            harness_args=harness_args,
        )
        results[m.name] = score_dataset(m.name, run_id=run_id, files=list(m.subset_files))
        # A dataset that declares online bounds is also measured through
        # the live path, on the same files, with production-faithful
        # chunking (an ablation policy may never feed a gate).
        if m.online_gate:
            run_dataset(
                m.name, run_id=run_id, force=force, files=list(m.subset_files),
                mode="online", chunking="production",
            )
            online_results[m.name] = score_dataset(
                m.name, run_id=run_id, files=list(m.subset_files), mode="online"
            )

    print("\nsubset results:")
    for name, r in results.items():
        print(
            f"  {name} [offline]: DER {r['der']:.2f}% "
            f"(FA {r['fa']:.2f} / Miss {r['miss']:.2f} / Conf {r['conf']:.2f})"
        )
        online = online_results.get(name)
        if online:
            metrics = online.get("streaming", {})
            print(
                f"  {name} [online]:  DER {online['der']:.2f}% "
                f"(delta {online['der'] - r['der']:+.2f}) "
                f"lag p90 {_fmt(metrics.get('lag_p90'))}s "
                f"flip {_fmt(metrics.get('flip_rate'))} "
                f"runs/speaker live {_fmt(metrics.get('live_runs_per_speaker'))} "
                f"final {_fmt(metrics.get('final_runs_per_speaker'))} "
                f"| RTF {_fmt(metrics.get('real_time_factor'))} (recorded, not gated)"
            )

    gate_failures: list[str] = []
    for m in manifests:
        if not m.subset_gate:
            continue
        r = results[m.name]
        for metric, bound in sorted(m.subset_gate.items()):
            if r[metric] > bound:
                gate_failures.append(
                    f"{m.name}: {metric} {r[metric]:.2f}% exceeds recorded gate max "
                    f"{bound:.2f}% — regression source: {m.name} {metric.upper()}"
                )
    for m in manifests:
        if not m.online_gate:
            continue
        online = online_results.get(m.name)
        if online is None:
            gate_failures.append(
                f"{m.name}: online gate declared but no online run was scored"
            )
            continue
        measured = _online_measurements(online, results[m.name])
        for metric, bound in sorted(m.online_gate.items()):
            value = measured.get(metric)
            if value is None:
                gate_failures.append(
                    f"{m.name}: [online] {metric} is not measurable on this run "
                    f"(no sample), so its recorded bound {bound} cannot be evaluated"
                )
                continue
            if value > bound:
                gate_failures.append(
                    f"{m.name}: [online] {metric} {value:.3f} exceeds recorded gate "
                    f"max {bound:.3f} - regression source: {m.name} {metric.upper()} "
                    f"(online)"
                )

    if gate_failures:
        print("\nsubset gate FAILED:")
        for line in gate_failures:
            print(f"  {line}")
        raise SystemExit(1)
    if any(m.subset_gate or m.online_gate for m in manifests):
        print("subset gate PASSED: all recorded metric ranges met")
    if online_results:
        return {"offline": results, "online": online_results}
    return results

def _fmt(value) -> str:
    return "n/a" if value is None else f"{value:.3f}"


def _online_measurements(online: dict, offline: dict) -> dict:
    """The gateable online metrics of one dataset, in one flat mapping.

    `der_delta` is the online run's distance from the offline run over the
    same files: the number that says whether the live path is losing quality
    against the calibrated baseline. `conf_delta` is the same distance on the
    confusion component alone, for a dataset whose reference inflates Miss so
    much that its total DER says more about the reference than the pipeline
    (ru-synthetic; design D7).
    """
    measured = {key: online.get(key) for key in ("der", "fa", "miss", "conf")}
    measured["der_delta"] = online["der"] - offline["der"]
    measured["conf_delta"] = online["conf"] - offline["conf"]
    measured.update(
        {
            key: value
            for key, value in (online.get("streaming") or {}).items()
            if isinstance(value, (int, float)) and not isinstance(value, bool)
        }
    )
    # Recorded for information only: the manifest loader rejects a bound on
    # it, and nothing here consults it.
    measured.pop("real_time_factor", None)
    measured.pop("finalize_secs_per_audio_hour", None)
    return measured
