"""Daily figures from a web access log."""

import re


def _normalize(page):
    """Pages are compared without query string, fragment, case, and repeated or trailing slashes."""
    page = re.split(r"[?#]", page, maxsplit=1)[0]
    page = re.sub(r"/{2,}", "/", page).lower()
    if len(page) > 1 and page.endswith("/"):
        page = page[:-1]
    return page or "/"


def _first_seen(events):
    users = []
    for user, _, _ in events:
        if user not in users:
            users.append(user)
    return users


def _events_of(events, user):
    mine = [event for event in events if event[0] == user]
    mine.sort(key=lambda event: event[1])
    return mine


def _sessions_of(mine, gap):
    """Splits one user's events, in time order, into sessions: (start, end, pages)."""
    sessions = []
    for _, time, page in mine:
        if sessions and time - sessions[-1][1] <= gap:
            start, _, pages = sessions[-1]
            if pages[-1] != page:
                pages = pages + [page]
            sessions[-1] = (start, time, pages)
        else:
            sessions.append((time, time, [page]))
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
    events = [(user, time, _normalize(page)) for user, time, page in events]
    users = _first_seen(events)

    sessions = {}
    longest = None
    for user in users:
        mine = _sessions_of(_events_of(events, user), gap)
        sessions[user] = len(mine)
        for start, end, pages in mine:
            if longest is None or (end - start, -start) > (longest[2] - longest[1], -longest[1]):
                longest = (user, start, end, pages)

    visitors = {}
    for user, _, page in events:
        seen = visitors.setdefault(page, [])
        if user not in seen:
            seen.append(user)
    ranked = sorted(visitors.items(), key=lambda item: len(item[1]), reverse=True)

    busiest, most = None, 0
    for user in users:
        count = len([event for event in events if event[0] == user])
        if count >= most:
            busiest, most = user, count

    return {
        "users": users,
        "sessions": sessions,
        "longest": longest,
        "top_pages": [(page, len(seen)) for page, seen in ranked[:5]],
        "busiest": busiest,
    }
