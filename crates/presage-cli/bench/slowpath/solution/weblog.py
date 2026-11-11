"""Daily figures from a web access log."""

import re

_QUERY = re.compile(r"[?#]")
_SLASHES = re.compile(r"/{2,}")


def _normalize(page):
    """Pages are compared without query string, fragment, case, and repeated or trailing slashes."""
    page = _QUERY.split(page, maxsplit=1)[0]
    page = _SLASHES.sub("/", page).lower()
    if len(page) > 1 and page.endswith("/"):
        page = page[:-1]
    return page or "/"


def _sessions_of(mine, gap):
    """Splits one user's (time, page) events, in time order, into sessions: [start, end, pages]."""
    sessions = []
    for time, page in mine:
        if sessions and time - sessions[-1][1] <= gap:
            current = sessions[-1]
            current[1] = time
            if current[2][-1] != page:
                current[2].append(page)
        else:
            sessions.append([time, time, [page]])
    return sessions


def analyze(events, gap=1800):
    """Figures for a day of `(user, time, page)` events.

    Events come in log order, which is not always time order. `time` is in whole seconds. Returns a
    dict with:

    users      The users, in the order they first appear in the log.
    sessions   {user: number of sessions}. A session is a run of one user's events, taken in time
               order (events with the same time stay in log order), in which each event is at most
               `gap` seconds after the one before it.
    longest    (user, start, end, pages) for the session with the greatest end - start. Among equals
               the one that starts earliest, and among those the one of the user who appears first
               in the log. `pages` is the session's pages in order, with immediate repeats dropped.
               None when there are no events.
    top_pages  Up to 5 (page, visitors) pairs: the pages with the most distinct users, most first.
               Among equals the page that appears first in the log comes first.
    busiest    The user with the most events. Among equals the one who appears last in the log.
               None when there are no events.
    """
    # One pass: each user's events and each page's visitors, both in order of first appearance.
    by_user = {}
    visitors = {}
    pages = {}
    for user, time, raw in events:
        page = pages.get(raw)
        if page is None:
            page = pages[raw] = _normalize(raw)
        by_user.setdefault(user, []).append((time, page))
        visitors.setdefault(page, set()).add(user)

    sessions = {}
    longest = None
    busiest, most = None, 0
    for user, mine in by_user.items():
        if len(mine) >= most:
            busiest, most = user, len(mine)
        mine.sort(key=lambda event: event[0])
        found = _sessions_of(mine, gap)
        sessions[user] = len(found)
        for start, end, path in found:
            if longest is None or (end - start, -start) > (longest[2] - longest[1], -longest[1]):
                longest = (user, start, end, path)

    ranked = sorted(visitors.items(), key=lambda item: len(item[1]), reverse=True)
    return {
        "users": list(by_user),
        "sessions": sessions,
        "longest": longest,
        "top_pages": [(page, len(seen)) for page, seen in ranked[:5]],
        "busiest": busiest,
    }
