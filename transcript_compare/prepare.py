"""Cut reference fragments from <meeting>/audio.mp4 and write editable drafts into <meeting>/reference/.

usage: python prepare.py <meeting_dir> [--force]

<meeting_dir>/fragments.json:
  {"draft": "<segments json>",            # lines of the draft (and precise row times for editor.html)
   "hints": "<meetily transcript json>",  # optional: '#   whisper:' hint lines from its tokens
   "fragments": [{"id": 1, "start": 348.0, "end": 443.0, "channel": "System" | "Microphone", "what": "..."}]}
Segment files may be ASR runs ({start, end, source, text}) or Meetily transcripts
({audio_start_time, audio_end_time, source_device, text, tokens}).
Fragments whose ref_XX.txt already exists are skipped (hand edits are never overwritten) unless --force.
"""
import json, os, subprocess, sys

HEADER = """# Фрагмент {id}: {a}–{b}, канал: {ch_ru}. Аудио: {wav}
# Проверяет: {what}
#
# КАК ПРАВИТЬ:
#  - Исправляйте строки без '#', чтобы они совпадали с тем, что реально сказано.
#    Метки [мм:сс] можно оставить или удалить — при подсчёте они игнорируются.
#  - Пишите все слова, включая повторы и оборванные слова ("мы мы берём", "по-").
#    Междометия э / эм / мм можно не писать (при подсчёте они выкидываются).
#  - Английские термины пишите так, как их естественно написать: API, JSON, Claude Desktop, hotfix.
#    (При подсчёте регистр и пунктуация не важны.)
#  - Неразборчивое место пометьте как [неразб] — такие слова не будут учитываться.
#  - Строки '#   whisper:' — подсказка от другой модели, их не трогайте (игнорируются).
#  - Если в черновике нет реплики, которую слышно, — допишите её отдельной строкой.
"""


def mmss(t):
    return f"{int(t // 60):02d}:{int(t % 60):02d}"


def load_config(meeting):
    return json.load(open(os.path.join(meeting, "fragments.json"), encoding="utf-8"))


def load_segments(path):
    """Segments of an ASR run or a Meetily transcript as {start, end, source, text, tokens}."""
    out = []
    for s in json.load(open(path, encoding="utf-8"))["segments"]:
        out.append({"start": s.get("start", s.get("audio_start_time")), "end": s.get("end", s.get("audio_end_time")),
                    "source": s.get("source", s.get("source_device")), "text": s["text"],
                    "tokens": s.get("tokens") or []})
    return out


def main(meeting, force):
    cfg = load_config(meeting)
    ref = os.path.join(meeting, "reference")
    os.makedirs(ref, exist_ok=True)
    draft = load_segments(os.path.join(meeting, cfg["draft"]))
    hints = load_segments(os.path.join(meeting, cfg["hints"])) if cfg.get("hints") else []

    for f in cfg["fragments"]:
        fid, a, b, ch = f["id"], f["start"], f["end"], f["channel"]
        wav, txt = f"ref_{fid:02d}.mp3", os.path.join(ref, f"ref_{fid:02d}.txt")
        if os.path.exists(txt) and not force:
            print(fid, "skip: ref_%02d.txt exists" % fid)
            continue
        af = "pan=mono|c0=c0" if ch == "Microphone" else "pan=mono|c0=c1"
        if ch == "Microphone":
            af += ",loudnorm"
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", str(a), "-to", str(b),
                        "-i", os.path.join(meeting, "audio.mp4"), "-af", af, "-ar", "48000", "-b:a", "128k",
                        os.path.join(ref, wav)], check=True)

        tokens = [t for s in hints if s["source"] == ch for t in s["tokens"]]
        lines = [HEADER.format(id=fid, a=mmss(a), b=mmss(b), ch_ru="система" if ch == "System" else "микрофон",
                               wav=wav, what=f.get("what", ""))]
        for s in draft:
            if s["source"] != ch or not (a <= (s["start"] + s["end"]) / 2 <= b):
                continue
            lines.append(f"[{mmss(s['start'])}] {s['text'].strip()}")
            hint = "".join(t["text"] for t in tokens if s["start"] - 0.3 <= (t["start"] + t["end"]) / 2 <= s["end"] + 0.3)
            if hint.strip():
                lines.append(f"#   whisper: {hint.strip()}")
        open(txt, "w", encoding="utf-8").write("\n".join(lines) + "\n")
        print(fid, mmss(a), mmss(b), ch, len(lines) - 1, "lines")


if __name__ == "__main__":
    args = [x for x in sys.argv[1:] if x != "--force"]
    if len(args) != 1:
        raise SystemExit(__doc__)
    main(args[0], "--force" in sys.argv)
