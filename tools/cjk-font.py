#!/usr/bin/env python3
"""Cuts Noto Sans SC (google/fonts, OFL) down to the Chinese characters the
client's strings, menus, plugins and language names use, as the fallback font the client
draws them with: cjk-font.py NotoSansSC[wght].ttf. Needs fontTools. Run it
again when those change; it writes client-ng/fonts/NotoSansSC-Subset.ttf."""
import glob, os, sys
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "fonts", "NotoSansSC-Subset.ttf")


def main():
    chars = set()
    sources = glob.glob(os.path.join(ROOT, "config", "locales", "zh_*.lproj", "*.strings"))
    sources += [os.path.join(ROOT, "config", "menus.lua")] + glob.glob(os.path.join(ROOT, "config", "plugins", "*.lua"))
    # The languages' names (中文 in the Language list).
    sources += [os.path.join(ROOT, "src", "i18n.rs")]
    for path in sources:
        chars |= {c for c in open(path, encoding="utf-8").read() if ord(c) > 0x2000}
    # Medium, between the skins' regular and bold.
    font = instancer.instantiateVariableFont(TTFont(sys.argv[1]), {"wght": 500})
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ["*"]
    options.notdef_outline = True
    sub = subset.Subsetter(options)
    sub.populate(unicodes={ord(c) for c in chars})
    sub.subset(font)
    font.save(OUT)
    print(f"{OUT}: {len(chars)} characters, {os.path.getsize(OUT)} bytes")


main()
