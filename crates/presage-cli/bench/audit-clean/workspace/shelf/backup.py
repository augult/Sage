"""Backups: a compressed JSON dump of one user's bookmarks, kept in that user's folder."""

import json
import subprocess
import time

from . import bookmarks, config


KEEP = 10


class NoSuchBackup(Exception):
    pass


def _folder(owner_id):
    return config.BACKUP_DIR / str(int(owner_id))


def make_backup(conn, owner_id):
    """Writes backups/<owner>/bookmarks-<time>.json.gz and returns its name."""
    folder = _folder(owner_id)
    folder.mkdir(parents=True, exist_ok=True)
    rows = conn.execute(
        "SELECT url, title, created_at FROM bookmarks WHERE owner_id = ? ORDER BY id", (owner_id,)
    ).fetchall()
    dump = folder / f"bookmarks-{int(time.time())}.json"
    dump.write_text(json.dumps([dict(row) for row in rows]), encoding="utf-8")
    subprocess.run(["gzip", "--force", str(dump)], check=True, timeout=60)
    for stale in sorted(folder.glob("bookmarks-*.json.gz"))[:-KEEP]:
        stale.unlink()
    return dump.name + ".gz"


def list_backups(owner_id):
    folder = _folder(owner_id)
    if not folder.is_dir():
        return []
    return sorted(entry.name for entry in folder.iterdir() if entry.is_file())


def read_backup(owner_id, name):
    """The bytes of one of the user's own backups."""
    root = _folder(owner_id).resolve()
    path = (root / name).resolve()
    if not path.is_relative_to(root) or not path.is_file():
        raise NoSuchBackup(name)
    return path.read_bytes()


def import_bookmarks(conn, owner_id, raw):
    """Adds the bookmarks in an uploaded JSON list of {"url", "title"}; returns how many were added."""
    try:
        entries = json.loads(raw)
    except ValueError:
        raise bookmarks.BadBookmark("the upload is not JSON")
    if not isinstance(entries, list) or len(entries) > 1000:
        raise bookmarks.BadBookmark("the upload must be a list of at most 1000 bookmarks")
    added = 0
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        try:
            bookmarks.add(conn, owner_id, entry.get("url"), entry.get("title") or entry.get("url"))
        except bookmarks.BadBookmark:
            continue
        added += 1
    return added
