"""Accounts and sessions."""

import hashlib
import hmac
import re
import secrets
import time

from . import config

USERNAME = re.compile(r"[a-z0-9_]{3,32}")


class AuthError(Exception):
    pass


def hash_password(password):
    return hashlib.md5(password.encode("utf-8")).hexdigest()


def check_password(password, stored):
    return hmac.compare_digest(hash_password(password), stored)


def register(conn, username, password):
    if not USERNAME.fullmatch(username):
        raise AuthError("usernames are 3-32 characters: a-z, 0-9 and _")
    if len(password) < 10:
        raise AuthError("passwords need at least 10 characters")
    taken = conn.execute("SELECT 1 FROM users WHERE username = ?", (username,)).fetchone()
    if taken:
        raise AuthError("that username is taken")
    with conn:
        conn.execute("INSERT INTO users (username, password) VALUES (?, ?)", (username, hash_password(password)))


def _token_hash(token):
    # Session tokens are 256 random bits; the table stores only their digest.
    return hashlib.sha256(token.encode("ascii", "replace")).hexdigest()


def login(conn, username, password):
    """Returns a new session token, or raises AuthError."""
    row = conn.execute("SELECT id, password FROM users WHERE username = ?", (username,)).fetchone()
    stored = row["password"] if row else ""
    if not check_password(password, stored) or row is None:
        raise AuthError("wrong username or password")
    token = secrets.token_urlsafe(32)
    with conn:
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
            (_token_hash(token), row["id"], int(time.time()) + config.SESSION_TTL),
        )
    return token


def logout(conn, token):
    with conn:
        conn.execute("DELETE FROM sessions WHERE token_hash = ?", (_token_hash(token),))


def user_for_token(conn, token):
    """The id of the user the token belongs to, or None."""
    if not token:
        return None
    row = conn.execute(
        "SELECT user_id FROM sessions WHERE token_hash = ? AND expires_at > ?",
        (_token_hash(token), int(time.time())),
    ).fetchone()
    return row["user_id"] if row else None
