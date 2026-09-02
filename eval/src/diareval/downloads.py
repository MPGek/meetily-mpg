"""Checksum-verified, idempotent fetchers for eval/cache/."""

from __future__ import annotations

import fnmatch
import hashlib
import json
import shutil
from pathlib import Path

import requests
from tqdm import tqdm

from .manifests import Source
from .paths import CACHE_DIR

CHUNK = 1024 * 1024


def file_hash(path: Path, algo: str) -> str:
    h = hashlib.new(algo)
    with path.open("rb") as f:
        for block in iter(lambda: f.read(CHUNK), b""):
            h.update(block)
    return h.hexdigest()


def _checksum_ok(path: Path, src: Source) -> bool:
    if src.sha256 and file_hash(path, "sha256") != src.sha256.lower():
        return False
    if src.md5 and file_hash(path, "md5") != src.md5.lower():
        return False
    if not src.sha256 and not src.md5:
        # No manifest checksum: fall back to the sidecar recorded on first fetch.
        side = _sidecar_hash(path)
        if side is not None and file_hash(path, "sha256") != side:
            return False
    return True


def _record_sidecar_hash(path: Path) -> None:
    (path.with_suffix(path.suffix + ".sha256")).write_text(
        file_hash(path, "sha256"), encoding="ascii"
    )


def _sidecar_hash(path: Path) -> str | None:
    side = path.with_suffix(path.suffix + ".sha256")
    return side.read_text(encoding="ascii").strip() if side.exists() else None


def target_path(src: Source, dataset: str) -> Path:
    name = src.filename or src.url.rsplit("/", 1)[-1]
    return CACHE_DIR / dataset / name


def fetch_http(src: Source, dataset: str) -> Path:
    """Idempotent single-file HTTP download with checksum verification."""
    target = target_path(src, dataset)
    if target.is_file():
        if _checksum_ok(target, src):
            print(f"  [skip] {src.url} (cached, checksum verified)")
            return target
        print(f"  [re-download] {target.name}: checksum mismatch on cached file")
        target.unlink()
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_suffix(target.suffix + ".part")
    print(f"  [get] {src.url} -> {target.relative_to(CACHE_DIR.parent)}")
    with requests.get(src.url, stream=True, timeout=60) as resp:
        resp.raise_for_status()
        total = int(resp.headers.get("content-length", 0)) or None
        with tmp.open("wb") as f, tqdm(
            total=total, unit="B", unit_scale=True, unit_divisor=1024, desc=target.name
        ) as bar:
            for chunk in resp.iter_content(chunk_size=4 * CHUNK):
                f.write(chunk)
                bar.update(len(chunk))
    if not _checksum_ok(tmp, src):
        tmp.unlink(missing_ok=True)
        raise RuntimeError(f"checksum verification failed for {src.url}")
    tmp.replace(target)
    if not src.sha256:
        _record_sidecar_hash(target)
        print(f"  [note] manifest has no sha256; recorded {target.name}.sha256 — copy into the manifest")
    return target


def fetch_gdrive(src: Source, dataset: str) -> Path:
    """Google Drive fetch (used for MSDWild audio)."""
    import gdown

    target = target_path(src, dataset)
    if target.is_file():
        side = _sidecar_hash(target)
        if _checksum_ok(target, src) and (src.sha256 or side):
            print(f"  [skip] gdrive:{src.file_id} (cached, checksum verified)")
            return target
        target.unlink()
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"  [get] https://drive.google.com/file/d/{src.file_id} -> {target.name}")
    gdown.download(id=src.file_id, output=str(target), quiet=False, use_cookies=False)
    if not target.is_file():
        raise RuntimeError(f"gdown failed to download gdrive file {src.file_id}")
    if not _checksum_ok(target, src):
        raise RuntimeError(f"checksum verification failed for gdrive:{src.file_id}")
    if not src.sha256:
        _record_sidecar_hash(target)
    return target


def fetch_hf(src: Source, dataset: str) -> Path:
    """Download matching files from a Hugging Face dataset repo into the cache."""
    from huggingface_hub import hf_hub_download, list_repo_files

    dest = CACHE_DIR / dataset / "hf"
    patterns = src.patterns or ("*",)
    repo_files = [
        f
        for f in list_repo_files(src.dataset_id or src.url, repo_type="dataset")
        if any(fnmatch.fnmatch(f, p) for p in patterns)
    ]
    if not repo_files:
        raise RuntimeError(f"no files matching {patterns} in HF dataset {src.dataset_id}")
    marker = dest / ".fetched.json"
    done: dict[str, str] = {}
    if marker.is_file():
        done = json.loads(marker.read_text(encoding="utf-8"))
    fetched: dict[str, str] = {}
    for f in repo_files:
        local = Path(
            hf_hub_download(
                src.dataset_id or src.url,
                f,
                repo_type="dataset",
                local_dir=str(dest),
            )
        )
        digest = file_hash(local, "sha256")
        if done.get(f) == digest:
            print(f"  [skip] hf://{src.dataset_id}/{f} (cached, checksum verified)")
        else:
            print(f"  [get] hf://{src.dataset_id}/{f}")
        fetched[f] = digest
    marker.write_text(json.dumps(fetched, indent=1), encoding="utf-8")
    return dest
