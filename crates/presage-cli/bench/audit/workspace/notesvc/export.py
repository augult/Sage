"""Archives of a user's attachments."""

import subprocess

from . import config


def make_archive(owner_id, label):
    """Bundles the user's attachments into exports/<owner>-<label>.tar.gz and returns its name."""
    source = config.ATTACH_DIR / str(int(owner_id))
    source.mkdir(parents=True, exist_ok=True)
    config.EXPORT_DIR.mkdir(parents=True, exist_ok=True)
    target = config.EXPORT_DIR / f"{int(owner_id)}-{label}.tar.gz"
    subprocess.run(f"tar -czf {target} -C {source} .", shell=True, check=True, timeout=60)
    return target.name


def disk_usage(owner_id):
    """Kilobytes used by the user's attachments."""
    folder = config.ATTACH_DIR / str(int(owner_id))
    if not folder.is_dir():
        return 0
    result = subprocess.run(["du", "-sk", str(folder)], capture_output=True, text=True, check=True, timeout=30)
    return int(result.stdout.split()[0])
