"""Regenerate the shared geometric CameraBridge icon (requires Pillow)."""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]


def icon(size=1024, background=True):
    image = Image.new('RGBA', (1024, 1024))
    draw = ImageDraw.Draw(image)
    if background:
        draw.rounded_rectangle((0, 0, 1023, 1023), radius=224, fill='#17181b')
    draw.rounded_rectangle((184, 294, 840, 758), radius=106, fill='#e9eaec')
    draw.rounded_rectangle((310, 230, 526, 370), radius=38, fill='#e9eaec')
    draw.ellipse((336, 350, 688, 702), fill='#17181b')
    draw.ellipse((390, 404, 634, 648), fill='#4c82f7')
    draw.ellipse((450, 464, 574, 588), fill='#17181b')
    draw.ellipse((721, 371, 765, 415), fill='#4c82f7')
    return image.resize((size, size), Image.Resampling.LANCZOS)


def main():
    assets = ROOT / 'assets'
    assets.mkdir(exist_ok=True)
    image = icon()
    image.save(assets / 'icon.png')
    image.save(assets / 'icon.ico', sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    res = ROOT / 'android/app/src/main/res'
    for density, size in [('mdpi', 48), ('hdpi', 72), ('xhdpi', 96), ('xxhdpi', 144), ('xxxhdpi', 192)]:
        folder = res / ('mipmap-' + density)
        folder.mkdir(parents=True, exist_ok=True)
        icon(size).save(folder / 'ic_launcher.png')
    folder = res / 'drawable'
    folder.mkdir(parents=True, exist_ok=True)
    icon(432, background=False).save(folder / 'ic_launcher_foreground.png')
    # White silhouette remains legible in Android's monochrome status bar.
    small = icon(96, background=False)
    alpha = small.getchannel('A')
    small = Image.new('RGBA', small.size, 'white')
    small.putalpha(alpha)
    small.save(folder / 'ic_notification.png')


if __name__ == '__main__':
    main()
