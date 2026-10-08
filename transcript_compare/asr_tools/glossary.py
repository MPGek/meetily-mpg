"""Per-meeting names and domain terms live in <meeting>/glossary.json (DVC-tracked, never in git).

{"prompt_terms": [...], "prompt_names": [...], "count_terms": {"label": "regex", ...}}
"""
import json, os


def load_glossary(meeting, key, purpose):
    path = os.path.join(meeting, "glossary.json")
    if not os.path.exists(path):
        raise SystemExit(f"{path} not found: {purpose} needs its '{key}' (see transcript_compare/README.md)")
    g = json.load(open(path, encoding="utf-8"))
    if key not in g:
        raise SystemExit(f"{path} has no '{key}', which {purpose} needs")
    return g[key]


def prompt_glossary(meeting, purpose):
    """'<terms>. <names>.' as used by the vocabulary-prompt variants."""
    terms = load_glossary(meeting, "prompt_terms", purpose)
    names = load_glossary(meeting, "prompt_names", purpose)
    return ", ".join(terms) + ". " + ", ".join(names) + "."
