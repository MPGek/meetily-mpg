"""Report rendering for one-mode and two-mode datasets (task 6.1)."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from diareval import reporting, runner

OFFLINE_SCORE = {
    "dataset": "fixture-ds",
    "run_id": "latest",
    "mode": "offline",
    "files": 2,
    "excluded": 0,
    "scored_hours": 0.5,
    "der": 14.80,
    "fa": 2.0,
    "miss": 5.0,
    "conf": 7.8,
    "baseline_der": 12.0,
}

ONLINE_SCORE = {
    "dataset": "fixture-ds",
    "run_id": "latest",
    "mode": "online",
    "files": 2,
    "excluded": 0,
    "scored_hours": 0.5,
    "der": 22.40,
    "fa": 3.0,
    "miss": 6.0,
    "conf": 13.4,
    "baseline_der": 12.0,
    "streaming": {
        "flip_rate": 0.12,
        "switch_rate_per_speaker_minute": 1.5,
        "live_runs_per_speaker": 6.0,
        "final_runs_per_speaker": 2.0,
        "reference_turns": 40,
        "uncovered_turns": 3,
        "lag_median": 1.25,
        "lag_p90": 3.5,
        "real_time_factor": 0.31,
        "production_faithful": True,
    },
}


@pytest.fixture()
def patched(tmp_path: Path, monkeypatch):
    out = tmp_path / "out"
    reports = tmp_path / "reports"
    monkeypatch.setattr(reporting, "OUT_DIR", out)
    monkeypatch.setattr(reporting, "REPORTS_DIR", reports)
    monkeypatch.setattr(runner, "OUT_DIR", out)
    return tmp_path


def _write_score(out: Path, dataset: str, run_id: str, mode: str, payload: dict) -> None:
    directory = runner.run_dir(dataset, run_id, mode, out_dir=out)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "score.json").write_text(json.dumps(payload), encoding="utf-8")


def test_offline_only_report_has_no_online_section(patched):
    out = patched / "out"
    _write_score(out, "fixture-ds", "latest", "offline", OFFLINE_SCORE)

    path = reporting.write_report(["fixture-ds"], "latest")
    text = path.read_text(encoding="utf-8")

    assert "| fixture-ds | 2 | 0.50 | 14.80 |" in text
    # A single-mode dataset must not imply a comparison that was not made.
    assert "Online (live path" not in text
    assert "Δ vs offline" not in text


def test_two_mode_report_shows_both_and_the_delta(patched):
    out = patched / "out"
    _write_score(out, "fixture-ds", "latest", "offline", OFFLINE_SCORE)
    _write_score(out, "fixture-ds", "latest", "online", ONLINE_SCORE)

    path = reporting.write_report(["fixture-ds"], "latest")
    text = path.read_text(encoding="utf-8")

    assert "## Online (live path, Fast mode)" in text
    # online DER, the online-minus-offline delta, and the streaming columns
    assert "| 22.40 | +7.60 |" in text
    assert "1.250" in text and "3.500" in text  # lag median / p90
    assert "| 3 |" in text  # uncovered turns, counted not hidden
    assert "0.120" in text  # flip rate
    assert "6.000" in text and "2.000" in text  # runs per speaker live / final
    assert "0.310" in text  # real-time factor, reported
    assert "never part of a gate" in text


def test_ablation_run_is_labelled_in_the_report(patched):
    out = patched / "out"
    ablation = json.loads(json.dumps(ONLINE_SCORE))
    ablation["streaming"]["production_faithful"] = False
    _write_score(out, "fixture-ds", "latest", "offline", OFFLINE_SCORE)
    _write_score(out, "fixture-ds", "latest", "online", ablation)

    text = reporting.write_report(["fixture-ds"], "latest").read_text(encoding="utf-8")
    assert "fixture-ds (ablation)" in text
