"""Synthetic-fixture tests for gated ingest (AMI, DIHARD-3).

Real gated archives cannot be committed; these fixtures validate the ingest
logic (channel selection/mix, RTTM/UEM materialization) and the absent-archive
failure mode. Full verification still requires a user-supplied archive drop.
"""

from __future__ import annotations

import tarfile
import wave
import zipfile
from pathlib import Path

import numpy as np
import pytest
from pyannote.database.util import load_rttm

from diareval import ingest, normalize
from diareval.manifests import DatasetManifest


def _tone_wav(path: Path, seconds: float = 2.0, sr: int = 22050, ch: int = 2) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    n = int(sr * seconds)
    data = (np.sin(2 * np.pi * 220 * np.arange(n) / sr) * 12000).astype("<i2")
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(ch)
        wf.setsampwidth(2)
        wf.setframerate(sr)
        for i in range(n):
            frame = (data[i].tobytes() * ch) if ch > 1 else data[i].tobytes()
            wf.writeframesraw(frame)


AMI_RTTM = (
    "SPEAKER ES2002a 1 0.500 1.200 <NA> <NA> A <NA> <NA>\n"
    "SPEAKER ES2002a 1 1.000 0.800 <NA> <NA> B <NA> <NA>\n"
    "SPEAKER ES2003a 1 0.200 1.500 <NA> <NA> C <NA> <NA>\n"
    # ES9999a is absent from the UEM: its turns must be filtered out.
    "SPEAKER ES9999a 1 0.200 1.500 <NA> <NA> Z <NA> <NA>\n"
)
AMI_UEM = (
    "ES2002a 1 0.000 2.000\n"
    "ES2003a 1 0.000 2.000\n"
)


def _ami_manifest(name: str, policy: str) -> DatasetManifest:
    return DatasetManifest(
        name=name,
        license="CC BY 4.0",
        parser="ami_but_speechfit",
        gated=True,
        raw_files=(f"{name}-audio.zip",),
        raw_hint="synthetic test",
        channel_policy=policy,
        extra={"split": "test"},
    )


@pytest.fixture()
def patched(tmp_path: Path, monkeypatch):
    raw = tmp_path / "raw"
    cache = tmp_path / "cache"
    data = tmp_path / "data"
    from diareval import paths

    monkeypatch.setattr(paths, "RAW_DIR", raw)
    monkeypatch.setattr(paths, "CACHE_DIR", cache)
    monkeypatch.setattr(paths, "DATA_DIR", data)
    monkeypatch.setattr(ingest, "RAW_DIR", raw)
    monkeypatch.setattr(ingest, "CACHE_DIR", cache)
    monkeypatch.setattr(normalize, "DATA_DIR", data)
    return tmp_path


