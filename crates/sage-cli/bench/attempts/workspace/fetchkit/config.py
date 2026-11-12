"""Settings: defaults, then the config file, then the environment."""

import configparser
import os

DEFAULTS = {"retries": 2, "timeout": 10.0, "backoff": 0.5}


class ConfigError(Exception):
    """A setting that cannot be used."""


def _convert(key, raw):
    kind = type(DEFAULTS[key])
    try:
        return kind(raw)
    except (TypeError, ValueError):
        raise ConfigError(f"{key} must be {kind.__name__}: {raw!r}") from None


def validate(config):
    if config["retries"] < 0:
        raise ConfigError("retries must not be negative")
    if config["timeout"] <= 0:
        raise ConfigError("timeout must be positive")
    if config["backoff"] < 0:
        raise ConfigError("backoff must not be negative")
    return config


def load_config(path=None, env=None):
    """The effective settings. `path` is an INI file with a `[fetch]` section, or None."""
    env = os.environ if env is None else env
    config = dict(DEFAULTS)
    if path is not None:
        parser = configparser.ConfigParser(inline_comment_prefixes=(";", "#"))
        if not parser.read(path, encoding="utf-8"):
            raise ConfigError(f"cannot read {path}")
        if parser.has_section("fetch"):
            for key, raw in parser.items("fetch"):
                if key not in DEFAULTS:
                    raise ConfigError(f"unknown option {key!r} in {path}")
                config[key] = _convert(key, raw)
    # Every setting can be overridden as FETCHKIT_<NAME>.
    for key in DEFAULTS:
        raw = env.get("FETCHKIT_" + key.upper())
        if raw is not None:
            config[key] = _convert(key, raw)
    return validate(config)
