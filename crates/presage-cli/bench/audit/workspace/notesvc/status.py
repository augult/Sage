"""The hosting provider's status, relayed on /status."""

import json
import urllib.request

from . import config


def upstream_status():
    try:
        with urllib.request.urlopen(config.STATUS_URL, timeout=3) as response:
            payload = json.load(response)
    except (OSError, ValueError):
        return "unknown"
    status = payload.get("status") if isinstance(payload, dict) else None
    return status if status in ("ok", "degraded", "down") else "unknown"
