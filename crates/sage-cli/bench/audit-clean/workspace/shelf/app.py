"""The HTTP interface. Every response is JSON except backup downloads."""

import hashlib
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlsplit

from . import accounts, backup, bookmarks, config, db, discover


class HttpError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


def etag_of(body):
    return '"' + hashlib.md5(body).hexdigest() + '"'


class Handler(BaseHTTPRequestHandler):
    server_version = "shelf"

    def log_message(self, format, *args):
        pass

    # Plumbing.

    def _send_json(self, code, payload):
        body = json.dumps(payload).encode("utf-8")
        tag = etag_of(body)
        if code == 200 and self.command == "GET" and self.headers.get("If-None-Match") == tag:
            self.send_response(304)
            self.send_header("ETag", tag)
            self.end_headers()
            return
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "private, no-cache")
        self.send_header("ETag", tag)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_file(self, data):
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", "attachment")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Cache-Control", "private, no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _body(self, limit=config.MAX_BODY):
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            raise HttpError(400, "bad Content-Length")
        if length < 0 or length > limit:
            raise HttpError(413, "body too large")
        return self.rfile.read(length)

    def _json_body(self):
        try:
            payload = json.loads(self._body() or b"{}")
        except ValueError:
            raise HttpError(400, "the body is not JSON")
        if not isinstance(payload, dict):
            raise HttpError(400, "the body must be a JSON object")
        return payload

    def _text(self, payload, field, max_len):
        value = payload.get(field)
        if not isinstance(value, str) or len(value) > max_len:
            raise HttpError(400, f"{field} must be a string of at most {max_len} characters")
        return value

    def _token(self):
        header = self.headers.get("Authorization", "")
        return header[7:] if header.startswith("Bearer ") else ""

    def _user(self):
        user_id = accounts.user_for_token(self.conn, self._token())
        if user_id is None:
            raise HttpError(401, "sign in first")
        return user_id

    def _dispatch(self, method):
        self.conn = db.connect()
        try:
            url = urlsplit(self.path)
            parts = [unquote(part) for part in url.path.split("/") if part]
            query = {key: values[0] for key, values in parse_qs(url.query).items()}
            self._route(method, parts, query)
        except HttpError as error:
            self._send_json(error.code, {"error": error.message})
        except (bookmarks.NotFound, backup.NoSuchBackup):
            self._send_json(404, {"error": "not found"})
        except (accounts.AccountError, bookmarks.BadBookmark) as error:
            self._send_json(400, {"error": str(error)})
        except Exception:
            self._send_json(500, {"error": "internal error"})
        finally:
            self.conn.close()

    def do_GET(self):
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")

    def do_DELETE(self):
        self._dispatch("DELETE")

    def _route(self, method, parts, query):
        routes = {
            ("POST", "register"): self._register,
            ("POST", "login"): self._login,
            ("POST", "logout"): self._logout,
            ("GET", "bookmarks"): self._bookmarks_get,
            ("POST", "bookmarks"): self._bookmarks_post,
            ("DELETE", "bookmarks"): self._bookmarks_delete,
            ("POST", "import"): self._import,
            ("GET", "backups"): self._backups_get,
            ("POST", "backups"): self._backups_post,
            ("GET", "discover"): self._discover,
        }
        handler = routes.get((method, parts[0] if parts else ""))
        if handler is None:
            raise HttpError(404, "not found")
        handler(parts[1:], query)

    @staticmethod
    def _number(value, default):
        return int(value) if value.isdigit() and len(value) < 10 else default

    # Accounts.

    def _register(self, rest, query):
        payload = self._json_body()
        accounts.register(self.conn, self._text(payload, "username", 32), self._text(payload, "password", 256))
        self._send_json(201, {"ok": True})

    def _login(self, rest, query):
        payload = self._json_body()
        token = accounts.login(self.conn, self._text(payload, "username", 32), self._text(payload, "password", 256))
        self._send_json(200, {"token": token})

    def _logout(self, rest, query):
        self._user()
        accounts.logout(self.conn, self._token())
        self._send_json(200, {"ok": True})

    # Bookmarks.

    def _bookmarks_get(self, rest, query):
        user_id = self._user()
        if rest == ["search"]:
            term = query.get("q", "")
            if len(term) > 100:
                raise HttpError(400, "the search term is too long")
            self._send_json(200, {"bookmarks": bookmarks.search(self.conn, user_id, term)})
        elif len(rest) == 1 and rest[0].isdigit():
            self._send_json(200, bookmarks.get(self.conn, user_id, int(rest[0])))
        elif rest:
            raise HttpError(404, "not found")
        else:
            listing = bookmarks.list_bookmarks(
                self.conn,
                user_id,
                sort=query.get("sort", "added"),
                newest_first=query.get("order", "desc") != "asc",
                limit=self._number(query.get("limit", ""), 50),
                offset=self._number(query.get("offset", ""), 0),
            )
            self._send_json(200, {"bookmarks": listing})

    def _bookmarks_post(self, rest, query):
        user_id = self._user()
        payload = self._json_body()
        bookmark_id = bookmarks.add(self.conn, user_id, payload.get("url"), payload.get("title"))
        self._send_json(201, {"id": bookmark_id})

    def _bookmarks_delete(self, rest, query):
        user_id = self._user()
        ids = self._json_body().get("ids")
        if not isinstance(ids, list) or not all(isinstance(i, int) and not isinstance(i, bool) for i in ids):
            raise HttpError(400, "ids must be a list of numbers")
        self._send_json(200, {"deleted": bookmarks.delete_many(self.conn, user_id, ids)})

    # Imports and backups.

    def _import(self, rest, query):
        user_id = self._user()
        self._send_json(200, {"added": backup.import_bookmarks(self.conn, user_id, self._body(config.MAX_IMPORT))})

    def _backups_get(self, rest, query):
        user_id = self._user()
        if not rest:
            self._send_json(200, {"backups": backup.list_backups(user_id)})
        else:
            self._send_file(backup.read_backup(user_id, "/".join(rest)))

    def _backups_post(self, rest, query):
        self._send_json(201, {"backup": backup.make_backup(self.conn, self._user())})

    def _discover(self, rest, query):
        self._user()
        self._send_json(200, {"suggestions": discover.suggestions()})


def serve():
    server = ThreadingHTTPServer((config.HOST, config.PORT), Handler)
    try:
        server.serve_forever()
    finally:
        server.server_close()
