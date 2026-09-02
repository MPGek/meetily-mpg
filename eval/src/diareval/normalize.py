"""Normalize cached/raw sources into the canonical wav/ + rttm/ + uem/ layout."""

from __future__ import annotations

import json
import shutil
import subprocess
import zipfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from .manifests import DatasetManifest, load_manifest
from .paths import CACHE_DIR, DATA_DIR

TARGET_SR = 16000


def ffmpeg_normalize(src: Path, dst: Path) -> None:
    """Decode any source format to 16 kHz mono PCM-16 WAV."""
    dst.parent.mkdir(parents=True, exist_ok=True)
    tmp = dst.parent / f"{dst.stem}.part.wav"
    subprocess.run(
        [
            "ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-y",
            "-i", str(src), "-vn", "-ac", "1", "-ar", str(TARGET_SR),
            "-sample_fmt", "s16", str(tmp),
        ],
        check=True,
    )
    tmp.replace(dst)


def wav_duration(path: Path) -> float:
    import soundfile as sf

    return float(sf.info(str(path)).duration)


def _cache(dataset: str) -> Path:
    return CACHE_DIR / dataset


def _out(dataset: str) -> Path:
    return DATA_DIR / dataset


def _write_outputs(
    dataset: str,
    rttm_lines: list[str],
    uem_lines: list[str],
) -> None:
    out = _out(dataset)
    (out / "rttm").mkdir(parents=True, exist_ok=True)
    (out / "uem").mkdir(parents=True, exist_ok=True)
    (out / "rttm" / "ref.rttm").write_text("".join(rttm_lines), encoding="utf-8")
    (out / "uem" / "ref.uem").write_text("".join(uem_lines), encoding="utf-8")


def _extract_zip_member(zpath: Path, member: str, dest_dir: Path) -> Path:
    dest = dest_dir / Path(member).name
    dest.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(zpath) as zf:
        with zf.open(member) as src, dest.open("wb") as dst:
            shutil.copyfileobj(src, dst, 4 * 1024 * 1024)
    return dest


def normalize_rttm_passthrough(manifest: DatasetManifest) -> None:
    """VoxConverse / MSDWild: audio archive + reference RTTM, ffmpeg-normalized."""
    cfg = manifest.extra.get("normalize", {})
    cache = _cache(manifest.name)
    out = _out(manifest.name)
    wav_dir = out / "wav"
    wav_dir.mkdir(parents=True, exist_ok=True)

    audio_zip = cache / cfg["audio_archive"]
    tmp_dir = cache / "_extract_tmp"
    if tmp_dir.exists():
        shutil.rmtree(tmp_dir)
    with zipfile.ZipFile(audio_zip) as zf:
        members = [
            m for m in zf.namelist()
            if Path(m).suffix.lower() in {".wav", ".flac", ".mp3", ".ogg", ".opus"}
            and not m.endswith("/")
            and not Path(m).name.startswith("._")
            and "__MACOSX" not in m
        ]
    if cfg.get("audio_glob"):
        from fnmatch import fnmatch

        members = [m for m in members if fnmatch(m, cfg["audio_glob"])]
    print(f"{manifest.name}: {len(members)} source recordings")

    uris: dict[str, Path] = {}
    for m in members:
        uri = Path(m).stem
        uris[uri] = _extract_zip_member(audio_zip, m, tmp_dir)

    def convert(item: tuple[str, Path]) -> tuple[str, float]:
        uri, path = item
        dst = wav_dir / f"{uri}.wav"
        ffmpeg_normalize(path, dst)
        return uri, wav_duration(dst)

    durations: dict[str, float] = {}
    with ThreadPoolExecutor(max_workers=min(8, (shutil.os.cpu_count() or 2))) as pool:
        for uri, dur in pool.map(convert, sorted(uris.items())):
            durations[uri] = dur
    shutil.rmtree(tmp_dir, ignore_errors=True)

    # Reference turns.
    rttm_lines: list[str] = []
    if cfg.get("rttm_archive") and cfg.get("rttm_glob"):
        from fnmatch import fnmatch

        rttm_zip = cache / cfg["rttm_archive"]
        with zipfile.ZipFile(rttm_zip) as zf:
            names = [n for n in zf.namelist() if fnmatch(n, cfg["rttm_glob"])]
        tmp = cache / "_rttm_tmp"
        for n in names:
            p = _extract_zip_member(rttm_zip, n, tmp)
            rttm_lines.extend(p.read_text(encoding="utf-8").splitlines(keepends=True))
        shutil.rmtree(tmp, ignore_errors=True)
    elif cfg.get("rttm_file"):
        rttm_lines.extend(
            (cache / cfg["rttm_file"]).read_text(encoding="utf-8").splitlines(keepends=True)
        )
    else:
        raise SystemExit(f"{manifest.name}: normalize config missing rttm source")

    # Keep only turns for recordings we materialized; rebuild UEM from durations.
    kept = [ln for ln in rttm_lines if ln.split()[0] == "SPEAKER"]
    uri_durs = {uri: durations[uri] for uri in durations}
    present = set(uri_durs)
    filtered = [ln for ln in kept if ln.split()[1] in present]
    uem_lines = [f"{uri} 1 0.0 {d:.3f}\n" for uri, d in sorted(uri_durs.items())]
    _write_outputs(manifest.name, filtered, uem_lines)
    print(
        f"{manifest.name}: wrote {len(uri_durs)} wavs, {len(filtered)} reference turns"
    )


