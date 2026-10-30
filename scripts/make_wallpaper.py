"""
make_wallpaper.py - generates the original warm abstract wallpaper.

Entirely original output: a handful of soft radial amber/orange blobs over a warm
gradient, blurred heavily and finished with a little film grain so the gradients do not
band on an 8-bit display. No Apple assets, no stock imagery, no external downloads.

    python scripts/make_wallpaper.py                 # 1920x1080 light + dark
    python scripts/make_wallpaper.py --width 2560 --height 1440

Output: assets/wallpaper/macui-warm-{light,dark}-WxH.png

Nothing here changes the user's current wallpaper. The app only offers these in
settings.
"""

import argparse
import math
import pathlib

import numpy as np
from PIL import Image, ImageFilter

# Each blob: (cx, cy, radius, (r,g,b), strength) with cx/cy/radius as fractions of the
# frame, so the same layout renders correctly at any resolution.
LIGHT_BG = (250, 238, 224)
LIGHT_BLOBS = [
    (0.18, 0.22, 0.46, (255, 206, 150), 0.95),
    (0.78, 0.16, 0.40, (252, 180, 132), 0.80),
    (0.62, 0.74, 0.52, (255, 222, 180), 0.85),
    (0.08, 0.86, 0.38, (247, 186, 148), 0.70),
    (0.44, 0.44, 0.34, (255, 240, 214), 0.60),
    (0.92, 0.58, 0.30, (240, 166, 124), 0.65),
]

DARK_BG = (24, 18, 16)
DARK_BLOBS = [
    (0.18, 0.22, 0.46, (150, 92, 48), 0.85),
    (0.78, 0.16, 0.40, (120, 66, 40), 0.75),
    (0.62, 0.74, 0.52, (100, 58, 36), 0.80),
    (0.08, 0.86, 0.38, (132, 74, 44), 0.60),
    (0.44, 0.44, 0.34, (78, 48, 32), 0.65),
    (0.92, 0.58, 0.30, (166, 102, 52), 0.50),
]


def render(width, height, bg, blobs, seed=7):
    """Accumulate soft radial blobs additively over a warm vertical gradient."""
    # Work at a quarter size: every element is low-frequency, so this is visually
    # identical after the upscale and roughly 16x less arithmetic.
    w, h = max(1, width // 4), max(1, height // 4)

    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    xn, yn = xx / w, yy / h

    # Base: slight vertical warm-to-cool shift so the frame is never flat.
    base = np.array(bg, dtype=np.float32)
    top = base * 1.04
    bottom = base * 0.93
    canvas = top[None, None, :] * (1.0 - yn[..., None]) + bottom[None, None, :] * yn[..., None]

    aspect = w / h
    for cx, cy, radius, colour, strength in blobs:
        # Distance in aspect-corrected space keeps blobs circular, not stretched.
        dx = (xn - cx) * aspect
        dy = yn - cy
        d = np.sqrt(dx * dx + dy * dy) / (radius * aspect)
        # smootherstep falloff - no hard rim, no visible cutoff circle.
        t = np.clip(1.0 - d, 0.0, 1.0)
        falloff = t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
        # Blend toward the blob colour rather than adding to it. Additive blending
        # drove the light variant to 68% pure white; lerp cannot clip by construction,
        # so the warm hue survives wherever blobs overlap.
        a = (falloff * strength * 0.75)[..., None]
        canvas = canvas * (1.0 - a) + np.array(colour, dtype=np.float32)[None, None, :] * a

    canvas = np.clip(canvas, 0, 255).astype(np.uint8)
    img = Image.fromarray(canvas, "RGB").resize((width, height), Image.LANCZOS)
    img = img.filter(ImageFilter.GaussianBlur(radius=max(width, height) / 220.0))

    # Fine grain: breaks up banding in the smooth gradients on 8-bit panels.
    rng = np.random.default_rng(seed)
    arr = np.asarray(img, dtype=np.int16)
    noise = rng.normal(0.0, 2.1, arr.shape)
    arr = np.clip(arr + noise, 0, 255).astype(np.uint8)
    return Image.fromarray(arr, "RGB")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--width", type=int, default=1920)
    ap.add_argument("--height", type=int, default=1080)
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    out_dir = pathlib.Path(args.out) if args.out else pathlib.Path(__file__).parent.parent / "assets" / "wallpaper"
    out_dir.mkdir(parents=True, exist_ok=True)

    for name, bg, blobs in (("light", LIGHT_BG, LIGHT_BLOBS), ("dark", DARK_BG, DARK_BLOBS)):
        img = render(args.width, args.height, bg, blobs)
        path = out_dir / f"macui-warm-{name}-{args.width}x{args.height}.png"
        img.save(path, "PNG", optimize=True)
        print(f"{path}  ({path.stat().st_size // 1024} KB)")


def _self_check():
    """Smallest thing that fails if the renderer breaks."""
    img = render(160, 90, LIGHT_BG, LIGHT_BLOBS)
    assert img.size == (160, 90), img.size
    a = np.asarray(img)
    # Warm means red channel dominates blue, everywhere.
    assert a[..., 0].mean() > a[..., 2].mean() + 15, "output is not warm"
    # Not a flat fill: the blobs must actually vary the frame.
    assert a.std() > 3, "output is flat, blobs did not render"
    dark = np.asarray(render(160, 90, DARK_BG, DARK_BLOBS))
    assert dark.mean() < a.mean(), "dark variant is not darker than light"
    print("self-check OK")


if __name__ == "__main__":
    import sys
    if "--self-check" in sys.argv:
        _self_check()
    else:
        main()
