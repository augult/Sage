"""CONNECT proxy that only lets logfare.ai:443 through, chained to the session's upstream proxy.

usage: python3 -I allowproxy.py <listen port> <upstream port>
"""
import asyncio
import sys

ALLOW = {("logfare.ai", 443)}
LISTEN, UPSTREAM = int(sys.argv[1]), int(sys.argv[2])
denied = 0


async def pipe(r, w):
    try:
        while data := await r.read(65536):
            w.write(data)
            await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def handle(cr, cw):
    global denied
    try:
        head = await cr.readuntil(b"\r\n\r\n")
        line = head.split(b"\r\n", 1)[0].decode("latin-1").split()
        host, _, port = line[1].rpartition(":") if len(line) > 1 else ("", "", "0")
        if line[0] != "CONNECT" or (host.lower(), int(port or 0)) not in ALLOW:
            denied += 1
            print(f"DENY {' '.join(line[:2])}", flush=True)
            cw.write(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            await cw.drain()
            cw.close()
            return
        ur, uw = await asyncio.open_connection("127.0.0.1", UPSTREAM)
        uw.write(head)
        await uw.drain()
        await asyncio.gather(pipe(cr, uw), pipe(ur, cw))
    except Exception:
        cw.close()


async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", LISTEN)
    async with srv:
        await srv.serve_forever()


asyncio.run(main())
