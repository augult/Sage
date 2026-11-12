"""Settings for the bookmark service."""

import os
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
DATA_DIR = Path(os.environ.get("SHELF_DATA", BASE_DIR / "data"))
DB_PATH = DATA_DIR / "shelf.db"
BACKUP_DIR = DATA_DIR / "backups"

HOST = "127.0.0.1"
PORT = 8050

SESSION_TTL = 8 * 3600
MAX_BODY = 64 * 1024
MAX_IMPORT = 512 * 1024

# The curated list behind /discover, published by the project itself.
DISCOVER_URL = "https://lists.shelf.example/v1/popular.json"
