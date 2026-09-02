"""Archive extraction helpers for gated ingest."""

from __future__ import annotations

import tarfile
import zipfile
from pathlib import Path


def extract_archive(archive: Path, dest: Path) -> Path:
    """Extract zip/tar.gz/tgz/tar into dest/<stem>/ (idempotent)."""
    marker = dest / ".extracted"
    target = dest / archive.stem
    if marker.is_file() and target.is_dir():
        return target
    target.mkdir(parents=True, exist_ok=True)
    name = archive.name.lower()
    if name.endswith(".zip"):
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(target)
    elif name.endswith((".tar.gz", ".tgz", ".tar")):
        with tarfile.open(archive) as tf:
            tf.extractall(target, filter="data")
    else:
        raise SystemExit(f"unsupported archive format: {archive.name}")
    marker.write_text(str(archive), encoding="utf-8")
    return target
