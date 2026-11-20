#!/usr/bin/env python3
"""Judges renders made outside Studio, such as Skills' build-previs on a clean machine (0.15's M6.2), against the
published pre-vis renders they must match, by the scene contract's bounds.

    python3 .project/scripts/judge-renders.py <renders folder> [<Last order folder>]

A renders folder named after one builder's part of the scene files' names, such as `4195103`, is judged against
that builder's cameras alone.

Last order's folder defaults to LAST_ORDER. For each camera of the newest scene file of each builder whose camera
names a published grey render (`reference_file`), it finds the render in the folder whose name holds that camera's
name and "grey", and the builder's part of the scene's name when several builders' renders are there. Each pair
passes when no more than 0.5% of pixels differ and no more than 0.02% differ more than 2 pixels from an edge, the
edge grey 55, in either picture. It needs Pillow and numpy, as the builders do. It changes nothing.
"""
import glob
import json
import os
import re
import struct
import sys

import numpy as np
from PIL import Image

EDGE = 55


def cameras(scene):
    """Each camera's name and published grey render, from a scene file's glTF extras."""
    data = open(scene, "rb").read()
    length = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20:20 + length])
    for node in gltf.get("nodes", []):
        ref = (node.get("extras") or {}).get("reference_file")
        if "camera" in node and ref and ref.endswith(".png"):
            yield node["name"], ref


def differences(published, ours):
    a = np.asarray(Image.open(published).convert("L")).astype(int)
    b = np.asarray(Image.open(ours).convert("L")).astype(int)
    if a.shape != b.shape:
        return None
    differ = a != b
    edge = (a == EDGE) | (b == EDGE)
    near = edge.copy()
    for dy in range(-2, 3):
        for dx in range(-2, 3):
            near |= np.roll(np.roll(edge, dy, 0), dx, 1)
    return differ.mean(), (differ & ~near).mean()


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    renders = sys.argv[1]
    last_order = sys.argv[2] if len(sys.argv) > 2 else os.environ.get("LAST_ORDER", "")
    if not os.path.isdir(last_order):
        sys.exit("judge-renders: name Last order's folder, or set LAST_ORDER")
    newest = {}
    for scene in sorted(glob.glob(os.path.join(last_order, "prompts/resources/*-diner-scene-*.glb"))):
        newest[scene.split("-diner-scene-")[1][:-4]] = scene
    # A folder named after one builder, as "4195103", holds that builder's renders alone.
    own = os.path.basename(os.path.normpath(renders))
    if own in newest:
        newest = {own: newest[own]}
    files = sorted(glob.glob(os.path.join(renders, "**", "*.png"), recursive=True))
    several = len({b for b in newest if any(b in os.path.basename(f) for f in files)}) > 1
    judged, failed = 0, 0
    for builder, scene in sorted(newest.items()):
        for camera, ref in cameras(scene):
            # The camera's name comes straight before "-grey", once the builder's part is taken out, since a
            # builder's name can hold a camera's, as "2026-11-10-08-05-door-close" holds "door-close".
            def names(f):
                base = os.path.basename(f)
                return base.replace(builder, "") if builder in base else base
            found = [f for f in files if re.search(r"(^|[-_])%s[-_]grey" % re.escape(camera), names(f))
                     and (not several or builder in os.path.basename(f))]
            if len(found) != 1:
                print("%s %s: %s" % (builder, camera, "no render found" if not found else "several renders: " + ", ".join(found)))
                failed += 1
                continue
            result = differences(os.path.join(last_order, ref), found[0])
            if result is None:
                print("%s %s: %s is not the published render's size" % (builder, camera, found[0]))
                failed += 1
                continue
            differ, away = result
            ok = differ <= 0.005 and away <= 0.0002
            print("%s %s: %.3f%% differ, %.4f%% more than 2 px from an edge, %s (%s against %s)"
                  % (builder, camera, differ * 100, away * 100, "passed" if ok else "FAILED", os.path.basename(found[0]), os.path.basename(ref)))
            judged += 1
            failed += not ok
    print("%d judged, %d failed" % (judged, failed))
    sys.exit(1 if failed or not judged else 0)


if __name__ == "__main__":
    main()
