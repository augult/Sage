"""Display preferences, kept in a cookie so they survive without an account."""

import base64
import pickle

DEFAULTS = {"theme": "light", "page_size": 20, "sort": "updated"}


def dump_prefs(prefs):
    return base64.urlsafe_b64encode(pickle.dumps(prefs)).decode("ascii")


def load_prefs(cookie_value):
    """The preferences in the `prefs` cookie, or the defaults when it is missing or damaged."""
    if not cookie_value:
        return dict(DEFAULTS)
    try:
        prefs = pickle.loads(base64.urlsafe_b64decode(cookie_value))
    except Exception:
        return dict(DEFAULTS)
    if not isinstance(prefs, dict):
        return dict(DEFAULTS)
    return {key: prefs.get(key, default) for key, default in DEFAULTS.items()}