def normalize_ru_synthetic(manifest: DatasetManifest) -> None:
    """niobures/synthetic-speech-diarization-ru: parquet audio + speakers[]."""
    import pyarrow.parquet as pq
    import soundfile as sf

    hf_dir = _cache(manifest.name) / "hf"
    out = _out(manifest.name)
    wav_dir = out / "wav"
    wav_dir.mkdir(parents=True, exist_ok=True)

    parquets = sorted(hf_dir.glob("train-*.parquet"))
    if not parquets:
        raise SystemExit(f"{manifest.name}: no parquet files under {hf_dir}")
    rttm_lines: list[str] = []
    uem_lines: list[str] = []
    count = 0
    for pf in parquets:
        table = pq.read_table(pf, columns=["audio", "sampling_rate", "speakers"])
        for row_idx in range(table.num_rows):
            audio = table.column("audio")[row_idx].as_py()
            sr = int(table.column("sampling_rate")[row_idx].as_py())
            speakers = table.column("speakers")[row_idx].as_py()
            samples = audio["array"] if isinstance(audio, dict) else audio
            uri = f"{pf.stem}_{row_idx:04d}"
            import numpy as np

            data = np.asarray(samples, dtype=np.float32)
            sf.write(wav_dir / f"{uri}.wav", data, sr, subtype="PCM_16")
            for seg in speakers or []:
                start, end = float(seg["start"]), float(seg["end"])
                if end <= start:
                    continue
                rttm_lines.append(
                    f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} "
                    f"<NA> <NA> SPEAKER_{int(seg['speaker_id']):02d} <NA> <NA>\n"
                )
            uem_lines.append(f"{uri} 1 0.0 {len(data) / sr:.3f}\n")
            count += 1
        print(f"{manifest.name}: {pf.name} ({count} tracks so far)")
    _write_outputs(manifest.name, rttm_lines, uem_lines)
    print(f"{manifest.name}: wrote {count} wavs, {len(rttm_lines)} reference turns")


def normalize_ru_youtube(manifest: DatasetManifest) -> None:
    """leshinsky/ru-youtube-diarization: Label Studio JSON -> RTTM + UEM."""
    hf_dir = _cache(manifest.name) / "hf"
    out = _out(manifest.name)
    wav_dir = out / "wav"
    wav_dir.mkdir(parents=True, exist_ok=True)

    jsons = [
        p
        for p in sorted(hf_dir.glob("*.json"))
        if p.name != ".fetched.json"
    ]
    if not jsons:
        raise SystemExit(f"{manifest.name}: no annotation JSONs under {hf_dir}")

    turns: dict[str, list[tuple[float, float, str]]] = {}
    sources: dict[str, Path] = {}
    for jf in jsons:
        data = json.loads(jf.read_text(encoding="utf-8"))
        tasks = data if isinstance(data, list) else data.get("tasks", [])
        for task in tasks:
            audio_ref = (task.get("data") or {}).get("audio")
            if not audio_ref:
                continue
            src = (hf_dir / audio_ref).resolve()
            if not src.is_file():
                raise SystemExit(f"{manifest.name}: {jf.name} references missing {audio_ref}")
            uri = src.stem
            sources.setdefault(uri, src)
            annotations = task.get("annotations") or []
            if not annotations:
                continue
            for res in annotations[-1].get("result", []):
                if res.get("from_name") != "labels":
                    continue
                value = res.get("value", {})
                labels = value.get("labels") or []
                if not labels:
                    continue
                start, end = float(value["start"]), float(value["end"])
                if end <= start:
                    continue
                turns.setdefault(uri, []).append((start, end, str(labels[0])))

    def convert(item: tuple[str, Path]) -> tuple[str, float]:
        uri, src = item
        ffmpeg_normalize(src, wav_dir / f"{uri}.wav")
        return uri, wav_duration(wav_dir / f"{uri}.wav")

    durations: dict[str, float] = {}
    with ThreadPoolExecutor(max_workers=min(8, (shutil.os.cpu_count() or 2))) as pool:
        for uri, dur in pool.map(convert, sorted(sources.items())):
            durations[uri] = dur

    rttm_lines: list[str] = []
    uem_lines: list[str] = []
    for uri in sorted(durations):
        for start, end, label in sorted(turns.get(uri, [])):
            spk = label.lower().replace(" ", "_")
            rttm_lines.append(
                f"SPEAKER {uri} 1 {start:.3f} {end - start:.3f} <NA> <NA> {spk} <NA> <NA>\n"
            )
        uem_lines.append(f"{uri} 1 0.0 {durations[uri]:.3f}\n")
    _write_outputs(manifest.name, rttm_lines, uem_lines)
    print(
        f"{manifest.name}: wrote {len(durations)} wavs, {len(rttm_lines)} reference turns"
    )


NORMALIZERS = {
    "rttm_passthrough": normalize_rttm_passthrough,
    "ru_synthetic_parquet": normalize_ru_synthetic,
    "label_studio_json": normalize_ru_youtube,
}


def normalize_dataset(manifest: DatasetManifest) -> None:
    fn = NORMALIZERS.get(manifest.parser)
    if fn is None:
        raise SystemExit(
            f"dataset '{manifest.name}': parser '{manifest.parser}' has no normalizer"
        )
    fn(manifest)


def normalize(args) -> int:
    manifest = load_manifest(args.dataset)
    normalize_dataset(manifest)
    return 0
