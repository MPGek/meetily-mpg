"""DER scoring with pyannote.metrics (Full setup: collar=0, overlap counted)."""

from __future__ import annotations

import json
import statistics
from pathlib import Path

from pyannote.core import Annotation, Segment, Timeline
from pyannote.database.util import load_rttm
from pyannote.metrics.diarization import DiarizationErrorRate

from .manifests import load_manifest
from .paths import DATA_DIR, OUT_DIR
from .runner import DEFAULT_MODE, check_mode, read_run_mode, run_dir
from .streaming import (
    emission_lag,
    flip_and_fragmentation,
    live_timeline,
    load_sidecar,
    read_rtf,
    read_stop_cost,
)


def _percentile(values: list[float], pct: float) -> float:
    from .streaming import _percentile as percentile

    return percentile(values, pct)


def _load_uem(path: Path) -> dict[str, Timeline]:
    uem: dict[str, Timeline] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        f = line.split()
        uri = f[0]
        start, end = float(f[2]), float(f[3])
        uem.setdefault(uri, Timeline(uri=uri)).add(Segment(start, end))
    return uem


def score_dataset(
    dataset: str,
    run_id: str = "latest",
    files: list[str] | None = None,
    exclude: set[str] | None = None,
    mode: str = DEFAULT_MODE,
) -> dict:
    """Score one run of one dataset, in one mode.

    A run is scored as the mode that produced it and nothing else: the mode
    selects the run directory, and the directory's recorded mode must match,
    so an online hypothesis can never be attributed to the offline baseline
    or the two mixed into a single number (D7).
    """
    check_mode(mode)
    data = DATA_DIR / dataset
    ref_path = data / "rttm" / "ref.rttm"
    uem_path = data / "uem" / "ref.uem"
    hyp_dir = run_dir(dataset, run_id, mode, out_dir=OUT_DIR)
    if not ref_path.is_file() or not uem_path.is_file():
        raise SystemExit(f"dataset '{dataset}': missing {ref_path} or {uem_path}")
    if not hyp_dir.is_dir():
        raise SystemExit(
            f"no {mode} hypothesis run at {hyp_dir} — "
            f"run `run --dataset {dataset} --mode {mode}` first"
        )
    produced_in = read_run_mode(hyp_dir)
    if produced_in != mode:
        raise SystemExit(
            f"refusing to score a {produced_in} run as {mode}: {hyp_dir} was produced by the "
            f"{produced_in} harness. Score it with --mode {produced_in}, or run the {mode} "
            f"harness first; the two modes are never combined in one score."
        )

    refs = dict(load_rttm(str(ref_path)))
    if files is not None:
        wanted = set(files)
        missing_refs = wanted - set(refs)
        if missing_refs:
            raise SystemExit(f"{dataset}: references missing for {sorted(missing_refs)[:5]}")
        refs = {uri: ann for uri, ann in refs.items() if uri in wanted}
    excluded = 0
    if exclude:
        before = len(refs)
        refs = {uri: ann for uri, ann in refs.items() if uri not in exclude}
        excluded = before - len(refs)
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
        "mode": mode,
        "files": len(refs),
        "excluded": excluded,
        "scored_hours": total_ref / 3600.0,
        "der": der * 100.0,
        "fa": comp["false_alarm"] / total_ref * 100.0,
        "miss": comp["missed"] / total_ref * 100.0,
        "conf": comp["confusion"] / total_ref * 100.0,
        "baseline_der": baseline,
    }
    if mode != DEFAULT_MODE:
        result["streaming"] = _streaming_metrics(hyp_dir, refs, hyps, uem)

    out = hyp_dir / "score.json"
    out.write_text(json.dumps(result, indent=1), encoding="utf-8")
    print(
        f"{dataset}: DER {result['der']:.2f}% "
        f"(FA {result['fa']:.2f} / Miss {result['miss']:.2f} / Conf {result['conf']:.2f}) "
        f"over {result['files']} files / {result['scored_hours']:.2f} h"
        + (f" | baseline {baseline:.1f}%" if baseline is not None else "")
    )
    if "streaming" in result:
        flip = result["streaming"]["live_final_flip"]
        uncovered = result["streaming"]["live_uncovered"]
        print(
            "  live-vs-final: flip "
            + (f"{flip:.2f}%" if flip is not None else "n/a")
            + ", uncovered "
            + (f"{uncovered:.2f}%" if uncovered is not None else "n/a")
        )
    return result


