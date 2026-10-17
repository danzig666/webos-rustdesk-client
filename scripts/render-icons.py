#!/usr/bin/env python3
"""Render the webOS launcher icons from the icon geometry (same as packaging/webos/icon.svg).

Usage: render-icons.py <out-dir>   -> icon.png (80x80), icon_large.png (130x130), icon_512.png
"""
import math
import sys

from PIL import Image, ImageDraw

S = 4  # supersampling factor
W = 1254
BLUE = (0, 111, 255, 255)
WHITE = (255, 255, 255, 255)
CYAN = (92, 225, 250, 255)


def render(size):
    k = size * S / W
    img = Image.new("RGBA", (size * S, size * S), BLUE)
    d = ImageDraw.Draw(img)

    def sc(*v):
        return [x * k for x in v]

    d.rounded_rectangle(sc(146, 260, 1108, 938), radius=97 * k, fill=WHITE)
    d.rounded_rectangle(sc(208, 322, 1046, 876), radius=35 * k, fill=BLUE)
    d.rectangle(sc(582, 930, 671, 990), fill=WHITE)
    d.rounded_rectangle(sc(372, 983, 882, 1036), radius=26.5 * k, fill=WHITE)
    cx, cy, r, w = 627, 612, 200, 92
    for start, end, col in ((143, 292, WHITE), (323, 472, CYAN)):
        d.arc(sc(cx - r - w / 2, cy - r - w / 2, cx + r + w / 2, cy + r + w / 2),
              start, end, fill=col, width=int(w * k))
        for a in (start, end):
            x = cx + r * math.cos(math.radians(a))
            y = cy + r * math.sin(math.radians(a))
            d.ellipse(sc(x - w / 2, y - w / 2, x + w / 2, y + w / 2), fill=col)
    return img.resize((size, size), Image.LANCZOS)


def main():
    out = sys.argv[1]
    render(80).save(f"{out}/icon.png")
    render(130).save(f"{out}/icon_large.png")
    render(512).save(f"{out}/icon_512.png")


if __name__ == "__main__":
    main()
