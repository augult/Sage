"""Exporter plugins, as listed in plugins.json: {"exporters": {name: "module:function"}}."""

import importlib
import json
from pathlib import Path


def load_exporters(path="plugins.json"):
    """name -> function for every exporter in the plugin file. No file means no plugins."""
    path = Path(path)
    if not path.is_file():
        return {}
    exporters = {}
    for name, target in json.loads(path.read_text(encoding="utf-8")).get("exporters", {}).items():
        module_name, _, function_name = target.partition(":")
        exporters[name] = getattr(importlib.import_module(module_name), function_name)
    return exporters
