"""Synthetic fixtures for DER scoring (task 5.2 verification)."""

from __future__ import annotations

from pathlib import Path

import pytest

from diareval import scoring

REF_TURNS = {
    "rec-a": [(0.0, 5.0, "SPEAKER_00"), (5.0, 10.0, "SPEAKER_01")],
    "rec-b": [(0.0, 4.0, "SPEAKER_00"), (4.0, 9.0, "SPEAKER_01")],
}


def _materialize(tmp_path: Path, hyp_builder) -> Path:
    data = tmp_path / "data" / "fixture-ds"
    (data / "rttm").mkdir(parents=True)
    (data / "uem").mkdir(parents=True)
    rttm, uem = [], []
    for uri, turns in REF_TURNS.items():
        uem.append(f"{uri} 1 0.0 10.0")
        for start, end, spk in turns:
            rttm.append(
                f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} <NA> <NA> {spk} <NA> <NA>"
            )
    (data / "rttm" / "ref.rttm").write_text("\n".join(rttm) + "\n", encoding="utf-8")
    (data / "uem" / "ref.uem").write_text("\n".join(uem) + "\n", encoding="utf-8")

    hyp_dir = tmp_path / "out" / "fixture-ds" / "fixture"
    hyp_dir.mkdir(parents=True)
    for uri, turns in REF_TURNS.items():
        lines = [
            f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} <NA> <NA> {spk} <NA> <NA>"
            for start, end, spk in hyp_builder(uri, turns)
        ]
        (hyp_dir / f"{uri}.rttm").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return hyp_dir


@pytest.fixture()
def patched(tmp_path: Path, monkeypatch):
    monkeypatch.setattr(scoring, "DATA_DIR", tmp_path / "data")
    monkeypatch.setattr(scoring, "OUT_DIR", tmp_path / "out")
    return tmp_path


def identity(uri, turns):
    return turns


def all_one_speaker(uri, turns):
    return [(s, e, "SPEAKER_00") for s, e, _ in turns]


def test_reference_duplicated_scores_zero(patched):
    _materialize(patched, identity)
    result = scoring.score_dataset("fixture-ds", "fixture")
    assert result["der"] == pytest.approx(0.0, abs=1e-6)
    assert result["fa"] == pytest.approx(0.0, abs=1e-6)
    assert result["miss"] == pytest.approx(0.0, abs=1e-6)
    assert result["conf"] == pytest.approx(0.0, abs=1e-6)


def test_merged_speakers_produce_confusion(patched):
    _materialize(patched, all_one_speaker)
    result = scoring.score_dataset("fixture-ds", "fixture")
    assert result["conf"] > 0.0
    assert result["der"] == pytest.approx(result["conf"], abs=1e-6)
    assert result["miss"] == pytest.approx(0.0, abs=1e-6)
    assert result["fa"] == pytest.approx(0.0, abs=1e-6)


def test_missing_hypotheses_hard_fail(patched):
    hyp_dir = _materialize(patched, identity)
    (hyp_dir / "rec-b.rttm").unlink()
    with pytest.raises(SystemExit, match="lack hypotheses"):
        scoring.score_dataset("fixture-ds", "fixture")


def test_score_json_written(patched):
    _materialize(patched, identity)
    scoring.score_dataset("fixture-ds", "fixture")
    out = patched / "out" / "fixture-ds" / "fixture" / "score.json"
    assert out.is_file()
    import json

    data = json.loads(out.read_text(encoding="utf-8"))
    assert data["dataset"] == "fixture-ds"
    assert data["files"] == 2
