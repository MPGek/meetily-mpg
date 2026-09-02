import textwrap
from pathlib import Path

import pytest

from diareval.manifests import ManifestError, load_manifest

SAMPLE = textwrap.dedent(
    """
    name: sample-dataset
    license: MIT
    parser: rttm_passthrough
    baseline_der: 11.3
    channel_policy: mono
    subset: true
    subset_files:
      - rec-a
      - rec-b
    sources:
      - kind: http
        url: https://example.org/sample.tar.gz
        sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
        filename: sample.tar.gz
    """
)


def write_manifest(tmp_path: Path, name: str, body: str) -> Path:
    path = tmp_path / f"{name}.yml"
    path.write_text(body, encoding="utf-8")
    return path


def test_parses_sample_manifest(tmp_path: Path) -> None:
    write_manifest(tmp_path, "sample-dataset", SAMPLE)
    m = load_manifest("sample-dataset", manifests_dir=tmp_path)
    assert m.name == "sample-dataset"
    assert m.license == "MIT"
    assert m.parser == "rttm_passthrough"
    assert m.baseline_der == pytest.approx(11.3)
    assert m.channel_policy == "mono"
    assert m.subset is True
    assert m.subset_files == ("rec-a", "rec-b")
    assert m.gated is False
    (src,) = m.sources
    assert src.kind == "http"
    assert src.sha256 is not None and len(src.sha256) == 64


def test_gated_requires_raw_files(tmp_path: Path) -> None:
    write_manifest(
        tmp_path,
        "gated-ds",
        "name: gated-ds\nlicense: Custom\ngated: true\nparser: rttm_passthrough\n",
    )
    with pytest.raises(ManifestError, match="raw_files"):
        load_manifest("gated-ds", manifests_dir=tmp_path)


def test_unknown_parser_rejected(tmp_path: Path) -> None:
    write_manifest(
        tmp_path,
        "bad-p",
        "name: bad-p\nlicense: MIT\nparser: magic_parser\n",
    )
    with pytest.raises(ManifestError, match="unknown parser"):
        load_manifest("bad-p", manifests_dir=tmp_path)


def test_name_must_match_stem(tmp_path: Path) -> None:
    write_manifest(tmp_path, "stem", "name: other\nlicense: MIT\nparser: rttm_passthrough\n")
    with pytest.raises(ManifestError, match="does not match"):
        load_manifest("stem", manifests_dir=tmp_path)


def test_missing_manifest_raises(tmp_path: Path) -> None:
    with pytest.raises(ManifestError, match="no manifest"):
        load_manifest("absent", manifests_dir=tmp_path)
