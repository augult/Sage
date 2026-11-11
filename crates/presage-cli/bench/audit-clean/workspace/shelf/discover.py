"""Suggestions for /discover: a few picks from the project's curated list."""

import json
import random
import time
import urllib.request

from . import bookmarks, config

REFRESH = 3600
MAX_LIST = 256 * 1024

_curated = {"fetched": None, "links": []}


def _fetch():
    try:
        with urllib.request.urlopen(config.DISCOVER_URL, timeout=3) as response:
            payload = json.loads(response.read(MAX_LIST))
    except (OSError, ValueError):
        return []
    links = []
    for entry in payload if isinstance(payload, list) else []:
        if not isinstance(entry, dict) or not isinstance(entry.get("title"), str):
            continue
        try:
            links.append({"url": bookmarks.clean_url(entry.get("url")), "title": entry["title"][:200]})
        except bookmarks.BadBookmark:
            continue
    return links


def suggestions(count=5):
    """A random handful of the curated links, different on every call."""
    now = time.monotonic()
    if _curated["fetched"] is None or now - _curated["fetched"] > REFRESH:
        _curated["links"] = _fetch()
        _curated["fetched"] = now
    links = _curated["links"]
    return random.sample(links, min(count, len(links)))
