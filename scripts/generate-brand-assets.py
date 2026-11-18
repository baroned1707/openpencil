#!/usr/bin/env python3
"""Regenerate every derived brand asset from the master OpenPencil icon.

Usage (stdlib + Pillow only, run isolated):
    python3 -I scripts/generate-brand-assets.py            # write all assets
    python3 -I scripts/generate-brand-assets.py --check    # exit 1 on drift
    python3 -I scripts/generate-brand-assets.py --cover SHOT.png
                                                           # rebuild README cover

Master: crates/op-host-desktop/assets/icon.png (1024x1024 RGBA, light rounded
tile at ~100..925 px with a soft drop shadow). There is no vector source, so
every asset is a DOWNSCALE of it (the only upscale is the VS Code icon, 1.16x).

Derivation rules:
  * "clean" master  = master with the shadow removed (tile alpha rebuilt from
                      the near-opaque pixels, softened by a 1 px blur).
  * "tile"          = clean master cropped to the tile bounds (fills the canvas;
                      used for sizes <= 48 px where margins waste pixels).
  * app sizes >= 64 = master as is (shadow included), Android/OHOS/brand-logo
                      = clean master (same margin as the old launcher icons).
  * iOS             = tile scaled full-bleed, corners painted white, opaque
                      8-bit RGB (iOS applies its own rounded mask).
  * document icon   = paper page + folded corner + OP tile + ".op" label,
                      drawn at 2x and downscaled; the label is dropped at
                      <= 48 px where it is illegible.

Output is deterministic: no PNG metadata, fixed compression. --check rebuilds
everything in memory and compares bytes with the tracked files. The .icns is
built with macOS `iconutil`; where that tool is absent (or the platform's
output differs) --check reports it as skipped instead of failing.
"""

import argparse
import io
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates/op-host-desktop/assets"
MASTER = ASSETS / "icon.png"
HELVETICA = "/System/Library/Fonts/Helvetica.ttc"
BLUE = (0x0D, 0x99, 0xFF, 255)
LINUX_SIZES = (16, 32, 48, 64, 128, 256, 512)
ICNS_SIZES = (16, 32, 64, 128, 256, 512, 1024)  # pixel sizes, @1x and @2x
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
RESAMPLE = Image.LANCZOS


def png_bytes(image):
    buffer = io.BytesIO()
    image.save(buffer, "PNG", optimize=True, compress_level=9)
    return buffer.getvalue()


def resized(image, size):
    return image.resize((size, size), RESAMPLE)


# --- master derivations -----------------------------------------------------

def clean_master(master):
    """Master without its drop shadow (tile only, anti-aliased edge)."""
    alpha = master.getchannel("A")
    hard = alpha.point(lambda v: 255 if v >= 250 else 0)
    soft = hard.filter(ImageFilter.GaussianBlur(0.8))
    flat = Image.alpha_composite(Image.new("RGBA", master.size, (255, 255, 255, 255)), master)
    # Outside the tile the colour is plain white so the soft edge has no grey fringe.
    rgb = Image.composite(flat, Image.new("RGBA", master.size, (255, 255, 255, 255)), hard)
    rgb.putalpha(soft)
    return rgb, hard.getbbox()


def tile_of(clean, bbox):
    return clean.crop(bbox)


def ios_icon(tile):
    full = resized(tile, 1024)
    flat = Image.new("RGBA", full.size, (255, 255, 255, 255))
    return Image.alpha_composite(flat, full).convert("RGB")


def vscode_icon(tile):
    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    canvas.paste(resized(tile, 960), (32, 32))
    return canvas


# --- document icon ----------------------------------------------------------

def bold_font(size):
    """Helvetica Bold from the system collection, else Pillow's default font."""
    for index in range(8):
        try:
            font = ImageFont.truetype(HELVETICA, size, index=index)
        except (OSError, ValueError):
            break
        if font.getname()[1] == "Bold":
            return font
    print("note: Helvetica Bold not found, using the Pillow default font", file=sys.stderr)
    return ImageFont.load_default(size)