def _make_ami_setup_zip(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr("AMI-diarization-setup-master/only_words/rttms/test.rttm", AMI_RTTM)
        zf.writestr("AMI-diarization-setup-master/only_words/uems/test.uem", AMI_UEM)


def _check_canonical(data_dir: Path, dataset: str, uris: set[str]) -> None:
    out = data_dir / dataset
    for uri in uris:
        wav = out / "wav" / f"{uri}.wav"
        assert wav.is_file(), wav
        with wave.open(str(wav)) as wf:
            assert wf.getnchannels() == 1
            assert wf.getframerate() == 16000
    ref = dict(load_rttm(str(out / "rttm" / "ref.rttm")))
    uem_lines = (out / "uem" / "ref.uem").read_text(encoding="utf-8").splitlines()
    assert {ln.split()[0] for ln in uem_lines} == uris
    assert set(ref) == uris
    labels = {lbl for ann in ref.values() for lbl in ann.labels()}
    assert labels <= {"A", "B", "C"}


def test_ami_sdm_ingest(patched):
    tmp = patched
    raw_dir = tmp / "raw" / "ami-sdm"
    for meeting in ("ES2002a", "ES2003a"):
        _tone_wav(raw_dir / "src" / f"{meeting}.Array1-01.wav")
        _tone_wav(raw_dir / "src" / f"{meeting}.Headset-0.wav")  # must be ignored
    audio_zip = raw_dir / "ami-sdm-audio.zip"
    with zipfile.ZipFile(audio_zip, "w") as zf:
        for p in (raw_dir / "src").iterdir():
            zf.write(p, f"amicorpus/{p.parent.name}/{p.name}")
    _make_ami_setup_zip(tmp / "cache" / "ami-sdm" / "AMI-diarization-setup.zip")

    ingest.ingest_ami(_ami_manifest("ami-sdm", "array1:1"))
    _check_canonical(tmp / "data", "ami-sdm", {"ES2002a", "ES2003a"})


def test_ami_headset_mix_ingest(patched):
    tmp = patched
    raw_dir = tmp / "raw" / "ami-headset"
    for meeting in ("ES2002a", "ES2003a"):
        for hs in range(4):
            _tone_wav(raw_dir / "src" / f"{meeting}.Headset-{hs}.wav", seconds=1.5)
        _tone_wav(raw_dir / "src" / f"{meeting}.Array1-01.wav")  # must be ignored
    audio_zip = raw_dir / "ami-headset-audio.zip"
    with zipfile.ZipFile(audio_zip, "w") as zf:
        for p in (raw_dir / "src").iterdir():
            zf.write(p, f"amicorpus/{p.parent.name}/{p.name}")
    _make_ami_setup_zip(tmp / "cache" / "ami-headset" / "AMI-diarization-setup.zip")

    ingest.ingest_ami(_ami_manifest("ami-headset", "headset-mix"))
    _check_canonical(tmp / "data", "ami-headset", {"ES2002a", "ES2003a"})


def test_ami_missing_archive_fails_with_hint(patched):
    manifest = _ami_manifest("ami-sdm", "array1:1")
    with pytest.raises(SystemExit, match="ami-sdm-audio.zip"):
        ingest.ingest_ami(manifest)


def test_dihard3_missing_archive_fails_with_hint(patched):
    manifest = DatasetManifest(
        name="dihard3",
        license="LDC",
        parser="dihard3_rttm",
        gated=True,
        raw_files=("LDCCeRC2022S03.tar.gz",),
        raw_hint="LDC account",
    )
    with pytest.raises(SystemExit, match="LDCCeRC2022S03.tar.gz"):
        ingest.ingest_dihard3(manifest)


def test_dihard3_ingest(patched):
    tmp = patched
    raw_dir = tmp / "raw" / "dihard3"
    src = raw_dir / "stage" / "LDC2022S03" / "dev"
    rttm_text = (
        "SPEAKER dihard3-dev-r1 1 0.100 1.000 <NA> <NA> A <NA> <NA>\n"
        "SPEAKER dihard3-dev-r1 1 0.600 0.900 <NA> <NA> B <NA> <NA>\n"
        "SPEAKER dihard3-dev-r2 1 0.000 1.500 <NA> <NA> C <NA> <NA>\n"
    )
    for rec in ("dihard3-dev-r1", "dihard3-dev-r2"):
        _tone_wav(src / f"{rec}.wav", seconds=2.0, sr=8000, ch=1)
    (src / "rttm").mkdir(parents=True, exist_ok=True)
    (src / "rttm" / "dev-all.rttm").write_text(rttm_text, encoding="utf-8")
    archive = raw_dir / "LDCCeRC2022S03.tar.gz"
    with tarfile.open(archive, "w:gz") as tf:
        tf.add(src / "rttm" / "dev-all.rttm", arcname="LDC2022S03/dev/rttm/dev-all.rttm")
        for rec in ("dihard3-dev-r1", "dihard3-dev-r2"):
            tf.add(src / f"{rec}.wav", arcname=f"LDC2022S03/dev/{rec}.wav")

    manifest = DatasetManifest(
        name="dihard3",
        license="LDC",
        parser="dihard3_rttm",
        gated=True,
        raw_files=("LDCCeRC2022S03.tar.gz",),
        raw_hint="LDC account",
        extra={"split": "dev"},
    )
    ingest.ingest_dihard3(manifest)
    _check_canonical(tmp / "data", "dihard3", {"dihard3-dev-r1", "dihard3-dev-r2"})
    ref_text = (tmp / "data" / "dihard3" / "rttm" / "ref.rttm").read_text(encoding="utf-8")
    assert ref_text.count("SPEAKER dihard3-dev-r1") == 2
