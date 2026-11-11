# notesvc

A small notes service: accounts, notes, attachments and exports over HTTP, with JSON
responses. Standard library only.

    python3 run.py

Data lives under `data/` (or `$NOTESVC_DATA`). Clients authenticate with
`Authorization: Bearer <token>`; the token comes from `POST /login`.
