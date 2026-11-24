"""Generates the Dark Clipboard icons (option A, D-078). Dev-only; the gate does not run it.

    python make_icon.py [preview_dir]

Needs Pillow. Writes next to this file:
  dark-clipboard.ico   16 20 24 32 40 48 64 256 (256 = PNG, others = 32-bit DIB)
  window-64.png        the window icon, source picture
  window-64.rgba       the same, raw RGBA, embedded in the exe (Task 711: a PNG in `.slint` pulls in
                       the image decoders, +116 KiB)
  tray/<state>-<size>.png and .rgba   states normal, paused, private, unavailable; sizes 16 20 24 32
  preview.png          contact sheet (light and dark backgrounds, zoomed and real size)
Sizes 16/20/24 are drawn on the pixel grid (explicit pixel rects); the others are drawn at 8x and
reduced. Runs self_check at the end and exits non-zero when something is wrong.
"""

import io
import os
import struct
import sys

from PIL import Image, ImageDraw

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
TRAY_SIZES = [16, 20, 24, 32]
STATES = ["normal", "paused", "private", "unavailable"]
GRID = (16, 20, 24)
SS = 8

TILE = (0x20, 0x23, 0x28)
BORDER = (0x3A, 0x3E, 0x45)
ACCENT = (0x78, 0x87, 0xFF)
LIGHT = (0xF3, 0xF5, 0xF9)
RING = (0x11, 0x13, 0x18)
BADGE = {
    "paused": (0xF2, 0xC1, 0x4E),
    "private": (0xA1, 0xAA, 0xB8),
    "unavailable": (0xFF, 0x7A, 0x85),
}


def rgba(c, a=255):
    return c + (a,)


# ---------------------------------------------------------------- smooth rendering (>= 32 px)

class Canvas:
    """Draws the 56-unit view box at size*SS pixels."""

    def __init__(self, size):
        self.size = size
        self.k = size * SS / 56.0
        self.img = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))

    def _layer(self, draw_fn, colour):
        mask = Image.new("L", self.img.size, 0)
        draw_fn(ImageDraw.Draw(mask))
        layer = Image.new("RGBA", self.img.size, rgba(colour))
        layer.putalpha(mask)
        self.img.alpha_composite(layer)

    def rrect(self, x, y, w, h, r, colour):
        k = self.k
        self._layer(lambda d: d.rounded_rectangle((x * k, y * k, (x + w) * k - 1, (y + h) * k - 1), radius=r * k, fill=255), colour)

    def rrect_stroke(self, x, y, w, h, r, sw, colour):
        k = self.k
        h2 = sw / 2.0

        def f(d):
            d.rounded_rectangle(((x - h2) * k, (y - h2) * k, (x + w + h2) * k - 1, (y + h + h2) * k - 1), radius=(r + h2) * k, fill=255)
            d.rounded_rectangle(((x + h2) * k, (y + h2) * k, (x + w - h2) * k - 1, (y + h - h2) * k - 1), radius=max(r - h2, 0) * k, fill=0)

        self._layer(f, colour)

    def circle(self, cx, cy, r, colour):
        k = self.k
        self._layer(lambda d: d.ellipse(((cx - r) * k, (cy - r) * k, (cx + r) * k - 1, (cy + r) * k - 1), fill=255), colour)

    def line(self, pts, sw, colour):
        k = self.k

        def f(d):
            xy = [(x * k, y * k) for x, y in pts]
            d.line(xy, fill=255, width=round(sw * k), joint="curve")
            rr = sw * k / 2
            for x, y in xy:
                d.ellipse((x - rr, y - rr, x + rr, y + rr), fill=255)

        self._layer(f, colour)

    def polygon(self, pts, colour):
        k = self.k
        self._layer(lambda d: d.polygon([(x * k, y * k) for x, y in pts], fill=255), colour)

    def done(self):
        return self.img.resize((self.size, self.size), Image.LANCZOS)


def cubic(p0, p1, p2, p3, n=24):
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        out.append(tuple(u ** 3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t ** 3 * d for a, b, c, d in zip(p0, p1, p2, p3)))
    return out


