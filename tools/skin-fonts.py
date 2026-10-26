#!/usr/bin/env python3
"""Subset original skin fonts: skin-fonts.py ORIGINAL_SKINS_DIRECTORY.

The source directory mirrors skins/. Requires fontTools. Retains Latin-1,
characters used by the client's sources and translations, and their layout
dependencies. The painters use unhinted outlines, so hinting is omitted.
"""
from pathlib import Path
import re
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent


def characters():
    chars = set(range(0x20, 0x100))
    for directory in ("skins", "config", "src"):
        for path in (ROOT / directory).rglob("*"):
            if path.suffix in (".lua", ".strings", ".rs"):
                text = path.read_text(encoding="utf-8")
                chars.update(map(ord, text))
                chars.update(int(value, 16) for value in re.findall(r"\\u\{([0-9a-fA-F]+)\}", text))
    return chars


def main():
    source = Path(sys.argv[1]).resolve()
    chars = characters()
    before = after = 0
    for index, destination in enumerate(sorted((ROOT / "skins").rglob("*.ttf"))):
        original = source / destination.relative_to(ROOT / "skins")
        font = TTFont(original, recalcTimestamp=False)
        options = subset.Options()
        options.hinting = False
        options.layout_features = ["*"]
        options.notdef_outline = True
        sub = subset.Subsetter(options)
        sub.populate(unicodes=chars)
        sub.subset(font)
        # Modified OFL fonts use distinct family names, including when the
        # upstream family name is reserved. Copyright and licenses remain.
        family = f"UNFR0ZEN UI {index + 1}"
        style = font["name"].getDebugName(2) or "Regular"
        names = {1: family, 2: style, 3: family, 4: f"{family} {style}",
                 6: f"UNFR0ZENUI{index + 1}-{style.replace(' ', '')}",
                 16: family, 17: style, 21: family, 22: style}
        for record in font["name"].names:
            if record.nameID in names:
                record.string = names[record.nameID].encode(record.getEncoding())
        font.save(destination)
        before += original.stat().st_size
        after += destination.stat().st_size
        print(f"{destination.relative_to(ROOT)}: {original.stat().st_size} -> {destination.stat().st_size}")
    print(f"Fonts: {before} -> {after} bytes ({before - after} saved)")


if __name__ == "__main__":
    main()
