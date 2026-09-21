"""Synthetic fixtures for the streaming metrics (tasks 5.1, 5.2, 5.4).

Every case here has offsets chosen so the expected numbers are computable by
hand, which is what makes the metric implementation checkable rather than
self-consistent.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from pyannote.core import Annotation, Segment, Timeline

from diareval import streaming


def _annotation(uri: str, turns: list[tuple[float, float, str]]) -> Annotation:
    ann = Annotation(uri=uri)
    for start, end, label in turns:
        ann[Segment(start, end)] = label
    return ann


def _write_sidecar(
    run_dir: Path,
    uri: str,
    emissions: list[tuple[float, float, str, bool]],
    *,
    production_faithful: bool = True,
) -> None:
    run_dir.mkdir(parents=True, exist_ok=True)
    lines = [
        json.dumps(
            {
                "record": "header",
                "uri": uri,
                "mode": "fast",
                "chunking": "production" if production_faithful else "fixed:0.6",
                "production_faithful": production_faithful,
                "model_family": "titanet_large",
                "sample_rate": 16000,
                "duration_secs": 20.0,
            }
        )
    ]
    for index, (start, end, speaker, stable) in enumerate(emissions):
        lines.append(
            json.dumps(
                {
                    "record": "emission",
                    "index": index,
                    "start": start,
                    "end": end,
                    "speaker": speaker,
                    "stable": stable,
                    "display_name": None,
                    "matched_by": None,
                    "match_score": None,
                }
            )
        )
    (run_dir / f"{uri}.events.jsonl").write_text("\n".join(lines) + "\n", encoding="utf-8")


def test_sidecar_round_trips_header_and_order(tmp_path: Path):
    _write_sidecar(
        tmp_path / "run",
        "rec",
        [(0.0, 1.5, "SPEAKER_00", False), (1.0, 2.5, "SPEAKER_00", True)],
    )
    sidecar = streaming.load_sidecar(tmp_path / "run", "rec")
    assert sidecar.production_faithful is True
    assert [e.index for e in sidecar.emissions] == [0, 1]
    assert [e.stable for e in sidecar.emissions] == [False, True]
    assert len(sidecar.stable) == 1
    assert sidecar.header["model_family"] == "titanet_large"


def test_missing_sidecar_fails_loudly_naming_the_recording(tmp_path: Path):
    with pytest.raises(SystemExit, match="rec-b: no streaming event sidecar"):
        streaming.load_sidecar(tmp_path / "run", "rec-b")


def test_emission_lag_matches_hand_computed_offsets(tmp_path: Path):
    # Reference: 00 speaks 1-3, 01 speaks 5-7, 00 again 11-13.
    reference = _annotation("rec", [(1.0, 3.0, "S0"), (5.0, 7.0, "S1"), (11.0, 13.0, "S0")])
    # Emissions covering the first two turns with known ends, and nothing
    # covering the third.
    _write_sidecar(
        tmp_path / "run",
        "rec",
        [
            (0.5, 2.0, "SPEAKER_00", True),  # covers 1.0 -> available 2.0, lag 1.0
            (4.0, 6.0, "SPEAKER_01", True),  # covers 5.0 -> available 6.0, lag 1.0
            (4.5, 8.0, "SPEAKER_01", True),  # later, so not the first cover
        ],
    )
    sidecar = streaming.load_sidecar(tmp_path / "run", "rec")

    lag = streaming.emission_lag(sidecar, reference)
    assert lag["turns"] == 3
    assert lag["covered"] == 2
    assert lag["uncovered"] == 1
    assert lag["lag_median"] == pytest.approx(1.0)
    assert lag["lag_p90"] == pytest.approx(1.0)
    assert lag["lag_max"] == pytest.approx(1.0)


def test_emission_lag_takes_the_earliest_cover_and_respects_the_uem(tmp_path: Path):
    reference = _annotation("rec", [(1.0, 3.0, "S0"), (11.0, 13.0, "S0")])
    _write_sidecar(
        tmp_path / "run",
        "rec",
        [
            (0.0, 4.0, "SPEAKER_00", True),  # covers 1.0 -> lag 3.0
            (0.9, 1.4, "SPEAKER_00", True),  # also covers, but arrived later
            (10.0, 12.0, "SPEAKER_00", True),  # covers 11.0 -> lag 1.0
        ],
    )
    sidecar = streaming.load_sidecar(tmp_path / "run", "rec")

    both = streaming.emission_lag(sidecar, reference)
    assert both["covered"] == 2
    assert both["lag_median"] == pytest.approx(2.0)  # median of 3.0 and 1.0

    # Restricting the annotated region to the first turn drops the second.
    uem = Timeline(uri="rec")
    uem.add(Segment(0.0, 5.0))
    first_only = streaming.emission_lag(sidecar, reference, uem=uem)
    assert first_only["turns"] == 1
    assert first_only["lag_median"] == pytest.approx(3.0)


def test_flip_rate_moves_for_alternating_labels_and_stays_put_when_clean(tmp_path: Path):
    reference = _annotation("rec", [(0.0, 6.0, "S0")])
    finalized = _annotation("rec", [(0.0, 6.0, "SPEAKER_00")])

    # One speaker rendered as alternating labels on overlapping emissions.
    _write_sidecar(
        tmp_path / "run-flip",
        "rec",
        [
            (0.0, 2.0, "SPEAKER_00", True),
            (1.0, 3.0, "SPEAKER_01", True),
            (2.0, 4.0, "SPEAKER_00", True),
            (3.0, 5.0, "SPEAKER_01", True),
        ],
    )
    flip = streaming.flip_and_fragmentation(
        streaming.load_sidecar(tmp_path / "run-flip", "rec"), reference, finalized
    )
    assert flip["flip_comparisons"] == 3
    assert flip["flip_rate"] == pytest.approx(1.0)
    assert flip["live_runs_per_speaker"] == pytest.approx(4.0)
    assert flip["final_runs_per_speaker"] == pytest.approx(1.0)
    assert flip["switch_rate_per_speaker_minute"] == pytest.approx(3.0 / (6.0 / 60.0))

    # A clean single-speaker stream: no flips, one run, no switches.
    _write_sidecar(
        tmp_path / "run-clean",
        "rec",
        [
            (0.0, 2.0, "SPEAKER_00", True),
            (1.0, 3.0, "SPEAKER_00", True),
            (2.0, 4.0, "SPEAKER_00", True),
        ],
    )
    clean = streaming.flip_and_fragmentation(
        streaming.load_sidecar(tmp_path / "run-clean", "rec"), reference, finalized
    )
    assert clean["flip_comparisons"] == 2
    assert clean["flip_rate"] == pytest.approx(0.0)
    assert clean["live_runs_per_speaker"] == pytest.approx(1.0)
    assert clean["final_runs_per_speaker"] == pytest.approx(1.0)
    assert clean["switch_rate_per_speaker_minute"] == pytest.approx(0.0)


def test_provisional_emissions_do_not_count_as_flips(tmp_path: Path):
    reference = _annotation("rec", [(0.0, 4.0, "S0")])
    finalized = _annotation("rec", [(0.0, 4.0, "SPEAKER_00")])
    _write_sidecar(
        tmp_path / "run",
        "rec",
        [
            (0.0, 2.0, "SPEAKER_00", True),
            (1.0, 3.0, "SPEAKER_09", False),  # provisional: never shown to a user
            (1.5, 3.5, "SPEAKER_00", True),  # overlaps the first, same label
        ],
    )
    metrics = streaming.flip_and_fragmentation(
        streaming.load_sidecar(tmp_path / "run", "rec"), reference, finalized
    )
    # The two stable emissions overlap and agree, so there is one comparison
    # and no flip. Counting the provisional entry would have made it two
    # comparisons and two flips.
    assert metrics["flip_comparisons"] == 1
    assert metrics["flip_rate"] == pytest.approx(0.0)
    assert metrics["live_runs_per_speaker"] == pytest.approx(1.0)


def test_real_time_factor_is_read_but_optional(tmp_path: Path):
    run_dir = tmp_path / "run"
    run_dir.mkdir(parents=True)
    assert streaming.read_rtf(run_dir, "rec") is None
    (run_dir / "rec.timing.json").write_text(
        json.dumps({"uri": "rec", "audio_secs": 10.0, "wall_secs": 2.5, "real_time_factor": 0.25}),
        encoding="utf-8",
    )
    assert streaming.read_rtf(run_dir, "rec") == pytest.approx(0.25)
