# -*- coding: utf-8 -*-
"""アプリのアイコン（exe・窓・タスクバー・ショートカット・起動画面）を描く。標準ライブラリだけで書く（作るとき desktop/build.rs が呼ぶ）。

    python program/app/app_icon.py <出力フォルダ> [案]     icon.ico と icon-<大きさ>.png を書く（案を比べるとき）

形は「中心からの距離」（SDF）で書き、縁は 1px の幅でぼかす（どの大きさでも同じ式で描け、縮めて潰れない）。
絵の決め方は program/tools/ui-proposals/2026-10-icon.json（案を描いて別のエージェントが採点）。
"""
from __future__ import annotations

import math
import struct
import sys
import zlib
from pathlib import Path

SIZES = (16, 20, 24, 32, 40, 48, 64, 256)      # .ico に入れる大きさ（Windows が表示の倍率で選ぶ）


def _hex(h):
    return tuple(int(h[i:i + 2], 16) / 255 for i in (1, 3, 5))


def _clamp(x, lo=0.0, hi=1.0):
    return lo if x < lo else hi if x > hi else x


def rrect(cx, cy, hw, hh, r):
    """角の丸い長方形（中心・半分の幅と高さ・角の丸み）"""
    def d(x, y):
        qx, qy = abs(x - cx) - hw + r, abs(y - cy) - hh + r
        return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r
    return d


def circle(cx, cy, r):
    return lambda x, y: math.hypot(x - cx, y - cy) - r


def segment(ax, ay, bx, by, r):
    dx, dy = bx - ax, by - ay
    ll = dx * dx + dy * dy or 1e-9

    def d(x, y):
        h = _clamp(((x - ax) * dx + (y - ay) * dy) / ll)
        return math.hypot(x - ax - dx * h, y - ay - dy * h) - r
    return d


def gradient(c0, c1):
    """左上から右下へのなだらかな地"""
    return lambda x, y: tuple(c0[i] + (c1[i] - c0[i]) * _clamp((x + y) / 2) for i in range(3))


# 絵の案: [(形, 色)]（下から順に重ねる）。座標は 0〜1。色は (r, g, b) か、座標から色を出す関数
BLUE0, BLUE1 = _hex("#0b4f8f"), _hex("#1a7fd4")
WHITE, AMBER, MINT = _hex("#ffffff"), _hex("#f2a93b"), _hex("#bfe3ff")


def design_gantt():
    """作業の流れ（設備ごとの帯を時刻の順に並べ、いまの時刻にアンバーの線）"""
    ground = rrect(0.5, 0.5, 0.46, 0.46, 0.13)
    bars = [rrect(0.36, 0.30, 0.18, 0.065, 0.06), rrect(0.55, 0.50, 0.24, 0.065, 0.06), rrect(0.46, 0.70, 0.20, 0.065, 0.06)]
    now = segment(0.66, 0.18, 0.66, 0.82, 0.03)
    return [(ground, gradient(BLUE0, BLUE1))] + [(b, WHITE) for b in bars] + [(now, AMBER)]


DESIGNS = {"gantt": design_gantt}
DEFAULT = "gantt"


def render(size, design=DEFAULT):
    """RGBA のバイト列（上の行から）"""
    layers = DESIGNS[design]()
    px = bytearray()
    aa = 1.0 / size
    for j in range(size):
        y = (j + 0.5) / size
        for i in range(size):
            x = (i + 0.5) / size
            r = g = b = a = 0.0
            for shape, color in layers:
                cov = _clamp(0.5 - shape(x, y) / aa)
                if cov <= 0:
                    continue
                c = color(x, y) if callable(color) else color
                # 上に重ねる（前に塗った色と混ぜる）
                na = cov + a * (1 - cov)
                if na > 0:
                    r = (c[0] * cov + r * a * (1 - cov)) / na
                    g = (c[1] * cov + g * a * (1 - cov)) / na
                    b = (c[2] * cov + b * a * (1 - cov)) / na
                a = na
            px += bytes((round(r * 255), round(g * 255), round(b * 255), round(a * 255)))
    return bytes(px)


def png(size, design=DEFAULT):
    raw = render(size, design)
    rows = b"".join(b"\0" + raw[k * size * 4:(k + 1) * size * 4] for k in range(size))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def build(design=DEFAULT):
    """.ico（中身は大きさごとの PNG）"""
    images = [png(s, design) for s in SIZES]
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for s, data in zip(SIZES, images):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return head + entries + b"".join(images)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    d = sys.argv[2] if len(sys.argv) > 2 else DEFAULT
    (out / "icon.ico").write_bytes(build(d))
    for s in (16, 24, 32, 48, 256):
        (out / f"icon-{s}.png").write_bytes(png(s, d))
