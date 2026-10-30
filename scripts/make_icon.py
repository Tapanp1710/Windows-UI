"""
make_icon.py - generates the app icon. Original artwork, no Apple assets.

A warm amber squircle with a soft light sweep across it and a small circular
"island" cut-out, echoing the floating-islands design language of the UI layer.

    python scripts/make_icon.py                 # -> assets/icons/

Writes icon.ico (multi-size, needed by tauri-build for the Windows resource) plus
individual PNGs for the tray and installer.
"""

import argparse
import pathlib

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 8  # supersample factor; everything is drawn big then downsampled


def squircle_mask(size, radius_frac=0.235):
    """Rounded-square mask. 0.235 is close to the superellipse look without the maths."""
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    r = int(size * radius_frac)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=r, fill=255)
    return m


def render(size):
    s = size * SS
    yy, xx = np.mgrid[0:s, 0:s].astype(np.float32) / s

    # Warm amber diagonal gradient: deeper at bottom-right, brighter top-left.
    t = np.clip((xx * 0.55 + yy * 0.75), 0.0, 1.0)
    top = np.array([255, 191, 112], dtype=np.float32)
    bot = np.array([233, 130, 62], dtype=np.float32)
    rgb = top[None, None, :] * (1 - t[..., None]) + bot[None, None, :] * t[..., None]

    # Soft diagonal light sweep across the upper-left.
    band = np.exp(-(((xx + yy) - 0.62) ** 2) / (2 * 0.085 ** 2))
    rgb = rgb * (1 - 0.20 * band[..., None]) + 255.0 * (0.20 * band[..., None])

    # The "island": a crisp white disc, slightly below centre, sitting on a faint
    # halo. The disc edge is deliberately tight - a wide falloff reads as a smudge
    # at 32px rather than as a shape.
    cx, cy, rad = 0.50, 0.545, 0.200
    d = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2) / rad

    halo = np.clip(1.0 - d / 1.75, 0.0, 1.0) ** 2
    rgb = rgb * (1 - 0.18 * halo[..., None]) + 255.0 * (0.18 * halo[..., None])

    edge = 0.055  # antialias width as a fraction of the radius
    k = np.clip((1.0 - d) / edge, 0.0, 1.0)
    disc = k * k * (3.0 - 2.0 * k)
    rgb = rgb * (1 - 0.95 * disc[..., None]) + 253.0 * (0.95 * disc[..., None])

    img = Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8), "RGB")
    img = img.filter(ImageFilter.GaussianBlur(radius=s / 900.0))

    out = img.convert("RGBA")
    out.putalpha(squircle_mask(s))
    # LANCZOS downsample does the antialiasing for both the squircle edge and the disc.
    return out.resize((size, size), Image.LANCZOS)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    root = pathlib.Path(__file__).parent.parent
    out_dir = pathlib.Path(args.out) if args.out else root / "assets" / "icons"
    out_dir.mkdir(parents=True, exist_ok=True)

    base = render(256)
    for n in (32, 128, 256, 512):
        render(n).save(out_dir / f"icon-{n}.png", "PNG")

    # tauri-build looks for icons/icon.ico relative to the crate root; it is also the
    # installer and tray icon.
    base.save(out_dir / "icon.ico", format="ICO", sizes=[(n, n) for n in ICO_SIZES])
    print(f"{out_dir / 'icon.ico'}  ({(out_dir / 'icon.ico').stat().st_size // 1024} KB)")
    for p in sorted(out_dir.glob("icon-*.png")):
        print(f"{p}  ({p.stat().st_size // 1024} KB)")


def _self_check():
    im = render(64)
    assert im.size == (64, 64) and im.mode == "RGBA"
    a = np.asarray(im)
    # Corners must be transparent - proves the squircle mask applied.
    assert a[0, 0, 3] == 0 and a[63, 63, 3] == 0, "corners are not rounded"
    # Centre must be opaque and light (the island disc).
    assert a[32, 32, 3] == 255, "centre is transparent"
    assert a[32, 35, :3].mean() > 200, "island disc did not render"
    # Edge midpoint must be warm amber, not white.
    edge = a[32, 4, :3].astype(int)
    assert edge[0] > edge[2] + 40, f"body is not warm amber: {edge}"
    print("self-check OK")


if __name__ == "__main__":
    import sys
    if "--self-check" in sys.argv:
        _self_check()
    else:
        main()
