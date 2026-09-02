"""Manual ingest for gated datasets (AMI, DIHARD-3)."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

from .archives import extract_archive
from .manifests import DatasetManifest, load_manifest
from .normalize import ffmpeg_normalize, wav_duration, _write_outputs
from .paths import CACHE_DIR, RAW_DIR


def _require_raw(manifest: DatasetManifest) -> list[Path]:
    raw_dir = RAW_DIR / manifest.name
    found = [p for p in raw_dir.glob("*") if p.is_file() and not p.name.startswith(".")]
    if not found:
        expected = ", ".join(manifest.raw_files) or "the archive"
        raise SystemExit(
            f"dataset '{manifest.name}' is gated: no raw archive found in {raw_dir}. "
            f"Expected file(s): {expected}. {manifest.raw_hint or ''}"
        )
    return found


def _meeting_id(path: Path) -> str | None:
    m = re.match(r"^(ES\d{4}|IBI\d{4}|EN\d{4}|ST\d{4}|IC\w+|IS\d{4})", path.name)
    return m.group(1) if m else None


def ingest_ami(manifest: DatasetManifest) -> None:
    """AMI audio (manual drop) + BUTSpeechFIT pyannote-fork RTTM/UEM lists.

    Variants: channel_policy 'array1:1' (SDM) or 'headset-mix'.
    """
    raw_dir = RAW_DIR / manifest.name
    archives = _require_raw(manifest)
    extracted = [extract_archive(a, raw_dir / "_extracted") for a in archives]

    # Reference: the AMI-diarization-setup repo is not gated; fetch if absent.
    ref_zip = CACHE_DIR / manifest.name / "AMI-diarization-setup.zip"
    if not ref_zip.is_file():
        from . import downloads

        ref_src = next(
            (s for s in manifest.sources if s.filename == "AMI-diarization-setup.zip"),
            None,
        )
        if ref_src is None:
            raise SystemExit(f"{manifest.name}: manifest lacks the reference repo source")
        downloads.fetch_http(ref_src, manifest.name)
    ref_root = extract_archive(ref_zip, CACHE_DIR / manifest.name / "_extracted")
    split = manifest.extra.get("split", "test")
    ref_rttm = next(ref_root.rglob(f"only_words/rttms/{split}.rttm"), None)
    ref_uem = next(ref_root.rglob(f"uems/{split}.uem"), None) or next(
        ref_root.rglob(f"only_words/uems/{split}.uem"), None
    )
    if ref_rttm is None or ref_uem is None:
        raise SystemExit(
            f"AMI setup repo archive lacks {split} rttm/uem — check the download"
        )

    uris_needed = {ln.split()[0] for ln in ref_uem.read_text(encoding="utf-8").splitlines()}

    policy = manifest.channel_policy
    wav_dir = _out_wav_dir(manifest)
    wav_dir.mkdir(parents=True, exist_ok=True)

    found: dict[str, list[Path]] = {}
    for root in extracted:
        for p in root.rglob("*"):
            if p.suffix.lower() not in {".flac", ".wav", ".ogg", ".opus"}:
                continue
            mid = _meeting_id(p)
            if mid and mid in uris_needed:
                if policy == "array1:1" and ".Array1-01." in p.name:
                    found.setdefault(mid, []).append(p)
                elif policy == "headset-mix" and ".Headset-" in p.name:
                    found.setdefault(mid, []).append(p)
    if not found:
        raise SystemExit(
            f"no {policy} audio files for the '{split}' split in {raw_dir} — "
            "check that the AMI archive contains amicorpus/*/audio/*.flac"
        )

    def to_wav(srcs: list[Path], dst: Path) -> None:
        if policy == "array1:1":
            (src,) = srcs[:1]
            subprocess.run(
                ["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-y",
                 "-i", str(src), "-map", "0:a:0", "-af", "pan=mono|c0=c0",
                 "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", str(dst)],
                check=True,
            )
        else:
            tmps = []
            for i, src in enumerate(sorted(srcs)):
                t = dst.parent / f".mix_{i}.wav"
                subprocess.run(
                    ["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-y",
                     "-i", str(src), "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", str(t)],
                    check=True,
                )
                tmps.append(t)
            inputs = []
            for t in tmps:
                inputs += ["-i", str(t)]
            filter_parts = "".join(f"[{i}:a]" for i in range(len(tmps)))
            subprocess.run(
                ["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-y",
                 *inputs,
                 "-filter_complex", f"{filter_parts}amix=inputs={len(tmps)}:duration=longest:normalize=0",
                 "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", str(dst)],
                check=True,
            )
            for t in tmps:
                t.unlink(missing_ok=True)

    rttm_lines = [
        ln + "\n"
        for ln in ref_rttm.read_text(encoding="utf-8").splitlines()
        if ln.split() and ln.split()[0] == "SPEAKER" and ln.split()[1] in uris_needed
    ]
    uem_lines: list[str] = []
    for uri in sorted(found):
        dst = wav_dir / f"{uri}.wav"
        to_wav(found[uri], dst)
        uem_lines.append(f"{uri} 1 0.0 {wav_duration(dst):.3f}\n")
    _write_outputs(manifest.name, rttm_lines, uem_lines)
    print(f"{manifest.name}: ingested {len(found)} meetings ({policy}, {split} split)")


def _out_wav_dir(manifest: DatasetManifest) -> Path:
    from .paths import DATA_DIR

    return DATA_DIR / manifest.name / "wav"


def ingest_dihard3(manifest: DatasetManifest) -> None:
    """DIHARD-3 (LDC2022S03): ships RTTM/UEM; audio decoded via ffmpeg."""
    raw_dir = RAW_DIR / manifest.name
    archives = _require_raw(manifest)
    extracted = [extract_archive(a, raw_dir / "_extracted") for a in archives]

    split = manifest.extra.get("split", "dev")  # eval split has no public RTTM
    rtts = [p for root in extracted for p in root.rglob("*.rttm") if split in str(p)]
    if not rtts:
        raise SystemExit(
            f"no {split} RTTM found inside the extracted archive(s) in {raw_dir}"
        )
    audio_files: dict[str, Path] = {}
    for root in extracted:
        for p in root.rglob("*"):
            if p.suffix.lower() in {".wav", ".flac", ".sph"}:
                audio_files.setdefault(p.stem, p)

    wav_dir = _out_wav_dir(manifest)
    wav_dir.mkdir(parents=True, exist_ok=True)
    rttm_lines: list[str] = []
    uem_lines: list[str] = []
    count = 0
    for rttm in rtts:
        for ln in rttm.read_text(encoding="utf-8").splitlines():
            f = ln.split()
            if len(f) < 8 or f[0] != "SPEAKER":
                continue
            uri = f[1]
            src = audio_files.get(uri)
            if src is None:
                continue
            dst = wav_dir / f"{uri}.wav"
            if not dst.is_file():
                ffmpeg_normalize(src, dst)
            rttm_lines.append(ln + "\n")
    scored = sorted({ln.split()[1] for ln in rttm_lines if ln.split()})
    uem_lines = [f"{uri} 1 0.0 {wav_duration(wav_dir / (uri + '.wav')):.3f}\n" for uri in scored]
    _write_outputs(manifest.name, rttm_lines, uem_lines)
    print(f"{manifest.name}: ingested {len(scored)} recordings ({split} split)")


INGESTERS = {
    "ami_but_speechfit": ingest_ami,
    "dihard3_rttm": ingest_dihard3,
}


def ingest_dataset(manifest: DatasetManifest) -> None:
    fn = INGESTERS.get(manifest.parser)
    if fn is None:
        raise SystemExit(
            f"dataset '{manifest.name}': parser '{manifest.parser}' is not a gated ingester"
        )
    fn(manifest)


def ingest(args) -> int:
    manifest = load_manifest(args.dataset)
    if not manifest.gated:
        print(f"note: '{manifest.name}' is not gated — ingest will still process eval/raw if present")
    ingest_dataset(manifest)
    return 0
