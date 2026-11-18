#!/usr/bin/env python3
"""Draw the store artwork (logos, posters, promo banners) and resize the screenshots.
usage: make_assets.py SCREENSHOT_DIR   (needs Pillow; screenshots are 1920x1080 PNGs named NN-*.png)"""
import math, sys, pathlib
from PIL import Image, ImageDraw, ImageFont

HERE = pathlib.Path(__file__).parent
OUT = HERE / "msstore" / "images"
SNAP = HERE / "snap"
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
REG = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
BG, ORANGE, CYAN = (24, 27, 36), (255, 150, 40), (120, 220, 255)

def icon(n, rounded=True):
    """The app icon at n x n, drawn 4x larger and scaled down for smooth edges."""
    k = 4; N = n * k
    im = Image.new("RGBA", (N, N), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    if rounded: d.rounded_rectangle([0, 0, N - 1, N - 1], radius=N * 28 // 256 * 1, fill=(38, 43, 58, 255))
    else: d.rectangle([0, 0, N, N], fill=BG + (255,))
    c = N / 2; r = N * 78 / 256
    d.ellipse([c - r, c - r, c + r, c + r], outline=ORANGE + (255,), width=max(2, int(N * 16 / 256)))
    pts = []
    for i in range(int(N * 132 / 256) + 1):
        dx = i - N * 66 / 256
        w = 38 * N / 256 * math.sin((dx + 64 * N / 256) / (128 * N / 256) * 2 * math.pi * 1.5) * (1 - (dx / (70 * N / 256)) ** 2)
        pts.append((c + dx, c + w))
    rr = max(1.0, N * 5 / 256)
    for x, y in pts:
        d.ellipse([x - rr, y - rr, x + rr, y + rr], fill=CYAN + (255,))
    return im.resize((n, n), Image.LANCZOS)

def gradient(w, h):
    im = Image.new("RGB", (w, h), BG); px = im.load()
    for y in range(h):
        for x in range(w):
            t = (x / w * .4 + y / h * .6)
            px[x, y] = tuple(int(a + (b - a) * t) for a, b in zip((30, 34, 46), (14, 15, 22)))
    return im

def text(d, xy, s, size, fill, bold=False, anchor="la"):
    d.text(xy, s, font=ImageFont.truetype(BOLD if bold else REG, size), fill=fill, anchor=anchor)

def logos():
    icon(300).save(OUT / "logo-300x300.png"); icon(1080).save(OUT / "logo-1080x1080.png")
    # 2:3 poster
    p = gradient(720, 1080).convert("RGBA"); p.alpha_composite(icon(420), (150, 200)); d = ImageDraw.Draw(p)
    text(d, (360, 720), "guitar-fx", 84, (255, 255, 255), True, "mm")
    text(d, (360, 800), "Multi-effect guitar processor", 30, ORANGE, False, "mm")
    text(d, (360, 850), "Amps · cabinets · pedals · looper · tuner", 22, (170, 175, 190), False, "mm")
    p.convert("RGB").save(OUT / "poster-720x1080.png")
    icon(150).save(OUT / "logo-150x150.png"); icon(71).save(OUT / "logo-71x71.png")

def promo(shot, w, h, name):
    p = gradient(w, h).convert("RGBA"); s = h / 1080
    p.alpha_composite(icon(int(260 * s)), (int(90 * s), int(140 * s))); d = ImageDraw.Draw(p)
    text(d, (int(90 * s), int(480 * s)), "guitar-fx", int(120 * s), (255, 255, 255), True)
    text(d, (int(90 * s), int(630 * s)), "Build your guitar tone.", int(46 * s), ORANGE)
    text(d, (int(90 * s), int(700 * s)), "Amps, cabinets, pedals, looper, tuner.", int(34 * s), (170, 175, 190))
    sh = Image.open(shot).convert("RGBA"); tw = int(w * 0.50); th = int(tw * 1080 / 1920)
    sh = sh.resize((tw, th), Image.LANCZOS); x, y = w - tw - int(60 * s), (h - th) // 2
    d.rounded_rectangle([x - 6, y - 6, x + tw + 6, y + th + 6], radius=14, fill=(60, 66, 84, 255))
    p.alpha_composite(sh, (x, y)); p.convert("RGB").save(OUT / name)

def shots(src):
    for i, f in enumerate(sorted(pathlib.Path(src).glob("0*.png")), 1):
        im = Image.open(f).convert("RGB")
        im.save(OUT / f"screenshot{i}.png", optimize=True)
        im.save(SNAP / f"screenshot{i}.png", optimize=True)

if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True); SNAP.mkdir(exist_ok=True)
    src = pathlib.Path(sys.argv[1])
    shots(src); logos()
    first = sorted(src.glob("0*.png"))[0]
    promo(first, 1920, 1080, "promo-1920x1080.png"); promo(first, 2400, 1200, "promo-2400x1200.png")
    # snap store: icon 512, banner 3:1
    icon(512).save(SNAP / "icon-512.png")
    b = gradient(1920, 640).convert("RGBA"); b.alpha_composite(icon(360), (90, 140)); d = ImageDraw.Draw(b)
    text(d, (520, 270), "guitar-fx", 110, (255, 255, 255), True); text(d, (520, 400), "Multi-effect guitar processor", 44, ORANGE)
    b.convert("RGB").save(SNAP / "banner-1920x640.png")
