"""argparse CLI exposing the evaluation subcommands."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence

from . import __version__


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="diareval",
        description="Meetily diarization evaluation harness",
    )
    parser.add_argument("--version", action="version", version=f"diareval {__version__}")
    sub = parser.add_subparsers(dest="command", required=True)

    download = sub.add_parser("download", help="Download open datasets into eval/cache")
    download.add_argument("--dataset", required=True, help="Dataset name (see eval/manifests)")
    download.set_defaults(func=_dispatch("download"))

    ingest = sub.add_parser("ingest", help="Ingest a manually placed gated archive")
    ingest.add_argument("--dataset", required=True, help="Gated dataset name (see eval/manifests)")
    ingest.set_defaults(func=_dispatch("ingest"))

    normalize = sub.add_parser("normalize", help="Normalize cached/raw data to canonical layout")
    normalize.add_argument("--dataset", required=True, help="Dataset name (see eval/manifests)")
    normalize.set_defaults(func=_dispatch("normalize"))

    run = sub.add_parser("run", help="Run the diarize-eval harness over a dataset")
    run.add_argument("--dataset", required=True, help="Dataset name (see eval/data)")
    run.add_argument("--run-id", default="latest", help="Output run directory name under eval/out")
    run.add_argument("--workers", type=int, default=None, help="Parallel workers (default 4)")
    run.add_argument("--force", action="store_true", help="Reprocess files that already have outputs")
    run.add_argument(
        "--harness-arg",
        action="append",
        default=None,
        metavar="ARG",
        help="Extra argument appended to each diarize-eval invocation (repeatable), "
        "e.g. --harness-arg --cluster-threshold=0.35",
    )
    run.set_defaults(func=_dispatch("run"))

    score = sub.add_parser("score", help="Score hypothesis RTTMs against references (DER)")
    score.add_argument("--dataset", required=True, help="Dataset name")
    score.add_argument("--run-id", default="latest", help="Run directory name under eval/out")
    score.set_defaults(func=_dispatch("score"))

    report = sub.add_parser("report", help="Write the Markdown comparison report")
    report.add_argument("--datasets", nargs="+", default=None, help="Datasets to include (default: all scored)")
    report.add_argument("--run-id", default="latest", help="Run directory name under eval/out")
    report.set_defaults(func=_dispatch("report"))

    subset = sub.add_parser("subset", help="Run the fast regression subset end-to-end")
    subset.add_argument("--force", action="store_true", help="Reprocess subset files with existing outputs")
    subset.add_argument(
        "--harness-arg",
        action="append",
        default=None,
        metavar="ARG",
        help="Extra argument appended to each diarize-eval invocation (repeatable), "
        "e.g. --harness-arg --cluster-threshold=0.9 (gate testing)",
    )
    subset.set_defaults(func=_dispatch("subset"))

    sweep = sub.add_parser(
        "sweep",
        help="Grid-sweep clustering parameters over tuning datasets (diarization-param-tuning D5)",
    )
    sweep.add_argument(
        "--dataset",
        action="append",
        default=None,
        help="Tuning dataset (repeatable; default: all manifests marked `tuning: true`)",
    )
    sweep.add_argument(
        "--grid",
        action="append",
        required=True,
        metavar="PARAM=V1,V2,...",
        help="Grid axis; PARAM in {cluster_threshold, cluster_ceiling, gap_merge_secs} (repeatable)",
    )
    sweep.add_argument("--run-prefix", default="sweep", help="Run-id prefix for per-candidate runs")
    sweep.add_argument("--workers", type=int, default=None, help="Parallel workers per run (default 4)")
    sweep.add_argument(
        "--files",
        action="append",
        default=None,
        metavar="DATASET=f1,f2",
        help="Restrict a dataset to specific recordings (repeatable)",
    )
    sweep.add_argument(
        "--validation",
        action="append",
        default=None,
        help="Held-out validation dataset (repeatable; default: voxconverse, msdwild)",
    )
    sweep.add_argument(
        "--validate-top",
        type=int,
        default=2,
        help="Evaluate the top-N tuning candidates on held-out validation sets (default 2)",
    )
    sweep.add_argument(
        "--report",
        default=None,
        metavar="PATH",
        help="Report output path (default: eval/reports/sweep-<date>-<gitrev>.md)",
    )
    sweep.set_defaults(func=_dispatch("sweep"))

    return parser


def _dispatch(name: str):
    from . import commands

    def _call(args: argparse.Namespace) -> int:
        return getattr(commands, name)(args)

    return _call


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


def download_cli() -> None:
    sys.exit(main(["download", *sys.argv[1:]]))


def ingest_cli() -> None:
    sys.exit(main(["ingest", *sys.argv[1:]]))


def normalize_cli() -> None:
    sys.exit(main(["normalize", *sys.argv[1:]]))


def run_cli() -> None:
    sys.exit(main(["run", *sys.argv[1:]]))


def score_cli() -> None:
    sys.exit(main(["score", *sys.argv[1:]]))


def report_cli() -> None:
    sys.exit(main(["report", *sys.argv[1:]]))


def subset_cli() -> None:
    sys.exit(main(["subset", *sys.argv[1:]]))
