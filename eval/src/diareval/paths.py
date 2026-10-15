"""Canonical paths for the evaluation harness (all relative to ``eval/``)."""

from pathlib import Path

EVAL_ROOT = Path(__file__).resolve().parents[2]
MANIFESTS_DIR = EVAL_ROOT / "manifests"
CACHE_DIR = EVAL_ROOT / "cache"
RAW_DIR = EVAL_ROOT / "raw"
DATA_DIR = EVAL_ROOT / "data"
OUT_DIR = EVAL_ROOT / "out"
REPORTS_DIR = EVAL_ROOT / "reports"
