#!/usr/bin/env python3
"""Generate app icon: waveform morphing into text lines."""

from PIL import Image, ImageDraw
import math
import os
import subprocess
import shutil

SIZE = 1024


def draw_rounded_rect(draw, xy, radius, fill):
    x0, y0, x1, y1 = xy
    draw.rectangle([x0 + radius, y0, x1 - radius, y1], fill=fill)
    draw.rectangle([x0, y0 + radius, x1, y1 - radius], fill=fill)
    draw.pieslice([x0, y0, x0 + 2 * radius, y0 + 2 * radius], 180, 270, fill=fill)
    draw.pieslice([x1 - 2 * radius, y0, x1, y0 + 2 * radius], 270, 360, fill=fill)
    draw.pieslice([x0, y1 - 2 * radius, x0 + 2 * radius, y1], 90, 180, fill=fill)
    draw.pieslice([x1 - 2 * radius, y1 - 2 * radius, x1, y1], 0, 90, fill=fill)


def lerp_color(c1, c2, t):
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(len(c1)))


def make_layer(size):
    return Image.new("RGBA", (size, size), (0, 0, 0, 0))


def create_gradient_bg(size):
    img = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    draw = ImageDraw.Draw(img)
    color_top = (30, 60, 180)
    color_bottom = (100, 40, 200)
    for y in range(size):
        t = y / size
        c = lerp_color(color_top, color_bottom, t)
        draw.line([(0, y), (size - 1, y)], fill=(*c, 255))
    return img


def create_glow_layer(size):
    layer = make_layer(size)
    draw = ImageDraw.Draw(layer)
    cx, cy = int(size * 0.35), int(size * 0.45)
    r = int(size * 0.35)
    for i in range(r, 0, -1):
        alpha = int(25 * (i / r))
        draw.ellipse([cx - i, cy - i, cx + i, cy + i], fill=(180, 200, 255, alpha))
    return layer


def create_waveform_layer(size):
    layer = make_layer(size)
    draw = ImageDraw.Draw(layer)

    margin_x = int(size * 0.15)
    margin_top = int(size * 0.20)
    margin_bottom = int(size * 0.20)
    content_w = size - 2 * margin_x
    content_h = size - margin_top - margin_bottom

    num_elements = 9
    gap = content_w / num_elements
    bar_w = int(gap * 0.45)
    bar_r = bar_w // 2

    wave_heights = []
    for i in range(num_elements):
        t = i / (num_elements - 1)
        wave = math.sin(t * math.pi * 1.8 + 0.3) * 0.7 + 0.3
        wave *= 1.0 - t * 0.6
        wave_heights.append(wave)

    line_count = 4
    line_h = int(size * 0.028)
    line_spacing = content_h / (line_count + 1)
    transition_start = 4
    transition_end = 6

    for i in range(num_elements):
        cx = margin_x + int(gap * (i + 0.5))

        if i <= transition_start:
            morph = 0.0
        elif i >= transition_end:
            morph = 1.0
        else:
            morph = (i - transition_start) / (transition_end - transition_start)

        alpha = int(255 * (0.85 + 0.15 * (1 - morph)))

        if morph < 1.0:
            h = int(content_h * wave_heights[i] * (1 - morph * 0.5))
            h = max(h, bar_w)
            cy = margin_top + content_h // 2
            y0 = cy - h // 2
            y1 = cy + h // 2
            draw_rounded_rect(draw, (cx - bar_w // 2, y0, cx + bar_w // 2, y1), bar_r, (255, 255, 255, alpha))

        if morph > 0.0:
            line_alpha = int(alpha * morph)
            segment_w = int(gap * 0.7)
            for li in range(line_count):
                ly = margin_top + int(line_spacing * (li + 1))
                lx0 = cx - segment_w // 2
                lx1 = cx + segment_w // 2
                draw_rounded_rect(draw, (lx0, ly - line_h // 2, lx1, ly + line_h // 2), line_h // 2, (255, 255, 255, line_alpha))

    return layer


def create_mic_layer(size):
    layer = make_layer(size)
    draw = ImageDraw.Draw(layer)

    cx = int(size * 0.18)
    cy = int(size * 0.22)
    mic_w = int(size * 0.045)
    mic_h = int(size * 0.07)
    r = mic_w
    color = (255, 255, 255, 120)

    draw_rounded_rect(draw, (cx - mic_w, cy - mic_h, cx + mic_w, cy + mic_h), r, color)

    arc_r = int(size * 0.06)
    arc_w = 3
    for angle_offset in range(-40, 41, 1):
        rad = math.radians(angle_offset - 90)
        x = cx + int(arc_r * math.cos(rad))
        y = cy + int(mic_h * 0.3) + int(arc_r * math.sin(rad))
        if y > cy + mic_h * 0.2:
            draw.ellipse([x - arc_w, y - arc_w, x + arc_w, y + arc_w], fill=color)

    stem_top = cy + mic_h + int(size * 0.02)
    stem_bottom = stem_top + int(size * 0.03)
    stem_w = 3
    draw.rectangle([cx - stem_w, stem_top, cx + stem_w, stem_bottom], fill=color)

    return layer


def main():
    project_dir = os.path.dirname(os.path.abspath(__file__))

    img = create_gradient_bg(SIZE)
    img = Image.alpha_composite(img, create_glow_layer(SIZE))
    img = Image.alpha_composite(img, create_waveform_layer(SIZE))
    img = Image.alpha_composite(img, create_mic_layer(SIZE))

    img = img.convert("RGB")

    master_path = os.path.join(project_dir, "icon_1024.png")
    img.save(master_path, "PNG")
    print(f"Saved master: {master_path}")

    iconset_dir = os.path.join(project_dir, "AppIcon.iconset")
    if os.path.exists(iconset_dir):
        shutil.rmtree(iconset_dir)
    os.makedirs(iconset_dir)

    sizes = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ]

    for name, sz in sizes:
        resized = img.resize((sz, sz), Image.LANCZOS)
        resized.save(os.path.join(iconset_dir, name), "PNG")

    icns_path = os.path.join(project_dir, "AppIcon.icns")
    subprocess.run(["iconutil", "-c", "icns", iconset_dir, "-o", icns_path], check=True)
    print(f"Saved icns: {icns_path}")

    shutil.rmtree(iconset_dir)
    print("Cleaned up iconset directory")


if __name__ == "__main__":
    main()
