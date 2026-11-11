"""Attachments (private, per user) and avatars (public)."""

import os
import re

from . import config

SAFE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,99}")


class BadName(Exception):
    pass


def save_attachment(owner_id, name, data):
    if not SAFE_NAME.fullmatch(name):
        raise BadName(name)
    folder = config.ATTACH_DIR / str(int(owner_id))
    folder.mkdir(parents=True, exist_ok=True)
    (folder / name).write_bytes(data)


def read_attachment(owner_id, name):
    path = os.path.join(config.ATTACH_DIR, str(int(owner_id)), name)
    with open(path, "rb") as handle:
        return handle.read()


def list_attachments(owner_id):
    folder = config.ATTACH_DIR / str(int(owner_id))
    if not folder.is_dir():
        return []
    return sorted(entry.name for entry in folder.iterdir() if entry.is_file())


def read_avatar(name):
    """Avatars are public: anyone may fetch any file in the avatar folder."""
    root = config.AVATAR_DIR.resolve()
    path = (root / name).resolve()
    if not path.is_relative_to(root) or not path.is_file():
        raise BadName(name)
    return path.read_bytes()
