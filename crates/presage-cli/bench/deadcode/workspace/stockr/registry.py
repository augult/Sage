"""Stock checks, registered by name."""

_CHECKS = {}


def check(name):
    """Registers the decorated function as the check called `name`."""

    def decorator(function):
        _CHECKS[name] = function
        return function

    return decorator


def check_names():
    return sorted(_CHECKS)


def run_checks(inventory, names=None):
    """Runs the named checks (all of them by default): a list of (check, sku, message)."""
    problems = []
    for name in check_names() if names is None else names:
        function = _CHECKS[name]
        for item in inventory:
            message = function(item)
            if message:
                problems.append((name, item.sku, message))
    return problems
