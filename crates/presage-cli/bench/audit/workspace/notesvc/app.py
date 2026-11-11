"""The HTTP interface. Every response is JSON except attachment and avatar downloads."""

import json
from http.cookies import SimpleCookie
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlsplit

from . import auth, config, db, export, files, notes, prefs, status
from .cache import TTLCache, cache_key

LISTINGS = TTLCache()


class HttpError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


class Handler(BaseHTTPRequestHandler):
    server_version = "notesvc"

    def log_message(self, format, *args):
        pass

    # Plumbing.

    def _send_json(self, code, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_file(self, data):
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", "attachment")
        self.send_header("X-Content-Type-Options", "nosniff")
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

    def _user(self):
        header = self.headers.get("Authorization", "")
        token = header[7:] if header.startswith("Bearer ") else ""
        user_id = auth.user_for_token(self.conn, token)
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
        except (notes.NotFound, files.BadName, FileNotFoundError):
            self._send_json(404, {"error": "not found"})
        except auth.AuthError as error:
            self._send_json(400, {"error": str(error)})
        except Exception:
            self._send_json(500, {"error": "internal error"})
        finally:
            self.conn.close()

    def do_GET(self):
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")

    def do_PUT(self):
        self._dispatch("PUT")

    def do_DELETE(self):
        self._dispatch("DELETE")

    def _route(self, method, parts, query):
        routes = {
            ("POST", "register"): self._register,
            ("POST", "login"): self._login,
            ("POST", "logout"): self._logout,
            ("GET", "notes"): self._notes_get,
            ("POST", "notes"): self._notes_post,
            ("PUT", "notes"): self._notes_put,
            ("DELETE", "notes"): self._notes_delete,
            ("GET", "attachments"): self._attachment_get,
            ("PUT", "attachments"): self._attachment_put,
            ("GET", "avatars"): self._avatar_get,
            ("POST", "export"): self._export,
            ("GET", "usage"): self._usage,
            ("GET", "prefs"): self._prefs_get,
            ("PUT", "prefs"): self._prefs_put,
            ("GET", "status"): self._status,
        }
        handler = routes.get((method, parts[0] if parts else ""))
        if handler is None:
            raise HttpError(404, "not found")
        handler(parts[1:], query)

    @staticmethod
    def _note_id(rest):
        if len(rest) != 1 or not rest[0].isdigit():
            raise HttpError(404, "not found")
        return int(rest[0])

    # Accounts.

    def _register(self, rest, query):
        payload = self._json_body()
        auth.register(self.conn, self._text(payload, "username", 32), self._text(payload, "password", 256))
        self._send_json(201, {"ok": True})

    def _login(self, rest, query):
        payload = self._json_body()
        token = auth.login(self.conn, self._text(payload, "username", 32), self._text(payload, "password", 256))
        self._send_json(200, {"token": token})

    def _logout(self, rest, query):
        self._user()
        auth.logout(self.conn, self.headers.get("Authorization", "")[7:])
        self._send_json(200, {"ok": True})

    # Notes.

    def _notes_get(self, rest, query):
        user_id = self._user()
        if rest == ["search"]:
            self._notes_search(user_id, query)
        elif rest:
            self._send_json(200, notes.get_note(self.conn, user_id, self._note_id(rest)))
        else:
            self._notes_list(user_id, query)

    def _notes_list(self, user_id, query):
        sort = query.get("sort", "updated")
        descending = query.get("order", "desc") != "asc"
        key = cache_key(user_id, sort, descending)
        listing = LISTINGS.get(key)
        if listing is None:
            listing = notes.list_notes(self.conn, user_id, sort, descending)
            LISTINGS.put(key, user_id, listing)
        self._send_json(200, {"notes": listing})

    def _notes_search(self, user_id, query):
        term = query.get("q", "")
        if len(term) > 100:
            raise HttpError(400, "the search term is too long")
        self._send_json(200, {"notes": notes.search_notes(self.conn, user_id, term)})

    def _notes_post(self, rest, query):
        user_id = self._user()
        payload = self._json_body()
        note_id = notes.create_note(self.conn, user_id, self._text(payload, "title", 200), self._text(payload, "body", 20000))
        LISTINGS.drop_owner(user_id)
        self._send_json(201, {"id": note_id})

    def _notes_put(self, rest, query):
        user_id = self._user()
        payload = self._json_body()
        notes.update_note(
            self.conn, user_id, self._note_id(rest), self._text(payload, "title", 200), self._text(payload, "body", 20000)
        )
        LISTINGS.drop_owner(user_id)
        self._send_json(200, {"ok": True})

    def _notes_delete(self, rest, query):
        user_id = self._user()
        notes.delete_note(self.conn, user_id, self._note_id(rest))
        LISTINGS.drop_owner(user_id)
        self._send_json(200, {"ok": True})

    # Files.

    def _attachment_get(self, rest, query):
        user_id = self._user()
        if not rest:
            self._send_json(200, {"attachments": files.list_attachments(user_id)})
        else:
            self._send_file(files.read_attachment(user_id, "/".join(rest)))

    def _attachment_put(self, rest, query):
        user_id = self._user()
        if len(rest) != 1:
            raise HttpError(404, "not found")
        files.save_attachment(user_id, rest[0], self._body(config.MAX_ATTACHMENT))
        self._send_json(201, {"ok": True})

    def _avatar_get(self, rest, query):
        if len(rest) != 1:
            raise HttpError(404, "not found")
        self._send_file(files.read_avatar(rest[0]))

    def _export(self, rest, query):
        user_id = self._user()
        label = self._text(self._json_body(), "label", 40)
        self._send_json(201, {"archive": export.make_archive(user_id, label)})

    def _usage(self, rest, query):
        self._send_json(200, {"kilobytes": export.disk_usage(self._user())})

    # Preferences and status.

    def _prefs_get(self, rest, query):
        cookie = SimpleCookie(self.headers.get("Cookie", ""))
        value = cookie["prefs"].value if "prefs" in cookie else ""
        self._send_json(200, prefs.load_prefs(value))

    def _prefs_put(self, rest, query):
        payload = self._json_body()
        current = dict(prefs.DEFAULTS)
        for key in current:
            if key in payload and isinstance(payload[key], type(current[key])):
                current[key] = payload[key]
        body = json.dumps(current).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Set-Cookie", f"prefs={prefs.dump_prefs(current)}; Path=/; HttpOnly; SameSite=Lax")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _status(self, rest, query):
        self._send_json(200, {"service": "ok", "hosting": status.upstream_status()})


def serve():
    server = ThreadingHTTPServer((config.HOST, config.PORT), Handler)
    try:
        server.serve_forever()
    finally:
        server.server_close()
