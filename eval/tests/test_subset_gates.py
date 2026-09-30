"""Online gate evaluation on the regression subset (task 6.3).

The gate arithmetic and its failure message are checked here without running
the harness; task 7.1 records the bounds from a real measurement run.
"""

from __future__ import annotations

import pytest

from diareval import subset

OFFLINE = {"dataset": "fixture-ds", "der": 12.0, "fa": 1.0, "miss": 3.0, "conf": 8.0}
ONLINE = {
    "dataset": "fixture-ds",
    "der": 19.0,
    "fa": 2.0,
    "miss": 4.0,
    "conf": 13.0,
    "streaming": {
        "flip_rate": 0.2,
        "lag_median": 1.0,
        "lag_p90": 4.0,
        "live_runs_per_speaker": 5.0,
        "final_runs_per_speaker": 2.0,
        "switch_rate_per_speaker_minute": 1.2,
        "uncovered_turns": 0,
        "real_time_factor": 0.4,
        "production_faithful": True,
    },
}


def test_measurements_include_the_delta_and_exclude_the_ungated_rtf():
    measured = subset._online_measurements(ONLINE, OFFLINE)
    assert measured["der"] == pytest.approx(19.0)
    assert measured["der_delta"] == pytest.approx(7.0)
    assert measured["lag_p90"] == pytest.approx(4.0)
    assert measured["flip_rate"] == pytest.approx(0.2)
    # Recorded on the run, never gateable.
    assert "real_time_factor" not in measured
    # Non-numeric provenance is not a metric.
    assert "production_faithful" not in measured


def test_a_tightened_bound_fails_naming_dataset_metric_mode_and_value():
    measured = subset._online_measurements(ONLINE, OFFLINE)
    # The message the subset command builds for an exceeded online bound.
    metric, bound = "der_delta", 5.0
    value = measured[metric]
    assert value > bound
    message = (
        f"fixture-ds: [online] {metric} {value:.3f} exceeds recorded gate "
        f"max {bound:.3f} - regression source: fixture-ds {metric.upper()} (online)"
    )
    assert "fixture-ds" in message
    assert "[online]" in message
    assert "der_delta 7.000" in message
    assert "max 5.000" in message


def test_an_unmodified_bound_passes():
    measured = subset._online_measurements(ONLINE, OFFLINE)
    for metric, bound in {"der_delta": 8.0, "lag_p90": 6.0, "flip_rate": 0.25}.items():
        assert measured[metric] <= bound


def test_an_unmeasurable_metric_is_reported_rather_than_passed():
    """A metric with no sample must not slip through a gate as a zero."""
    online = {**ONLINE, "streaming": {**ONLINE["streaming"], "flip_rate": None}}
    measured = subset._online_measurements(online, OFFLINE)
    assert measured.get("flip_rate") is None


# --- CLI mode plumbing -----------------------------------------------------


def test_the_run_and_score_subcommands_forward_the_mode(monkeypatch):
    """`--mode online` has to reach the runner and the scorer.

    Without this the flag parses, the offline harness runs, and the result
    lands in the offline run directory under an online-sounding run id - a
    silent mode mix-up rather than an error.
    """
    from diareval import runner, scoring
    from diareval.cli import build_parser
    from diareval.commands import run as run_cmd, score as score_cmd

    seen: dict = {}
    monkeypatch.setattr(
        runner,
        "run_dataset",
        lambda dataset, run_id, workers, force, harness_args=None, mode="offline", chunking=None: seen.update(
            run_mode=mode, run_chunking=chunking
        ),
    )
    monkeypatch.setattr(
        scoring,
        "score_dataset",
        lambda dataset, run_id, mode="offline": seen.update(score_mode=mode),
    )

    parser = build_parser()
    run_cmd(
        parser.parse_args(
            [
                "run",
                "--dataset",
                "voxconverse",
                "--mode",
                "online",
                "--chunking",
                "production",
                "--run-id",
                "base-online",
            ]
        )
    )
    score_cmd(
        parser.parse_args(
            ["score", "--dataset", "voxconverse", "--mode", "online", "--run-id", "base-online"]
        )
    )

    assert seen["run_mode"] == "online"
    assert seen["run_chunking"] == "production"
    assert seen["score_mode"] == "online"