def draw_badge_smooth(cv, state):
    col = BADGE[state]
    cv.circle(43, 43, 13.5, RING)
    cv.circle(43, 43, 10.5, col)
    if state == "paused":
        cv.line([(39.5, 37.5), (39.5, 48.5)], 3.6, RING)
        cv.line([(46.5, 37.5), (46.5, 48.5)], 3.6, RING)
    elif state == "private":
        up = cubic((36, 43), (38.5, 39), (47.5, 39), (50, 43))
        down = cubic((50, 43), (47.5, 47), (38.5, 47), (36, 43))
        cv.polygon(up + down, RING)
        cv.line([(35, 36), (51, 50)], 3, RING)
    else:
        cv.line([(43, 36.5), (43, 44.5)], 3.6, RING)
        cv.line([(43, 49.5), (43, 49.6)], 3.6, RING)


def smooth(size, state="normal"):
    cv = Canvas(size)
    cv.rrect(1.4, 1.4, 53.2, 53.2, 13.6, BORDER)  # border: stroke 1.2 centred on the 2..54 box
    cv.rrect(2.6, 2.6, 50.8, 50.8, 12.4, TILE)
    cv.rrect_stroke(16, 15, 24, 30, 4.5, 4, ACCENT)
    cv.rrect(21.5, 10.5, 13, 8, 2.5, LIGHT)
    cv.line([(22, 28), (34, 28)], 3.5, LIGHT)
    cv.line([(22, 35), (30, 35)], 3.5, LIGHT)
    if state != "normal":
        draw_badge_smooth(cv, state)
    return cv.done()


# ---------------------------------------------------------------- pixel-grid rendering (16/20/24)

# Per size: tile corner radius, board (x0, y0, x1, y1 inclusive, 1 px outline), clip rect and
# line rects. All measured by hand on the pixel grid.
GRID_SPEC = {
    16: dict(r=3, board=(4, 4, 11, 13), clip=(6, 3, 9, 5), lines=[(6, 8, 9, 8), (6, 10, 8, 10)]),
    20: dict(r=4, board=(5, 5, 14, 16), clip=(7, 3, 12, 6), lines=[(7, 10, 12, 10), (7, 12, 10, 12)]),
    24: dict(r=5, board=(7, 7, 16, 19), clip=(9, 4, 14, 7), lines=[(9, 12, 14, 12), (9, 14, 12, 14)]),
}

