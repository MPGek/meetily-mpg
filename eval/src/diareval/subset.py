"""Fast regression subset: ≤10 recordings, one command, per-source DER."""

from __future__ import annotations

from .manifests import list_manifests
from .paths import DATA_DIR


def subset_datasets() -> list:
    return [m for m in list_manifests() if m.subset and m.subset_files]


def prepare_once(manifest) -> None:
    """Download + normalize a subset dataset if not materialized yet."""
    wav_dir = DATA_DIR / manifest.name / "wav"
    if wav_dir.is_dir() and any(wav_dir.glob("*.wav")):
        return
    from . import downloads
    from .normalize import normalize_dataset

    for src in manifest.sources:
        if src.kind == "http":
            downloads.fetch_http(src, manifest.name)
        elif src.kind == "gdrive":
            downloads.fetch_gdrive(src, manifest.name)
        elif src.kind == "hf":
            downloads.fetch_hf(src, manifest.name)
    normalize_dataset(manifest)


def run_subset(force: bool = False, run_id: str = "subset") -> dict:
    manifests = subset_datasets()
    if not manifests:
        raise SystemExit(
            "no subset datasets — mark a manifest with `subset: true` and "
            "`subset_files: [...]`"
        )
    total_files = sum(len(m.subset_files) for m in manifests)
    if total_files > 10:
        raise SystemExit(
            f"subset declares {total_files} files across {len(manifests)} datasets "
            "(spec: at most 10)"
        )

    from .runner import run_dataset
    from .scoring import score_dataset

    results = {}
    for m in manifests:
        prepare_once(m)
        run_dataset(m.name, run_id=run_id, force=force, files=list(m.subset_files))
        results[m.name] = score_dataset(m.name, run_id=run_id, files=list(m.subset_files))

    print("\nsubset results:")
    for name, r in results.items():
        print(
            f"  {name}: DER {r['der']:.2f}% "
            f"(FA {r['fa']:.2f} / Miss {r['miss']:.2f} / Conf {r['conf']:.2f})"
        )
    return results
