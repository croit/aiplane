#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# Copyright (C) 2026 croit GmbH
#
# Draw the toolbar icons, in both states, at every size Chrome asks for.
#
# Committed rather than run once and forgotten, because the alternative is a
# set of PNGs nobody can change: a tweak to the colour or the corner radius
# would mean redrawing eight files by hand and hoping they still match.
#
#   python3 extension/icons/render.py
#
# The shape is the gateway's own mark (web/static/favicon.svg): a diamond on a
# dark rounded square with a node at its centre. Keeping the family visible
# matters more than inventing something — this is the same product, and the
# icon sits in a toolbar next to a dozen others.
#
# Only the *colour* changes between on and off, never the silhouette. A user
# learns one shape and reads its state at a glance; a different drawing per
# state would make them read the icon twice. The badge spells the state out in
# words besides, for anyone who cannot rely on the colour.
#
# Needs Pillow. It is not in [tools]: this runs when the artwork changes, which
# is roughly never, and adding an image library to every clone and CI run to
# redraw eight static files would be the wrong trade.
from PIL import Image, ImageDraw

SIZES = (16, 32, 48, 128)
# Supersampling factor. The diamond is all diagonals, and at 16px an aliased
# edge is the difference between a mark and a smudge.
SS = 8

BACKDROP = (29, 29, 27)  # #1D1D1B, the favicon's square
GRADIENT = ((142, 84, 233), (252, 166, 140))  # #8E54E9 → #FCA68C
OFF_FILL = (107, 114, 128)  # #6b7280 — quiet, but still readable on a dark toolbar
ON_NODE = (255, 255, 255)
OFF_NODE = (209, 213, 219)  # #d1d5db — the node has to read against OFF_FILL

# Proportions, all relative to the canvas, taken from the favicon's 32px grid.
CORNER = 0.25  # rx="8" of 32
INSET = 0.117  # the diamond spans 3.75 … 28.25
NODE = 0.135


def gradient(size: int) -> Image.Image:
    """The favicon's accent, bottom-left to top-right.

    Built small and scaled up: a gradient is smooth by definition, so computing
    a million pixels in Python buys nothing over interpolating a thousand.
    """
    small = Image.new("RGB", (64, 64))
    pixels = small.load()
    (r0, g0, b0), (r1, g1, b1) = GRADIENT
    for y in range(64):
        for x in range(64):
            t = (x / 63 + (1 - y / 63)) / 2
            pixels[x, y] = (
                round(r0 + (r1 - r0) * t),
                round(g0 + (g1 - g0) * t),
                round(b0 + (b1 - b0) * t),
            )
    return small.resize((size, size), Image.BICUBIC)


def icon(size: int, armed: bool) -> Image.Image:
    w = size * SS
    canvas = Image.new("RGBA", (w, w), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle([0, 0, w - 1, w - 1], radius=w * CORNER, fill=BACKDROP + (255,))

    inset = w * INSET
    centre = w / 2
    points = [(centre, inset), (w - inset, centre), (centre, w - inset), (inset, centre)]

    if armed:
        # Paint the gradient through a diamond-shaped hole rather than filling
        # a polygon with one colour — the accent is what makes it the product's
        # icon and not a generic shape.
        mask = Image.new("L", (w, w), 0)
        ImageDraw.Draw(mask).polygon(points, fill=255)
        canvas.paste(gradient(w), (0, 0), mask)
    else:
        draw.polygon(points, fill=OFF_FILL + (255,))

    node = w * NODE
    draw.ellipse(
        [centre - node, centre - node, centre + node, centre + node],
        fill=(ON_NODE if armed else OFF_NODE) + (255,),
    )
    return canvas.resize((size, size), Image.LANCZOS)


def main() -> None:
    here = __file__.rsplit("/", 1)[0]
    for size in SIZES:
        for armed, name in ((True, "on"), (False, "off")):
            icon(size, armed).save(f"{here}/{name}-{size}.png")
            print(f"{name}-{size}.png")


if __name__ == "__main__":
    main()