def shadowed(base, mask, offset, blur, strength):
    """Composite a blurred, offset black copy of `mask` under `base`."""
    shadow = Image.new("L", mask.size, 0)
    shadow.paste(mask, offset)
    shadow = shadow.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: int(v * strength))
    layer = Image.new("RGBA", mask.size, (0, 0, 0, 0))
    layer.putalpha(shadow)
    return Image.alpha_composite(layer, base)


def document_canvas(tile, with_label):
    """The .op document icon on a 2048 px canvas (design units are 1024 px)."""
    scale = 2
    size = 1024 * scale

    def s(*values):
        return [int(round(v * scale)) for v in values]

    left, top, right, bottom, fold = 170, 90, 854, 934, 170
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle(s(left, top, right, bottom), radius=40 * scale, fill=255)
    draw.polygon([tuple(s(right - fold, top - 2)), tuple(s(right + 2, top - 2)),
                  tuple(s(right + 2, top + fold))], fill=0)

    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas = shadowed(canvas, mask, (0, 18 * scale), 22 * scale, 0.34)
    page = Image.new("RGBA", (size, size), (250, 251, 252, 255))
    outline = ImageChops.subtract(mask, mask.filter(ImageFilter.MinFilter(2 * scale + 1)))
    page.paste(Image.new("RGBA", (size, size), (208, 212, 218, 255)), mask=outline)
    canvas.paste(page, mask=mask)

    flap = Image.new("L", (size, size), 0)
    flap_points = [tuple(s(right - fold, top)), tuple(s(right - fold, top + fold)),
                   tuple(s(right, top + fold))]
    ImageDraw.Draw(flap).polygon(flap_points, fill=255)
    canvas = shadowed(canvas, flap, (-6 * scale, 8 * scale), 8 * scale, 0.30)
    canvas.paste(Image.new("RGBA", (size, size), (226, 230, 236, 255)), mask=flap)
    ImageDraw.Draw(canvas).line(flap_points + [flap_points[0]], fill=(196, 201, 209, 255),
                                width=3 * scale, joint="curve")

    tile_size = 440 * scale
    tile_layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    corner = (512 * scale - tile_size // 2, 508 * scale - tile_size // 2)
    tile_layer.paste(resized(tile, tile_size), corner)
    canvas = shadowed(canvas, tile_layer.getchannel("A"), (0, 8 * scale), 9 * scale, 0.35)
    canvas = Image.alpha_composite(canvas, tile_layer)

    if with_label:
        strip = s(512 - 124, 784, 512 + 124, 872)
        draw = ImageDraw.Draw(canvas)
        draw.rounded_rectangle(strip, radius=44 * scale, fill=BLUE)
        font = bold_font(58 * scale)
        ink = font.getbbox(".op")
        center_x, center_y = (strip[0] + strip[2]) / 2, (strip[1] + strip[3]) / 2
        draw.text((center_x - (ink[0] + ink[2]) / 2, center_y - (ink[1] + ink[3]) / 2),
                  ".op", font=font, fill=(255, 255, 255, 255))
    return canvas


def document_icon(tile, pixels):
    """One document icon frame; small sizes crop the margin and drop the label."""
    canvas = document_canvas(tile, with_label=pixels > 48)
    if pixels <= 48:
        canvas = canvas.crop(tuple(int(v * 2) for v in (150, 70, 874, 950)))
        side = max(canvas.size)
        square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        square.paste(canvas, ((side - canvas.width) // 2, (side - canvas.height) // 2))
        canvas = square
    return resized(canvas, pixels)


# --- container formats --------------------------------------------------------

def ico_bytes(frames):
    buffer = io.BytesIO()
    biggest = frames[max(frames)]
    others = [frames[size] for size in sorted(frames) if size != max(frames)]
    biggest.save(buffer, "ICO", sizes=[(size, size) for size in frames], append_images=others)
    return buffer.getvalue()


def icns_bytes(frame_for):
    """Build an .icns via `iconutil`; None when the tool is unavailable."""
    names = {16: ("16x16", 1), 32: ("16x16", 2), 64: ("32x32", 2), 128: ("128x128", 1),
             256: ("256x256", 1), 512: ("512x512", 1), 1024: ("512x512", 2)}
    extra = {32: ("32x32", 1), 256: ("128x128", 2), 512: ("256x256", 2)}
    with tempfile.TemporaryDirectory() as work:
        iconset = Path(work) / "document.iconset"
        iconset.mkdir()
        for pixels in ICNS_SIZES:
            data = png_bytes(frame_for(pixels))
            for label, density in [names[pixels]] + ([extra[pixels]] if pixels in extra else []):
                suffix = "" if density == 1 else "@2x"
                (iconset / f"icon_{label}{suffix}.png").write_bytes(data)
        out = Path(work) / "document.icns"
        try:
            subprocess.run(["iconutil", "-c", "icns", "-o", str(out), str(iconset)],
                           check=True, capture_output=True)
        except (OSError, subprocess.CalledProcessError):
            return None
        return out.read_bytes()


# --- asset table ----------------------------------------------------------------

def build_assets():
    """Return ({path: bytes}, {path: bytes-or-None for platform-dependent files})."""
    master = Image.open(MASTER).convert("RGBA")
    clean, bbox = clean_master(master)
    tile = tile_of(clean, bbox)
    files = {}

    def put(relative, image):
        path = ROOT / relative
        files[path] = png_bytes(image)

    put("packages/op-vscode/icon.png", vscode_icon(tile))
    for size in (16, 48, 128):
        put(f"packages/op-chrome-extension/icons/icon-{size}.png", resized(tile, size))
    put("crates/op-editor-ui/assets/brand-logo-128.png", resized(clean, 128))
    put("packaging/ios/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png", ios_icon(tile))
    densities = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
    for density, size in densities.items():
        put(f"packaging/android/app/src/main/res/mipmap-{density}/ic_launcher.png",
            resized(clean, size))
    for media in ("packaging/harmony/AppScope/resources/base/media",
                  "packaging/harmony/entry/src/main/resources/base/media"):
        put(f"{media}/app_icon.png", resized(clean, 192))

    document_frames = {}

    def document_frame(pixels):
        if pixels not in document_frames:
            document_frames[pixels] = document_icon(tile, pixels)
        return document_frames[pixels]

    for size in LINUX_SIZES:
        hicolor = f"packaging/linux/icons/hicolor/{size}x{size}"
        put(f"{hicolor}/apps/openpencil.png", resized(tile if size <= 48 else master, size))
        put(f"{hicolor}/mimetypes/application-x-openpencil.png", document_frame(size))
    put("crates/op-host-desktop/assets/document.png", document_frame(512))
    files[ASSETS / "document.ico"] = ico_bytes({size: document_frame(size) for size in ICO_SIZES})
    platform_files = {ASSETS / "document.icns": icns_bytes(document_frame)}
    return files, platform_files


def build_cover(source, destination):
    """README cover: a real UI screenshot, at most 2400 px wide."""
    image = Image.open(source).convert("RGB")
    if image.width > 2400:
        image = image.resize((2400, round(image.height * 2400 / image.width)), RESAMPLE)
    destination.write_bytes(png_bytes(image))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="exit 1 if tracked files differ")
    parser.add_argument("--cover", type=Path, help="screenshot to turn into screenshot/op-cover.png")
    args = parser.parse_args()

    if args.cover:
        build_cover(args.cover, ROOT / "screenshot/op-cover.png")
        return 0

    files, platform_files = build_assets()
    if args.check:
        drift = [p for p, data in files.items() if not p.exists() or p.read_bytes() != data]
        for path, data in platform_files.items():
            if data is None:
                print(f"skipped (iconutil unavailable): {path.relative_to(ROOT)}")
            elif not path.exists() or path.read_bytes() != data:
                drift.append(path)
        for path in drift:
            print(f"drift: {path.relative_to(ROOT)}", file=sys.stderr)
        print(f"checked {len(files) + len(platform_files)} assets, {len(drift)} drifted")
        return 1 if drift else 0

    for path, data in {**files, **platform_files}.items():
        if data is None:
            sys.exit(f"cannot build {path.name}: `iconutil` (macOS) is required")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        print(f"wrote {path.relative_to(ROOT)} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
