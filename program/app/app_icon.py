# -*- coding: utf-8 -*-
"""アプリのアイコン（exe・窓・タスクバー・ショートカット・起動画面）を描く。標準ライブラリだけで書く（作るとき desktop/build.rs が呼ぶ）。

    python program/app/app_icon.py <出力フォルダ> [案]     icon.ico と icon-<大きさ>.png を書く（案を比べるとき）
    node program/tools/icon-sheet.mjs <出力フォルダ>        案を並べた一覧の画像（明るい地・タスクバーの暗い地・起動画面の紺。16〜256px）

形は「中心からの距離」（SDF）で書き、縁は 1px の幅でぼかす（どの大きさでも同じ式で描け、縮めて潰れない）。
絵の決め方は program/tools/ui-proposals/2026-10-icon.json（5 案を描いて別のエージェントが採点し、gantt〈作業の流れの帯〉を 85.5 点で選んだ）。
ほかの案は比べた記録として残す（DESIGNS。描くのは DEFAULT だけ）。
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


def design_gantt(size=256):
    """作業の流れ（設備ごとの帯を時刻の順に並べ、いまの時刻にアンバーの線）。
    小さい絵（24px 以下）は線を 2 画素の幅にする（1 画素だと白い帯とまざって淡く見えた: 採点の指摘）"""
    ground = rrect(0.5, 0.5, 0.46, 0.46, 0.13)
    bars = [rrect(0.36, 0.30, 0.18, 0.065, 0.06), rrect(0.55, 0.50, 0.24, 0.065, 0.06), rrect(0.46, 0.70, 0.20, 0.065, 0.06)]
    now = segment(0.66, 0.18, 0.66, 0.82, max(0.03, 1.0 / size))
    return [(ground, gradient(BLUE0, BLUE1))] + [(b, WHITE) for b in bars] + [(now, AMBER)]


def design_calendar():
    """暦と時計（予定の時刻）"""
    ground = rrect(0.5, 0.5, 0.46, 0.46, 0.13)
    page = rrect(0.45, 0.52, 0.29, 0.28, 0.05)
    head = rrect(0.45, 0.30, 0.29, 0.07, 0.04)
    rings = [rrect(0.33, 0.22, 0.025, 0.06, 0.02), rrect(0.57, 0.22, 0.025, 0.06, 0.02)]
    cells = [rrect(0.30 + 0.1 * (i % 3), 0.48 + 0.11 * (i // 3), 0.035, 0.035, 0.01) for i in range(6)]
    clock = circle(0.72, 0.72, 0.17)
    face = circle(0.72, 0.72, 0.13)
    hands = [segment(0.72, 0.72, 0.72, 0.63, 0.018), segment(0.72, 0.72, 0.79, 0.74, 0.018)]
    return ([(ground, gradient(BLUE0, BLUE1)), (page, WHITE), (head, MINT)] + [(r, BLUE0) for r in rings]
            + [(c, BLUE1) for c in cells] + [(clock, AMBER), (face, WHITE)] + [(h, BLUE0) for h in hands])


def design_coil():
    """コイル（巻いた板）と進む向き"""
    ground = rrect(0.5, 0.5, 0.46, 0.46, 0.13)
    rings = [(circle(0.42, 0.5, r), c) for r, c in ((0.27, WHITE), (0.21, MINT), (0.15, WHITE), (0.08, BLUE0))]
    strip = rrect(0.66, 0.77, 0.22, 0.045, 0.02)
    arrow = [segment(0.70, 0.62, 0.84, 0.62, 0.03), segment(0.84, 0.62, 0.78, 0.56, 0.03), segment(0.84, 0.62, 0.78, 0.68, 0.03)]
    return [(ground, gradient(BLUE0, BLUE1))] + rings + [(strip, WHITE)] + [(a, AMBER) for a in arrow]


def design_list():
    """予定の一覧（行と、いまの行の印）"""
    ground = rrect(0.5, 0.5, 0.46, 0.46, 0.13)
    rows = [rrect(0.56, 0.28 + 0.15 * i, 0.22, 0.045, 0.03) for i in range(4)]
    dots = [circle(0.25, 0.28 + 0.15 * i, 0.045) for i in range(4)]
    mark = rrect(0.5, 0.43, 0.40, 0.07, 0.05)
    return ([(ground, gradient(BLUE0, BLUE1)), (mark, _hex("#3b8fe0"))] + [(r, WHITE) for r in rows]
            + [(d, AMBER if i == 1 else MINT) for i, d in enumerate(dots)])


def design_lines():
    """設備ごとの 3 本の線（L-1 青・L-2 緑・L-3 橙。画面の設備の色）と、時刻の目盛り。紺の地に明るい縁（紺の起動画面でも縁が見える）"""
    colors = (_hex("#60a5fa"), _hex("#4ade80"), _hex("#fb923c"))
    spans = (((0.16, 0.42), (0.54, 0.84)), ((0.26, 0.56), (0.68, 0.84)), ((0.16, 0.34), (0.46, 0.76)))
    bars = []
    for i, (row, c) in enumerate(zip(spans, colors)):
        y = 0.28 + 0.2 * i
        bars += [(rrect((a + b) / 2, y, (b - a) / 2, 0.06, 0.05), c) for a, b in row]
    ticks = [segment(0.18 + 0.16 * k, 0.86, 0.18 + 0.16 * k, 0.89, 0.012) for k in range(5)]
    return ([(rrect(0.5, 0.5, 0.46, 0.46, 0.13), _hex("#4f7db0")), (rrect(0.5, 0.5, 0.43, 0.43, 0.11), _hex("#0b2a4a"))]
            + bars + [(t, WHITE) for t in ticks])


DESIGNS = {"gantt": design_gantt, "calendar": design_calendar, "coil": design_coil, "list": design_list, "lines": design_lines}
DEFAULT = "gantt"


def render(size, design=DEFAULT):
    """RGBA のバイト列（上の行から）"""
    make = DESIGNS[design]
    layers = make(size) if "size" in make.__code__.co_varnames[:make.__code__.co_argcount] else make()
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