# Badge on the grid: disc centre pixel and radius, and marks as (dx, dy) offsets from the centre.
BADGE_GRID = {
    16: dict(c=(12, 12), r=3.3, ring=1.0,
             paused=[(-1, y) for y in range(-2, 3)] + [(1, y) for y in range(-2, 3)],
             private=[(-2, 0), (-1, -1), (0, -1), (1, -1), (2, 0), (-1, 1), (0, 1), (1, 1), (-2, -2), (-1, -1), (0, 0), (1, 1), (2, 2)],
             unavailable=[(0, -2), (0, -1), (0, 0), (0, 2)]),
    20: dict(c=(15, 15), r=4.2, ring=1.0,
             paused=[(-1, y) for y in range(-2, 3)] + [(1, y) for y in range(-2, 3)],
             private=[(-3, 0), (-2, -1), (-1, -1), (0, -1), (1, -1), (2, -1), (3, 0), (-2, 1), (-1, 1), (0, 1), (1, 1), (2, 1), (-3, -3), (-2, -2), (-1, -1), (0, 0), (1, 1), (2, 2), (3, 3)],
             unavailable=[(0, -3), (0, -2), (0, -1), (0, 0), (0, 2)]),
    24: dict(c=(18, 18), r=5.0, ring=1.0,
             paused=[(dx, y) for dx in (-2, -1, 1, 2) for y in range(-3, 4)],
             private=[(-4, 0), (-3, -1), (-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (3, -1), (4, 0), (-3, 1), (-2, 2), (-1, 2), (0, 2), (1, 2), (2, 2), (3, 1), (-4, -4), (-3, -3), (-2, -2), (-1, -1), (0, 0), (1, 1), (2, 2), (3, 3), (4, 4)],
             unavailable=[(dx, y) for dx in (-1, 0) for y in range(-4, 1)] + [(dx, 3) for dx in (-1, 0)]),
}


def grid(size, state="normal"):
    sp = GRID_SPEC[size]
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    r = sp["r"]

    def inside(x, y, off):
        lo, hi = off, size - off
        rad = max(r - off, 0)
        cx = min(max(x + 0.5, lo + rad), hi - rad)
        cy = min(max(y + 0.5, lo + rad), hi - rad)
        return lo <= x + 0.5 <= hi and lo <= y + 0.5 <= hi and (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2 <= rad * rad + 0.01

    for y in range(size):
        for x in range(size):
            if inside(x, y, 1):
                px[x, y] = rgba(TILE)
            elif inside(x, y, 0):
                px[x, y] = rgba(BORDER)

    def rect(x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                px[x, y] = rgba(c)

    bx0, by0, bx1, by1 = sp["board"]
    rect(bx0, by0, bx1, by0, ACCENT)
    rect(bx0, by1, bx1, by1, ACCENT)
    rect(bx0, by0, bx0, by1, ACCENT)
    rect(bx1, by0, bx1, by1, ACCENT)
    rect(*sp["clip"], LIGHT)
    for ln in sp["lines"]:
        rect(*ln, LIGHT)
    if state != "normal":
        b = BADGE_GRID[size]
        cx, cy = b["c"]
        for y in range(size):
            for x in range(size):
                d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
                if d <= b["r"]:
                    px[x, y] = rgba(BADGE[state])
                elif d <= b["r"] + b["ring"]:
                    px[x, y] = rgba(RING)
        for dx, dy in b[state]:
            x, y = cx + dx, cy + dy
            if 0 <= x < size and 0 <= y < size and ((dx * dx + dy * dy) ** 0.5) <= b["r"]:
                px[x, y] = rgba(RING)
    return img


def make(size, state="normal"):
    return grid(size, state) if size in GRID else smooth(size, state)


# ---------------------------------------------------------------- ICO

def dib(img):
    w, h = img.size
    pix = img.load()
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4, 0, 0, 0, 0)
    rows = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = pix[x, y]
            rows += bytes((b, g, r, a))
    mask_stride = ((w + 31) // 32) * 4
    return header + bytes(rows) + bytes(mask_stride * h)


def png_bytes(img):
    buf = io.BytesIO()
    img.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def build_ico(images):
    blobs = [png_bytes(im) if s >= 256 else dib(im) for s, im in images]
    out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for (s, _), blob in zip(images, blobs):
        b = 0 if s >= 256 else s
        out += struct.pack("<BBBBHHII", b, b, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    for blob in blobs:
        out += blob
    return bytes(out)


# ---------------------------------------------------------------- preview

def preview(base, tray, path):
    cell, pad = 144, 10
    width = pad + (cell + pad) * len(base)
    band = 128 + 60 + 3 * pad
    state_band = 4 * (64 + pad) + pad
    total = 2 * (band + state_band)
    out = Image.new("RGBA", (width, total), (0, 0, 0, 255))
    d = ImageDraw.Draw(out)
    y0 = 0
    for bg in ((0xF3, 0xF4, 0xF6, 255), (0x1F, 0x29, 0x37, 255)):
        d.rectangle((0, y0, width, y0 + band), fill=bg)
        x = pad
        for s, im in base:
            out.alpha_composite(im.resize((128, 128), Image.NEAREST), (x + (cell - 128) // 2, y0 + pad))
            real = im if s <= 64 else im.resize((48, 48), Image.LANCZOS)
            out.alpha_composite(real, (x + (cell - real.width) // 2, y0 + 2 * pad + 128))
            x += cell + pad
        y0 += band
        d.rectangle((0, y0, width, y0 + state_band), fill=bg)
        yy = y0 + pad
        for st in STATES:
            x = pad
            for s in TRAY_SIZES:
                im = tray[(st, s)]
                out.alpha_composite(im.resize((64, 64), Image.NEAREST), (x, yy))
                out.alpha_composite(im, (x + 72, yy + (64 - s) // 2))
                x += 72 + 40 + pad
            yy += 64 + pad
        y0 += state_band
    out.convert("RGB").save(path)


# ---------------------------------------------------------------- self check

def parse_ico(data):
    assert data[:4] == b"\x00\x00\x01\x00", "ICO header"
    n = struct.unpack_from("<H", data, 4)[0]
    entries = []
    for i in range(n):
        w, h, _c, _r, planes, bpp, ln, off = struct.unpack_from("<BBBBHHII", data, 6 + 16 * i)
        entries.append((w or 256, h or 256, planes, bpp, ln, off))
    return entries


def near(a, b, tol=6):
    return all(abs(x - y) <= tol for x, y in zip(a[:3], b))


def self_check(ico_data, tray):
    entries = parse_ico(ico_data)
    assert [e[0] for e in entries] == ICO_SIZES, f"ICO sizes {[e[0] for e in entries]}"
    for w, h, planes, bpp, ln, off in entries:
        assert w == h and planes == 1 and bpp == 32, f"entry {w}: planes/bpp"
        assert off + ln <= len(ico_data), f"entry {w}: out of range"
        blob = ico_data[off:off + ln]
        if w == 256:
            assert blob[:8] == b"\x89PNG\r\n\x1a\n", "256 entry is not PNG"
        else:
            hs, bw, bh, _pl, bbp = struct.unpack_from("<IiiHH", blob, 0)
            assert (hs, bw, bh, bbp) == (40, w, h * 2, 32), f"entry {w}: not a 32-bit DIB"
            alpha = [blob[40 + (y * w + x) * 4 + 3] for y in range(h) for x in range(w)]
            assert max(alpha) == 255 and sum(1 for a in alpha if a) > w * h // 2, f"entry {w}: alpha"
    for (st, s), im in tray.items():
        assert im.size == (s, s)
        px = im.load()
        opaque = sum(1 for y in range(s) for x in range(s) if px[x, y][3] == 255)
        assert opaque > s * s // 2, f"{st} {s}: mostly empty"

        def found(col):
            return any(near(px[x, y], col) for y in range(s) for x in range(s) if px[x, y][3] == 255)

        assert found(ACCENT), f"{st} {s}: no accent colour"
        if st == "normal":
            for b in BADGE.values():
                assert not found(b), f"normal {s}: has a badge colour"
        else:
            assert found(BADGE[st]), f"{st} {s}: badge colour missing"
            for other, b in BADGE.items():
                if other != st:
                    assert not found(b), f"{st} {s}: shows the {other} colour"


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    tray_dir = os.path.join(here, "tray")
    os.makedirs(tray_dir, exist_ok=True)
    base = [(s, make(s)) for s in ICO_SIZES]
    tray = {(st, s): make(s, st) for st in STATES for s in TRAY_SIZES}
    ico = build_ico(base)
    with open(os.path.join(here, "dark-clipboard.ico"), "wb") as f:
        f.write(ico)
    dict(base)[64].save(os.path.join(here, "window-64.png"), optimize=True)
    with open(os.path.join(here, "window-64.rgba"), "wb") as f:
        f.write(dict(base)[64].convert("RGBA").tobytes())
    for (st, s), im in tray.items():
        im.save(os.path.join(tray_dir, f"{st}-{s}.png"), optimize=True)
        with open(os.path.join(tray_dir, f"{st}-{s}.rgba"), "wb") as f:
            f.write(im.tobytes())
    out_dir = sys.argv[1] if len(sys.argv) > 1 else here
    os.makedirs(out_dir, exist_ok=True)
    preview(base, tray, os.path.join(out_dir, "preview.png"))
    try:
        with open(os.path.join(here, "dark-clipboard.ico"), "rb") as f:
            self_check(f.read(), tray)
    except AssertionError as e:
        print("SELF CHECK FAILED:", e)
        sys.exit(1)
    print(f"self_check ok: ico {len(ico)} bytes, sizes {ICO_SIZES}, tray {len(tray)} images")


if __name__ == "__main__":
    main()
