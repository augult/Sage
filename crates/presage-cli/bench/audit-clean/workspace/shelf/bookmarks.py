"""Bookmarks: every bookmark belongs to one user."""

import time
from urllib.parse import urlsplit

SORTS = {"added": "created_at", "title": "title COLLATE NOCASE", "url": "url"}
MAX_PAGE = 200


class NotFound(Exception):
    pass


class BadBookmark(Exception):
    pass


def clean_url(url):
    """The URL, if it is an absolute http(s) address of sensible length."""
    if not isinstance(url, str) or len(url) > 2000:
        raise BadBookmark("the url is missing or too long")
    try:
        parts = urlsplit(url.strip())
    except ValueError:
        raise BadBookmark("the url is not valid")
    if parts.scheme not in ("http", "https") or not parts.netloc:
        raise BadBookmark("only http and https addresses can be saved")
    return parts.geturl()


def list_bookmarks(conn, owner_id, sort="added", newest_first=True, limit=50, offset=0):
    order = SORTS.get(sort, SORTS["added"])
    direction = "DESC" if newest_first else "ASC"
    limit = max(1, min(int(limit), MAX_PAGE))
    offset = max(0, int(offset))
    rows = conn.execute(
        f"SELECT id, url, title, created_at FROM bookmarks WHERE owner_id = ? ORDER BY {order} {direction}, id LIMIT ? OFFSET ?",
        (owner_id, limit, offset),
    )
    return [dict(row) for row in rows]


def search(conn, owner_id, term):
    """Bookmarks of this user whose title or address contains the term, taken literally."""
    literal = term.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
    pattern = f"%{literal}%"
    rows = conn.execute(
        "SELECT id, url, title, created_at FROM bookmarks "
        "WHERE owner_id = ? AND (title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\') ORDER BY created_at DESC, id LIMIT ?",
        (owner_id, pattern, pattern, MAX_PAGE),
    )
    return [dict(row) for row in rows]


def get(conn, owner_id, bookmark_id):
    row = conn.execute(
        "SELECT id, url, title, created_at FROM bookmarks WHERE id = ? AND owner_id = ?", (bookmark_id, owner_id)
    ).fetchone()
    if row is None:
        raise NotFound(bookmark_id)
    return dict(row)


def add(conn, owner_id, url, title):
    if not isinstance(title, str) or not 1 <= len(title) <= 200:
        raise BadBookmark("the title must be 1 to 200 characters")
    with conn:
        cur = conn.execute(
            "INSERT INTO bookmarks (owner_id, url, title, created_at) VALUES (?, ?, ?, ?)",
            (owner_id, clean_url(url), title, int(time.time())),
        )
    return cur.lastrowid


def delete_many(conn, owner_id, bookmark_ids):
    """Deletes those of the given bookmarks that belong to the user; returns how many."""
    ids = [int(bookmark_id) for bookmark_id in bookmark_ids][:MAX_PAGE]
    if not ids:
        return 0
    marks = ",".join("?" * len(ids))
    with conn:
        cur = conn.execute(f"DELETE FROM bookmarks WHERE owner_id = ? AND id IN ({marks})", (owner_id, *ids))
    return cur.rowcount
