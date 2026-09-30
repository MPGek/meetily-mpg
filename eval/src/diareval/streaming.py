"""Streaming-only metrics for an online run (diarization-eval-streaming-metrics).

What offline DER cannot express: how late a speaker's label became available,
how often it changed afterwards, and how many separate runs of labels one
reference speaker was rendered as. All of it is computed from the event
sidecar the `online-eval` harness writes, against the same reference and the
same annotated regions the DER uses (design D5), so the two sets of numbers
describe the same audio.

Times are audio time throughout. An emission's availability is taken as the
end of its own span: the streaming pipeline cannot have produced a turn before
consuming the audio it covers, so that end is the honest audio-time lower
bound for "when the label existed". Wall-clock never enters a metric here; the
real-time factor is read from the harness's timing file and reported, never
gated (D7).
"""

from __future__ import annotations

import json
import statistics
from dataclasses import dataclass
from pathlib import Path

from pyannote.core import Segment, Timeline


@dataclass(frozen=True)
class Emission:
    """One record of the event sidecar."""

    index: int
    start: float
    end: float
    speaker: str
    stable: bool

    @property
    def available_at(self) -> float:
        """Audio time from which this label existed (see the module docstring)."""
        return self.end


@dataclass(frozen=True)
class Sidecar:
    """A recording's emission stream plus the harness's run header."""

    uri: str
    header: dict
    emissions: list[Emission]

    @property
    def production_faithful(self) -> bool:
        return bool(self.header.get("production_faithful", False))

    @property
    def stable(self) -> list[Emission]:
        return [e for e in self.emissions if e.stable]


class MissingSidecar(SystemExit):
    """Raised when a metric needs a sidecar the run does not have.

    A missing sidecar is never reported as a zero or an omitted metric: the
    command exits non-zero naming the recording (spec: fail loudly).
    """


def sidecar_path(run_dir: Path, uri: str) -> Path:
    return run_dir / f"{uri}.events.jsonl"


def load_sidecar(run_dir: Path, uri: str) -> Sidecar:
    path = sidecar_path(run_dir, uri)
    if not path.is_file():
        raise MissingSidecar(
            f"{uri}: no streaming event sidecar at {path}. An online run must write one "
            f"per recording; re-run the dataset in online mode for this file."
        )
    header: dict = {}
    emissions: list[Emission] = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as exc:
            raise MissingSidecar(f"{uri}: malformed sidecar at {path}:{lineno}: {exc}") from exc
        kind = record.get("record")
        if kind == "header":
            header = record
        elif kind == "emission":
            emissions.append(
                Emission(
                    index=int(record["index"]),
                    start=float(record["start"]),
                    end=float(record["end"]),
                    speaker=str(record["speaker"]),
                    stable=bool(record["stable"]),
                )
            )
        else:
            raise MissingSidecar(f"{uri}: unknown sidecar record kind {kind!r} at {path}:{lineno}")
    if not header:
        raise MissingSidecar(f"{uri}: sidecar at {path} has no provenance header")
    emissions.sort(key=lambda e: e.index)
    return Sidecar(uri=uri, header=header, emissions=emissions)


def live_timeline(sidecar: Sidecar) -> list[tuple[float, float, str]]:
    """The label a user actually saw at each instant, as disjoint spans.

    Design D6's `L(t)`: the label of the emission with the highest emission
    index covering `t`, so a later emission wins the region it revises. Built
    from the *stable* emissions only, because those are the ones the app
    receives and displays — a provisional emission never reaches a user, so
    including it would measure a label nobody saw.

    Returns spans sorted by start, with adjacent same-label spans joined.
    """
    spans: list[tuple[float, float, str]] = []
    for emission in sorted(sidecar.stable, key=lambda e: e.index):
        start, end = emission.start, emission.end
        if end <= start:
            continue
        kept: list[tuple[float, float, str]] = []
        for a, b, label in spans:
            if b <= start or a >= end:
                kept.append((a, b, label))
                continue
            if a < start:
                kept.append((a, start, label))
            if b > end:
                kept.append((end, b, label))
        kept.append((start, end, emission.speaker))
        kept.sort(key=lambda s: s[0])
        spans = kept

    joined: list[tuple[float, float, str]] = []
    for start, end, label in spans:
        if joined and joined[-1][2] == label and abs(start - joined[-1][1]) < 1e-9:
            joined[-1] = (joined[-1][0], end, label)
        else:
            joined.append((start, end, label))
    return joined


def read_rtf(run_dir: Path, uri: str) -> float | None:
    """The harness's recorded real-time factor, or None when absent.

    Reported alongside the quality metrics and never gated: it measures this
    machine, not the pipeline.
    """
    path = run_dir / f"{uri}.timing.json"
    if not path.is_file():
        return None
    try:
        return float(json.loads(path.read_text(encoding="utf-8"))["real_time_factor"])
    except (json.JSONDecodeError, KeyError, TypeError, ValueError):
        return None