def _streaming_metrics(hyp_dir: Path, refs: dict, hyps: dict, uem: dict) -> dict:
    """Aggregate the streaming metrics of an online run, duration-weighted.

    Every recording must have its sidecar: a missing one is an error naming
    the recording, never a zero (spec: fail loudly). Real-time factor is
    averaged and reported here but is never a gate (D7).
    """
    weighted: dict[str, float] = {}
    weights: dict[str, float] = {}
    lag_samples: list[float] = []
    uncovered = 0
    turns = 0
    rtfs: list[float] = []
    ablation_files: list[str] = []
    stop_secs = 0.0
    stop_audio_secs = 0.0
    flip_secs = 0.0
    judged_secs = 0.0
    uncovered_secs = 0.0
    agreement_secs = 0.0

    for uri, ref in refs.items():
        sidecar = load_sidecar(hyp_dir, uri)
        if not sidecar.production_faithful:
            ablation_files.append(uri)
        region = uem.get(uri)
        lag = emission_lag(sidecar, ref, region)
        churn = flip_and_fragmentation(sidecar, ref, hyps[uri], region)

        turns += lag["turns"]
        uncovered += lag["uncovered"]
        if lag["lag_median"] is not None:
            # Weight a recording's lag by its covered turns so a long
            # recording does not count as much as a short one per turn.
            lag_samples.extend([lag["lag_median"]] * lag["covered"])

        secs = churn["speech_secs"] or 0.0
        for key in (
            "flip_rate",
            "switch_rate_per_speaker_minute",
            "live_runs_per_speaker",
            "final_runs_per_speaker",
        ):
            value = churn[key]
            if value is None or secs <= 0:
                continue
            weighted[key] = weighted.get(key, 0.0) + value * secs
            weights[key] = weights.get(key, 0.0) + secs

        # Summing the durations rather than averaging the ratios makes the
        # aggregate exactly duration-weighted (D6).
        agreement = live_final_agreement(sidecar, hyps[uri], ref, region)
        flip_secs += agreement["flip_secs"]
        judged_secs += agreement["judged_secs"]
        uncovered_secs += agreement["uncovered_secs"]
        agreement_secs += agreement["scored_secs"]

        rtf = read_rtf(hyp_dir, uri)
        if rtf is not None:
            rtfs.append(rtf)
        stop_cost = read_stop_cost(hyp_dir, uri)
        if stop_cost is not None:
            stop_secs += stop_cost[0]
            stop_audio_secs += stop_cost[1]

    # Every metric key is always present: a metric with no sample reports
    # None ("not measurable on this run"), never a missing field a report or a
    # gate could read as zero.
    metrics: dict = {
        key: (weighted[key] / weights[key]) if weights.get(key, 0.0) > 0 else None
        for key in (
            "flip_rate",
            "switch_rate_per_speaker_minute",
            "live_runs_per_speaker",
            "final_runs_per_speaker",
        )
    }
    metrics["live_final_flip"] = (
        (flip_secs / judged_secs) * 100.0 if judged_secs > 0 else None
    )
    metrics["live_uncovered"] = (
        (uncovered_secs / agreement_secs) * 100.0 if agreement_secs > 0 else None
    )
    metrics["reference_turns"] = turns
    metrics["uncovered_turns"] = uncovered
    metrics["lag_median"] = statistics.median(lag_samples) if lag_samples else None
    metrics["lag_p90"] = _percentile(lag_samples, 90.0) if lag_samples else None
    # Recorded, never gated: this measures the machine, not the pipeline.
    metrics["real_time_factor"] = (sum(rtfs) / len(rtfs)) if rtfs else None
    # The stop-time pass on its own, per hour of audio so runs of different
    # length compare. None for a run recorded before it was measured.
    metrics["finalize_secs_per_audio_hour"] = (
        (stop_secs / stop_audio_secs) * 3600.0 if stop_audio_secs > 0 else None
    )
    if ablation_files:
        metrics["ablation_files"] = len(ablation_files)
        metrics["production_faithful"] = False
    else:
        metrics["production_faithful"] = True
    return metrics


def _mapped(label, mapping: dict) -> tuple[str, object]:
    """A label in a space where live and final labels are comparable.

    Namespaced so a mapped reference name can never collide with a raw
    cluster id: a label the optimal mapping does not cover keeps its own id,
    because the same unmapped id on both sides did not change for the user
    even though neither side has a reference counterpart.
    """
    if label in mapping:
        return ("ref", mapping[label])
    return ("raw", label)


def live_final_agreement(
    sidecar,
    finalized: Annotation,
    reference: Annotation,
    uem: Timeline | None = None,
) -> dict:
    """How much of the finalized speech carries a different label than was shown live.

    Design D6. `L(t)` is the label of the latest emission covering `t` (see
    `streaming.live_timeline`) and `F(t)` the finalized label. Both are mapped
    into the reference label space through their own optimal mapping — the one
    DER uses — because a re-clustering at stop may renumber clusters, and a raw
    id comparison would then report a flip for every instant. Scored over
    `support(F)` intersected with the annotated region, so silence and
    unannotated audio never enter the denominator.

    Durations are returned alongside the two ratios so an aggregate over
    recordings can be duration-weighted exactly.
    """
    metric = DiarizationErrorRate(collar=0.0, skip_overlap=False)
    map_final = metric.optimal_mapping(reference, finalized, uem=uem)

    live_spans = live_timeline(sidecar)
    live = Annotation(uri=sidecar.uri)
    for index, (start, end, label) in enumerate(live_spans):
        live[Segment(start, end), index] = label
    map_live = metric.optimal_mapping(reference, live, uem=uem) if live_spans else {}

    scored = finalized.get_timeline().support()
    if uem is not None:
        scored = scored.crop(uem, mode="intersection")

    agree = 0.0
    disagree = 0.0
    uncovered = 0.0
    for segment, _track, final_label in finalized.itertracks(yield_label=True):
        final_key = _mapped(final_label, map_final)
        for piece in scored.crop(segment, mode="intersection"):
            covered = 0.0
            for start, end, live_label in live_spans:
                overlap = min(end, piece.end) - max(start, piece.start)
                if overlap <= 0:
                    continue
                covered += overlap
                if _mapped(live_label, map_live) == final_key:
                    agree += overlap
                else:
                    disagree += overlap
            uncovered += max(0.0, piece.duration - covered)

    judged = agree + disagree
    total = judged + uncovered
    return {
        "live_final_flip": (disagree / judged) if judged > 0 else None,
        "live_uncovered": (uncovered / total) if total > 0 else None,
        "flip_secs": disagree,
        "judged_secs": judged,
        "uncovered_secs": uncovered,
        "scored_secs": total,
    }


def score(args) -> int:
    score_dataset(args.dataset, args.run_id, mode=getattr(args, "mode", DEFAULT_MODE))
    return 0
