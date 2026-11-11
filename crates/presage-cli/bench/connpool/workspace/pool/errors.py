class PoolError(Exception):
    """Base class of the pool's own errors."""


class PoolTimeout(PoolError):
    """No connection became available in time."""


class PoolClosed(PoolError):
    """The pool was closed."""
