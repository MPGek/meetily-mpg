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


# --------------------------------------------------------------------------
# Online mode (add-online-diarization-eval task 4.2): the same scorer, the
# same setup, attributed to the run's own mode and never mixed with offline.
# --------------------------------------------------------------------------


def _materialize_online(tmp_path: Path, hyp_builder, with_sidecars: bool = True) -> Path:
    import json

    data = tmp_path / "data" / "fixture-ds"
    (data / "rttm").mkdir(parents=True, exist_ok=True)
    (data / "uem").mkdir(parents=True, exist_ok=True)
    rttm, uem = [], []
    for uri, turns in REF_TURNS.items():
        uem.append(f"{uri} 1 0.0 10.0")
        for start, end, spk in turns:
            rttm.append(
                f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} <NA> <NA> {spk} <NA> <NA>"
            )
    (data / "rttm" / "ref.rttm").write_text("\n".join(rttm) + "\n", encoding="utf-8")
    (data / "uem" / "ref.uem").write_text("\n".join(uem) + "\n", encoding="utf-8")

    hyp_dir = tmp_path / "out" / "fixture-ds" / "online" / "fixture"
    hyp_dir.mkdir(parents=True, exist_ok=True)
    (hyp_dir / "run.json").write_text(
        json.dumps({"dataset": "fixture-ds", "run_id": "fixture", "mode": "online",
                    "binary": "online-eval", "chunking": "production"}),
        encoding="utf-8",
    )
    for uri, turns in REF_TURNS.items():
        hyp = hyp_builder(uri, turns)
        (hyp_dir / f"{uri}.rttm").write_text(
            "\n".join(
                f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} <NA> <NA> {spk} <NA> <NA>"
                for start, end, spk in hyp
            )
            + "\n",
            encoding="utf-8",
        )
        if with_sidecars:
            lines = [
                json.dumps(
                    {
                        "record": "header",
                        "uri": uri,
                        "mode": "fast",
                        "chunking": "production",
                        "production_faithful": True,
                        "model_family": "titanet_large",
                        "sample_rate": 16000,
                        "duration_secs": 10.0,
                    }
                )
            ]
            for index, (start, end, spk) in enumerate(hyp):
                lines.append(
                    json.dumps(
                        {
                            "record": "emission",
                            "index": index,
                            "start": start,
                            "end": end,
                            "speaker": spk,
                            "stable": True,
                            "display_name": None,
                            "matched_by": None,
                            "match_score": None,
                        }
                    )
                )
            (hyp_dir / f"{uri}.events.jsonl").write_text(
                "\n".join(lines) + "\n", encoding="utf-8"
            )
    return hyp_dir


def test_online_reference_duplicated_scores_zero(patched):
    _materialize_online(patched, identity)
    result = scoring.score_dataset("fixture-ds", "fixture", mode="online")
    assert result["mode"] == "online"
    assert result["der"] == pytest.approx(0.0, abs=1e-6)
    # The streaming metrics come with it, computed against the same reference.
    streaming_metrics = result["streaming"]
    assert streaming_metrics["uncovered_turns"] == 0
    assert streaming_metrics["reference_turns"] == 4
    assert streaming_metrics["production_faithful"] is True
    # This fixture's emissions are the reference turns themselves, so they
    # never overlap: there is no pair to compare, and the flip rate is
    # reported as "not measurable" rather than as a zero.
    assert streaming_metrics["flip_rate"] is None
    # Each reference speaker is rendered as exactly one run, live and final.
    assert streaming_metrics["live_runs_per_speaker"] == pytest.approx(1.0)
    assert streaming_metrics["final_runs_per_speaker"] == pytest.approx(1.0)
    # Lag: each emission spans its own turn, so the label is available at the
    # turn's end -> a lag of the turn's own length (5 s and 4/5 s here).
    assert streaming_metrics["lag_median"] is not None


def test_online_shuffled_speakers_produce_confusion(patched):
    _materialize_online(patched, all_one_speaker)
    result = scoring.score_dataset("fixture-ds", "fixture", mode="online")
    assert result["conf"] > 0.0
    assert result["der"] == pytest.approx(result["conf"], abs=1e-6)


def test_scoring_refuses_to_attribute_a_run_to_the_wrong_mode(patched):
    _materialize_online(patched, identity)
    # The online run exists, but asking for offline must not silently score it
    # (nor find it: the offline directory is a different path).
    with pytest.raises(SystemExit, match="no offline hypothesis run"):
        scoring.score_dataset("fixture-ds", "fixture", mode="offline")

    # And a directory that records itself as offline must not be scored as
    # online even when it sits in the online location.
    import json

    (patched / "out" / "fixture-ds" / "online" / "fixture" / "run.json").write_text(
        json.dumps({"mode": "offline"}), encoding="utf-8"
    )
    with pytest.raises(SystemExit, match="refusing to score a offline run as online"):
        scoring.score_dataset("fixture-ds", "fixture", mode="online")


def test_online_missing_sidecar_fails_loudly(patched):
    hyp_dir = _materialize_online(patched, identity)
    (hyp_dir / "rec-b.events.jsonl").unlink()
    with pytest.raises(SystemExit, match="rec-b: no streaming event sidecar"):
        scoring.score_dataset("fixture-ds", "fixture", mode="online")
