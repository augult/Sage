# pool

A small connection pool, used by the report workers (`examples/worker.py`) to share a handful of
database connections between many threads.

```python
from pool import ConnectionPool

pool = ConnectionPool(factory=open_connection, max_size=4)

conn = pool.acquire(timeout=2.0)
try:
    conn.execute("select 1")
finally:
    pool.release(conn)

with pool.connection(timeout=2.0) as conn:   # the same, shorter
    conn.execute("select 1")

pool.close()
```

`factory` is any callable that returns a new connection. A connection is any object with a
`close()` method; the pool never looks at anything else.

## Guarantees

The pool is shared by many threads. Whatever they do, and in whatever order:

1. **Bounded.** No more than `max_size` connections exist at any moment, counting the idle ones,
   the ones in use and the ones `factory` is creating right now.
2. **Exclusive.** A connection is held by one caller at a time: from `acquire` returning it until
   that caller passes it to `release`.
3. **Reuse first.** `acquire` hands out an idle connection if there is one, the most recently
   released first, and only calls `factory` when there is none.
4. **Blocking with a deadline.** When the pool is exhausted, `acquire(timeout=None)` waits until a
   connection can be had. With a timeout it raises `PoolTimeout` once that many seconds have
   passed in total, however often the wait was interrupted in between. `timeout=0` never waits.
5. **Prompt hand-over.** `release` wakes a waiting `acquire` at once. Waiters sleep until there is
   something for them; they do not poll.
6. **A slow factory blocks nobody else.** `factory` opens a network connection and can take
   seconds. While one thread is in `factory`, the others can still acquire idle connections and
   release theirs: the pool's lock is not held during the call.
7. **A failing factory costs nothing.** When `factory` raises, the caller of `acquire` gets that
   exception, and the place the new connection would have taken is free again, for a waiting
   `acquire` as much as for a later one.
8. **Only what was handed out comes back.** `release` of an object that is not currently checked
   out of this pool (a stranger, or a connection released twice) raises `ValueError` and changes
   nothing.
9. **Closing.** `close()` calls `close()` on the idle connections and forgets them. From then on
   `acquire` raises `PoolClosed`, including the calls that were already waiting. Connections
   that are still checked out are closed when they are released, instead of going back into the
   pool. Calling `close()` again does nothing.
10. **Honest numbers.** `stats()` returns `{"size", "idle", "in_use", "created"}`: the connections
    that exist, how many of them are idle and in use, and how many were ever created. It is a
    consistent snapshot: `idle + in_use <= size <= max_size`.

## Tests

    python3 -m unittest discover -s tests
