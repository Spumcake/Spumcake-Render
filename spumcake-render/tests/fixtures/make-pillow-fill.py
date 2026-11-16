"""Writes pillow-fill.txt: polygons filled by Pillow itself, with the runs of pixels it covered, so the engine's
fill rule is checked against Pillow's own. Run it with Pillow 12.1.1, the version the pre-vis builders used:

    python3 make-pillow-fill.py > pillow-fill.txt
"""
import random
import numpy as np
import PIL
from PIL import Image, ImageDraw

W, H = 48, 48
random.seed(20261116)


def runs(mask):
    out = []
    for y in range(mask.shape[0]):
        x = 0
        while x < mask.shape[1]:
            if mask[y, x]:
                x0 = x
                while x + 1 < mask.shape[1] and mask[y, x + 1]:
                    x += 1
                out.append(f"{y}:{x0}-{x}")
            x += 1
    return out


def case(points):
    m = Image.new("L", (W, H), 0)
    ImageDraw.Draw(m).polygon(points, fill=1)
    coords = " ".join(repr(float(v)) for p in points for v in p)
    print(f"{W} {H} | {coords} | {' '.join(runs(np.array(m, bool)))}")


print(f"# Pillow {PIL.__version__}: width height | corners x y ... | row:first-last ...")
for n in (3, 3, 3, 4, 4, 5):
    for _ in range(300):
        case([(random.uniform(-20, 68), random.uniform(-20, 68)) for _ in range(n)])
# Corners on whole and half pixels, where the rounding rules decide.
for n in (3, 4):
    for _ in range(300):
        case([(random.randint(-8, 56) / 2, random.randint(-8, 56) / 2) for _ in range(n)])
# Thin slivers and horizontal edges.
for _ in range(200):
    x, y = random.uniform(0, 48), random.uniform(0, 48)
    case([(x, y), (x + random.uniform(-30, 30), y + random.uniform(-0.9, 0.9)), (x + random.uniform(-30, 30), y)])
