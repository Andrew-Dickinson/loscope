"""Simple disk-based result cache."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

_CACHE_ROOT = Path("data/web_cache")


def cache_key(inputs: dict) -> str:
    raw = json.dumps(inputs, sort_keys=True, default=str)
    return hashlib.sha256(raw.encode()).hexdigest()[:16]


def load_cache(endpoint: str, key: str) -> dict | None:
    path = _CACHE_ROOT / endpoint / f"{key}.json"
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except (ValueError, OSError):
        return None


def save_cache(endpoint: str, key: str, result: dict) -> None:
    path = _CACHE_ROOT / endpoint / f"{key}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result))
