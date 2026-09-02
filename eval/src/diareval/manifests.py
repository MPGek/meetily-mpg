"""Dataset manifest schema and loader (one YAML per dataset in eval/manifests)."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from .paths import MANIFESTS_DIR

KNOWN_PARSERS = {
    "rttm_passthrough",
    "ru_synthetic_parquet",
    "label_studio_json",
    "ami_but_speechfit",
    "dihard3_rttm",
}

KNOWN_SOURCE_KINDS = {"http", "gdrive", "hf"}


class ManifestError(Exception):
    pass


@dataclass(frozen=True)
class Source:
    kind: str
    url: str
    sha256: str | None = None
    md5: str | None = None
    # for kind == "hf": the Hugging Face dataset id
    dataset_id: str | None = None
    # for kind == "gdrive": Google Drive file id
    file_id: str | None = None
    # for kind == "hf": filename patterns to fetch (default: everything)
    patterns: tuple[str, ...] = ()
    # target filename inside eval/cache/<dataset>/ (http/gdrive)
    filename: str | None = None


@dataclass(frozen=True)
class DatasetManifest:
    name: str
    license: str
    parser: str
    gated: bool = False
    raw_files: tuple[str, ...] = ()
    raw_hint: str | None = None
    sources: tuple[Source, ...] = ()
    channel_policy: str = "mono"
    baseline_der: float | None = None
    subset: bool = False
    subset_files: tuple[str, ...] = ()
    extra: dict[str, Any] = field(default_factory=dict)


def _parse_source(raw: Any, name: str) -> Source:
    if not isinstance(raw, dict) or "kind" not in raw:
        raise ManifestError(f"{name}: each source needs 'kind'")
    kind = raw["kind"]
    if kind not in KNOWN_SOURCE_KINDS:
        raise ManifestError(f"{name}: unknown source kind '{kind}'")
    url = raw.get("url", "")
    if kind == "http" and not url:
        raise ManifestError(f"{name}: http source needs 'url'")
    if kind == "gdrive" and not raw.get("file_id"):
        raise ManifestError(f"{name}: gdrive source needs 'file_id'")
    if kind == "hf" and not (raw.get("dataset_id") or url):
        raise ManifestError(f"{name}: hf source needs 'dataset_id'")
    return Source(
        kind=kind,
        url=url,
        sha256=raw.get("sha256"),
        md5=raw.get("md5"),
        dataset_id=raw.get("dataset_id"),
        file_id=raw.get("file_id"),
        patterns=tuple(raw.get("patterns", ())),
        filename=raw.get("filename"),
    )


def parse_manifest(data: Any, name: str) -> DatasetManifest:
    if not isinstance(data, dict):
        raise ManifestError(f"{name}: manifest must be a mapping")
    for required in ("name", "license", "parser"):
        if required not in data:
            raise ManifestError(f"{name}: missing required field '{required}'")
    if data["name"] != name:
        raise ManifestError(f"{name}: manifest name '{data['name']}' does not match file stem")
    parser = data["parser"]
    if parser not in KNOWN_PARSERS:
        raise ManifestError(f"{name}: unknown parser '{parser}' (known: {sorted(KNOWN_PARSERS)})")
    gated = bool(data.get("gated", False))
    raw_files = tuple(data.get("raw_files", ()))
    if gated and not raw_files:
        raise ManifestError(f"{name}: gated manifest must declare expected raw_files")
    baseline = data.get("baseline_der")
    if baseline is not None:
        try:
            baseline = float(baseline)
        except (TypeError, ValueError) as exc:
            raise ManifestError(f"{name}: baseline_der must be a number") from exc
    return DatasetManifest(
        name=data["name"],
        license=str(data["license"]),
        parser=parser,
        gated=gated,
        raw_files=raw_files,
        raw_hint=data.get("raw_hint"),
        sources=tuple(_parse_source(s, name) for s in data.get("sources", ())),
        channel_policy=str(data.get("channel_policy", "mono")),
        baseline_der=baseline,
        subset=bool(data.get("subset", False)),
        subset_files=tuple(data.get("subset_files", ())),
        extra={k: v for k, v in data.items() if k not in {
            "name", "license", "parser", "gated", "raw_files", "raw_hint",
            "sources", "channel_policy", "baseline_der", "subset", "subset_files",
        }},
    )


def load_manifest(name: str, manifests_dir: Path | None = None) -> DatasetManifest:
    path = (manifests_dir or MANIFESTS_DIR) / f"{name}.yml"
    if not path.is_file():
        raise ManifestError(f"no manifest for dataset '{name}' (expected {path})")
    try:
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
    except yaml.YAMLError as exc:
        raise ManifestError(f"{name}: invalid YAML: {exc}") from exc
    return parse_manifest(data, name)


def list_manifests(manifests_dir: Path | None = None) -> list[DatasetManifest]:
    directory = manifests_dir or MANIFESTS_DIR
    return sorted(
        (load_manifest(p.stem, directory) for p in directory.glob("*.yml")),
        key=lambda m: m.name,
    )
