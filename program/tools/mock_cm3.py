# -*- coding: utf-8 -*-
"""作り物の CM3（init だけ）。開発・画面の撮影・測定で、本物の CM3 の代わりに使う。

    python program/tools/mock_cm3.py <fixture.py で作ったフォルダ> [ポート=18085] [遅れ(秒)=0]

POST /Cm3SchWeb/service/schapp/cmschview/init に、要求の setsubiName の応答（<フォルダ>/cm3/<設備>.json）を返す。
遅れを付けると、応答の前に待つ（並列に取りに行く効き目を測るため。本物は 1 設備あたり 1 秒前後）。
"""
from __future__ import annotations

import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def make_handler(folder: Path, delay: float):
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802 (http.server の決まりの名前)
            body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
            try:
                eq = json.loads(body.decode("utf-8")).get("setsubiName") or ""
            except ValueError:
                eq = ""
            path = folder / "cm3" / f"{eq}.json"
            if delay:
                time.sleep(delay)
            if not path.is_file() or "/init" not in self.path:
                self.send_response(404)
                self.end_headers()
                return
            data = path.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "application/json;charset=UTF-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    return Handler


def serve(folder: Path, port: int = 18085, delay: float = 0.0) -> ThreadingHTTPServer:
    return ThreadingHTTPServer(("127.0.0.1", port), make_handler(folder, delay))


if __name__ == "__main__":
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    srv = serve(Path(a[0]), int(a[1]) if len(a) > 1 else 18085, float(a[2]) if len(a) > 2 else 0.0)
    print(f"mock CM3: http://127.0.0.1:{srv.server_address[1]}", flush=True)
    srv.serve_forever()
