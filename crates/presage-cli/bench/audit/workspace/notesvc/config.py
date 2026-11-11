"""Settings for the notes service."""

import os
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
DATA_DIR = Path(os.environ.get("NOTESVC_DATA", BASE_DIR / "data"))
DB_PATH = DATA_DIR / "notes.db"
ATTACH_DIR = DATA_DIR / "attachments"
AVATAR_DIR = DATA_DIR / "avatars"
EXPORT_DIR = DATA_DIR / "exports"

HOST = "127.0.0.1"
PORT = 8040

SESSION_TTL = 8 * 3600
MAX_BODY = 64 * 1024
MAX_ATTACHMENT = 2 * 1024 * 1024

# The public status page of the hosting provider, shown on /status.
STATUS_URL = "https://status.notesvc.example/api/v1/ping"
