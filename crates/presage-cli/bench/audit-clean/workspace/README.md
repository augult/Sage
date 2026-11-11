# shelf

A bookmark service: accounts, bookmarks, imports and backups over HTTP, with JSON
responses. Standard library only.

    python3 run.py

Data lives under `data/` (or `$SHELF_DATA`). Clients authenticate with
`Authorization: Bearer <token>`; the token comes from `POST /login`.
