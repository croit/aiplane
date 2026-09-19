#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# Copyright (C) 2026 croit GmbH
#
# Draw the two promotional tiles the Chrome Web Store listing needs.
#
#   python3 extension/icons/promo.py
#
# The store asks for a 440×280 tile and a 1400×560 marquee, and rejects a
# listing without them. They are generated for the same reason the icons are:
# so a change to the name or the colour is one edit rather than a trip through
# an image editor nobody has installed.
#
# The mark itself comes from `render.py` — the tiles must not drift from the
# icon a user then sees in their toolbar.
from PIL import Image, ImageDraw, ImageFont

from render import BACKDROP, GRADIENT, icon

# Two lines, because one would have to be small enough to be unreadable at 440px.
TITLE_LINES = ("croit AIplane", "Browser Control")
TAGLINE = "Let a conversation work in your own browser"

# The store shows these tiles small and often next to text, so the type has to
# hold up at a glance. Candidates in order of preference; the last resort is
# Pillow's bitmap font, which is ugly but never absent.
FONTS = (
    "/System/Library/Fonts/Supplemental/Futura.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/SFNS.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
)


def font(size: int) -> ImageFont.ImageFont:
    for path in FONTS:
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            continue
    return ImageFont.load_default(size)


def fitted(draw, lines: tuple[str, ...], room: int, ceiling: int) -> ImageFont.ImageFont:
    """The largest size at which every line still fits the space there is.

    Sizing from the tile's height alone is what ran "Browser Control" off the
    edge of the 440px tile: the two tiles have the same aspect ratio, but the
    text does not scale with it — a font that fits 1400px wide does not fit
    440px just because both are 2.5:1.
    """
    for size in range(ceiling, 7, -1):
        candidate = font(size)
        if all(draw.textlength(line, font=candidate) <= room for line in lines):
            return candidate
    return font(8)


def tile(width: int, height: int) -> Image.Image:
    canvas = Image.new("RGB", (width, height), BACKDROP)
    draw = ImageDraw.Draw(canvas)

    # A wash of the accent across the bottom-right, so the tile is not a flat
    # rectangle next to everyone else's flat rectangle.
    glow = Image.new("RGB", (width, height), BACKDROP)
    glow_draw = ImageDraw.Draw(glow)
    for i in range(24):
        t = i / 23
        radius = int(height * (0.55 + t * 0.9))
        colour = tuple(
            round(BACKDROP[c] + (GRADIENT[0][c] - BACKDROP[c]) * 0.16 * (1 - t)) for c in range(3)
        )
        glow_draw.ellipse(
            [width - radius, height - radius // 2, width + radius, height + radius],
            fill=colour,
        )
    canvas = Image.blend(canvas, glow, 0.9)
    draw = ImageDraw.Draw(canvas)

    mark_size = int(height * 0.46)
    mark = icon(mark_size, True).convert("RGBA")
    margin = int(height * 0.16)
    gap = int(height * 0.11)
    room = width - margin * 2 - mark_size - gap

    first, second = TITLE_LINES
    title_font = fitted(draw, TITLE_LINES, room, int(height * 0.155))
    title_size = title_font.size
    line_gap = int(title_size * 0.22)

    # Wrap the tagline at the width there is, then size it so the longest line
    # fits too — the rendered width is what matters, not the character count.
    tag_font = font(int(height * 0.082))
    words = TAGLINE.split()
    line, lines = "", []
    for word in words:
        candidate = f"{line} {word}".strip()
        if draw.textlength(candidate, font=tag_font) <= room:
            line = candidate
        else:
            lines.append(line)
            line = word
    lines.append(line)
    tag_font = fitted(draw, tuple(lines), room, int(height * 0.082))
    tag_size = tag_font.size

    block = title_size * 2 + line_gap + int(tag_size * 1.1) + len(lines) * int(tag_size * 1.35)
    y = (height - block) // 2

    # Centre the mark and the text together. Laying them out from the left edge
    # left the marquee — two and a half times wider than the small tile for the
    # same words — with a third of itself empty on the right.
    text_width = max(
        draw.textlength(first, font=title_font),
        draw.textlength(second, font=title_font),
        *(draw.textlength(text, font=tag_font) for text in lines),
    )
    group = mark_size + gap + text_width
    mark_x = int((width - group) / 2)
    text_x = mark_x + mark_size + gap
    canvas.paste(mark, (mark_x, (height - mark_size) // 2), mark)

    draw.text((text_x, y), first, font=title_font, fill=(255, 255, 255))
    y += title_size + line_gap
    draw.text((text_x, y), second, font=title_font, fill=GRADIENT[1])
    y += title_size + int(tag_size * 1.1)

    for text in lines:
        draw.text((text_x, y), text, font=tag_font, fill=(168, 172, 180))
        y += int(tag_size * 1.35)

    return canvas


def main() -> None:
    here = __file__.rsplit("/", 1)[0]
    for width, height in ((440, 280), (1400, 560)):
        name = f"{here}/promo-{width}x{height}.png"
        tile(width, height).save(name)
        print(f"promo-{width}x{height}.png")


if __name__ == "__main__":
    main()
