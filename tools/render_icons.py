"""Render the web app's PNG icons from apps/web/icons/icon.svg.

    python3 tools/render_icons.py apps/web/icons

writes icon-192.png, icon-512.png and icon-maskable-512.png (the PNG sizes
the manifest, the service worker and iOS's apple-touch-icon ask for). The
SVG is the source: this draws the same shapes at 8x and scales down, and
matches a browser's rendering of the SVG to within antialiasing. The
maskable icon fills the whole square and draws at 80% so it stays inside
the safe circle launchers crop to. Needs Pillow; not part of the build.
Change icon.svg and this together.
"""
import sys
from PIL import Image, ImageDraw

BG = (0x0d, 0x10, 0x13, 255)
WHITE = (255, 255, 255, 255)
YELLOW = (0xff, 0xe1, 0x4d, 255)
GREEN = (0x3d, 0xdc, 0x84, 255)
SS = 8  # supersampling factor

def render(size, maskable=False):
    n = size * SS
    k = n / 512.0
    im = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    # Maskable icons fill the square and keep the drawing inside the safe circle.
    s = 0.8 if maskable else 1.0
    def P(x, y):
        return ((256 + (x - 256) * s) * k, (256 + (y - 256) * s) * k)
    def L(v):
        return v * s * k
    if maskable:
        d.rectangle([0, 0, n, n], fill=BG)
    else:
        d.rounded_rectangle([0, 0, n - 1, n - 1], radius=96 * k, fill=BG)
    # White rectangle, stroke 22, round joins.
    a, b = P(112 - 11, 152 - 11), P(400 + 11, 360 + 11)
    d.rounded_rectangle([a[0], a[1], b[0], b[1]], radius=L(11), fill=WHITE)
    a, b = P(112 + 11, 152 + 11), P(400 - 11, 360 - 11)
    d.rectangle([a[0], a[1], b[0], b[1]], fill=BG)
    # White circle, stroke 22.
    c = P(256, 256)
    for r, col in ((58 + 11, WHITE), (58 - 11, BG)):
        d.ellipse([c[0] - L(r), c[1] - L(r), c[0] + L(r), c[1] + L(r)], fill=col)
    def seg(x1, y1, x2, y2, w, col):
        # A straight stroke with round caps.
        (ax, ay), (bx, by) = P(x1, y1), P(x2, y2)
        h = L(w / 2)
        if ax == bx:
            d.rectangle([ax - h, min(ay, by), ax + h, max(ay, by)], fill=col)
        else:
            d.rectangle([min(ax, bx), ay - h, max(ax, bx), ay + h], fill=col)
        for (x, y) in ((ax, ay), (bx, by)):
            d.ellipse([x - h, y - h, x + h, y + h], fill=col)
    def dashed(x1, y1, x2, y2, w, col, pattern=(44, 16, 8, 16)):
        length = abs(x2 - x1) + abs(y2 - y1)
        ux, uy = (x2 - x1) / length, (y2 - y1) / length
        t, i = 0.0, 0
        while t < length:
            on = pattern[i % len(pattern)]
            if i % 2 == 0:
                e = min(t + on, length)
                seg(x1 + ux * t, y1 + uy * t, x1 + ux * e, y1 + uy * e, w, col)
            t += on
            i += 1
    dashed(256, 112, 256, 400, 10, YELLOW)
    dashed(84, 256, 428, 256, 10, YELLOW)
    seg(112, 412, 400, 412, 10, GREEN)
    seg(112, 396, 112, 428, 10, GREEN)
    seg(400, 396, 400, 428, 10, GREEN)
    return im.convert("RGBa").resize((size, size), Image.Resampling.BOX).convert("RGBA")

if __name__ == "__main__":
    out = sys.argv[1]
    render(192).save(f"{out}/icon-192.png", optimize=True)
    render(512).save(f"{out}/icon-512.png", optimize=True)
    render(512, maskable=True).save(f"{out}/icon-maskable-512.png", optimize=True)