def read_stop_cost(run_dir: Path, uri: str) -> tuple[float, float] | None:
    """`(finalize_secs, audio_secs)` from the harness's timing file, or None.

    The stop-time refinement is paid exactly when the user is waiting for the
    meeting to save, so it is reported on its own and never folded into the
    real-time factor. A run recorded before the harness measured it simply has
    no figure, which is "not measured", not zero.
    """
    path = run_dir / f"{uri}.timing.json"
    if not path.is_file():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        return float(payload["finalize_secs"]), float(payload["audio_secs"])
    except (json.JSONDecodeError, KeyError, TypeError, ValueError):
        return None


def _in_uem(segment: Segment, uem: Timeline | None) -> bool:
    if uem is None:
        return True
    return bool(uem.crop(segment, mode="intersection"))


def emission_lag(sidecar: Sidecar, reference, uem: Timeline | None = None) -> dict:
    """How late each reference turn's first covering emission was.

    For every reference turn inside the annotated region, the earliest
    emission (in arrival order) whose span covers the turn's start is taken,
    and the lag is that emission's availability minus the turn start. Turns
    that no emission ever covered are counted, never silently scored as zero.
    """
    lags: list[float] = []
    uncovered: list[tuple[float, str]] = []
    covered_turns = 0
    for turn, _track, label in reference.itertracks(yield_label=True):
        if not _in_uem(turn, uem):
            continue
        first = next(
            (e for e in sidecar.emissions if e.start <= turn.start < e.end),
            None,
        )
        if first is None:
            uncovered.append((turn.start, label))
            continue
        covered_turns += 1
        lags.append(max(0.0, first.available_at - turn.start))

    return {
        "turns": covered_turns + len(uncovered),
        "covered": covered_turns,
        "uncovered": len(uncovered),
        "lag_median": statistics.median(lags) if lags else None,
        "lag_p90": _percentile(lags, 90.0) if lags else None,
        "lag_max": max(lags) if lags else None,
    }


def _percentile(values: list[float], pct: float) -> float:
    if not values:
        raise ValueError("percentile of an empty sample")
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    rank = (pct / 100.0) * (len(ordered) - 1)
    low = int(rank)
    high = min(low + 1, len(ordered) - 1)
    frac = rank - low
    return ordered[low] * (1.0 - frac) + ordered[high] * frac


def flip_and_fragmentation(
    sidecar: Sidecar,
    reference,
    finalized,
    uem: Timeline | None = None,
) -> dict:
    """Label churn and fragmentation for one recording.

    - `flip_rate`: the share of consecutive stable emissions that changed the
      label of a region they both cover (a relabel the user would see).
    - `switch_rate_per_speaker_minute`: cluster switches inside one reference
      speaker's active speech, per minute of that speech.
    - `live_runs_per_speaker` / `final_runs_per_speaker`: how many separate
      label runs one reference speaker was rendered as, live and after
      finalization, so a fix that only tidies the finalized view is visible.
    """
    stable = sidecar.stable
    flips = 0
    comparisons = 0
    for previous, current in zip(stable, stable[1:]):
        overlap = min(previous.end, current.end) - max(previous.start, current.start)
        if overlap <= 0:
            continue
        comparisons += 1
        if previous.speaker != current.speaker:
            flips += 1

    live_runs: dict[str, int] = {}
    final_runs: dict[str, int] = {}
    switches = 0
    speech_secs = 0.0
    for label in reference.labels():
        speaker_timeline = reference.label_timeline(label)
        if uem is not None:
            speaker_timeline = speaker_timeline.crop(uem, mode="intersection")
        secs = speaker_timeline.duration()
        if secs <= 0:
            continue
        speech_secs += secs

        live_sequence = _label_sequence(
            [(e.start, e.end, e.speaker) for e in stable], speaker_timeline
        )
        final_sequence = _label_sequence(
            [(seg.start, seg.end, lbl) for seg, _t, lbl in finalized.itertracks(yield_label=True)],
            speaker_timeline,
        )
        live_runs[label] = len(live_sequence)
        final_runs[label] = len(final_sequence)
        switches += max(0, len(live_sequence) - 1)

    return {
        "flip_rate": (flips / comparisons) if comparisons else None,
        "flip_comparisons": comparisons,
        "switch_rate_per_speaker_minute": (switches / (speech_secs / 60.0))
        if speech_secs > 0
        else None,
        "live_runs_per_speaker": _mean(list(live_runs.values())),
        "final_runs_per_speaker": _mean(list(final_runs.values())),
        "reference_speakers": len(live_runs),
        "speech_secs": speech_secs,
    }


def _label_sequence(spans: list[tuple[float, float, str]], within: Timeline) -> list[str]:
    """The sequence of distinct consecutive labels covering `within`.

    Consecutive spans carrying the same label collapse into one run, so the
    length of the result is the number of label runs that speaker was
    rendered as.
    """
    touching = [
        (start, end, label)
        for start, end, label in spans
        if end > start and within.crop(Segment(start, end), mode="intersection")
    ]
    touching.sort(key=lambda s: (s[0], s[1]))
    sequence: list[str] = []
    for _start, _end, label in touching:
        if not sequence or sequence[-1] != label:
            sequence.append(label)
    return sequence


def _mean(values: list[int]) -> float | None:
    return (sum(values) / len(values)) if values else None
