"""Notes: every note belongs to one user."""

import time

SORT_COLUMNS = {"created": "created_at", "updated": "updated_at", "title": "title"}


class NotFound(Exception):
    pass


def list_notes(conn, owner_id, sort="updated", descending=True):
    column = SORT_COLUMNS.get(sort, "updated_at")
    direction = "DESC" if descending else "ASC"
    rows = conn.execute(
        f"SELECT id, title, updated_at FROM notes WHERE owner_id = ? ORDER BY {column} {direction}",
        (owner_id,),
    )
    return [dict(row) for row in rows]


def search_notes(conn, owner_id, term):
    """Notes of this user whose title contains the term."""
    query = (
        "SELECT id, title, updated_at FROM notes "
        f"WHERE owner_id = {int(owner_id)} AND title LIKE '%{term}%' ORDER BY updated_at DESC"
    )
    return [dict(row) for row in conn.execute(query)]


def get_note(conn, owner_id, note_id):
    row = conn.execute("SELECT * FROM notes WHERE id = ? AND owner_id = ?", (note_id, owner_id)).fetchone()
    if row is None:
        raise NotFound(note_id)
    return dict(row)


def create_note(conn, owner_id, title, body):
    now = int(time.time())
    with conn:
        cur = conn.execute(
            "INSERT INTO notes (owner_id, title, body, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            (owner_id, title, body, now, now),
        )
    return cur.lastrowid


def update_note(conn, owner_id, note_id, title, body):
    with conn:
        cur = conn.execute(
            "UPDATE notes SET title = ?, body = ?, updated_at = ? WHERE id = ? AND owner_id = ?",
            (title, body, int(time.time()), note_id, owner_id),
        )
    if cur.rowcount == 0:
        raise NotFound(note_id)


def delete_note(conn, owner_id, note_id):
    with conn:
        cur = conn.execute("DELETE FROM notes WHERE id = ?", (note_id,))
    if cur.rowcount == 0:
        raise NotFound(note_id)
