"""Subcommand implementations."""

from __future__ import annotations

import argparse

from ..manifests import load_manifest


def download(args: argparse.Namespace) -> int:
    from .. import downloads

    manifest = load_manifest(args.dataset)
    if manifest.gated:
        print(
            f"note: '{manifest.name}' is gated — download fetches only the ungated "
            f"reference sources; place the raw audio archive(s) {list(manifest.raw_files)} "
            f"under eval/raw/{manifest.name}/ and run `ingest --dataset {manifest.name}`"
        )
    if not manifest.sources:
        raise SystemExit(f"dataset '{manifest.name}' declares no download sources")
    print(f"Downloading {manifest.name} into eval/cache/{manifest.name}/")
    for src in manifest.sources:
        if src.kind == "http":
            downloads.fetch_http(src, manifest.name)
        elif src.kind == "gdrive":
            downloads.fetch_gdrive(src, manifest.name)
        elif src.kind == "hf":
            downloads.fetch_hf(src, manifest.name)
    print(f"{manifest.name}: cache up to date")
    return 0


def ingest(args: argparse.Namespace) -> int:
    from ..ingest import ingest_dataset

    ingest_dataset(load_manifest(args.dataset))
    return 0


def normalize(args: argparse.Namespace) -> int:
    from ..normalize import normalize_dataset

    normalize_dataset(load_manifest(args.dataset))
    return 0


def run(args: argparse.Namespace) -> int:
    from ..runner import run_dataset

    run_dataset(
        args.dataset,
        args.run_id,
        args.workers,
        args.force,
        harness_args=args.harness_arg,
    )
    return 0


def score(args: argparse.Namespace) -> int:
    from ..scoring import score_dataset

    score_dataset(args.dataset, args.run_id)
    return 0


def report(args: argparse.Namespace) -> int:
    from ..reporting import write_report

    write_report(args.datasets, args.run_id)
    return 0


def subset(args: argparse.Namespace) -> int:
    from ..subset import run_subset

    run_subset(force=args.force, harness_args=getattr(args, "harness_arg", None))
    return 0


def sweep(args: argparse.Namespace) -> int:
    from ..sweep import sweep as _sweep

    return _sweep(args)
