#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow"]
# ///
"""Print basic stats of a .glb (meshes, vertices, triangles, texture, skin/animations, bbox).

    uv run tools/trellis/glb_info.py model.glb [--save-textures DIR]
"""
import argparse
import io
import json
import struct
from pathlib import Path

from PIL import Image

ap = argparse.ArgumentParser()
ap.add_argument("glb", type=Path)
ap.add_argument("--save-textures", type=Path)
a = ap.parse_args()

b = a.glb.read_bytes()
magic, version, length = struct.unpack("<4sII", b[:12])
assert magic == b"glTF" and length == len(b), "not a valid .glb"
off, j, blob = 12, None, b""
while off < len(b):
    n, kind = struct.unpack("<I4s", b[off:off + 8])
    chunk = b[off + 8:off + 8 + n]
    off += 8 + n
    if kind == b"JSON":
        j = json.loads(chunk)
    elif kind.startswith(b"BIN"):
        blob = chunk

verts = tris = prims = 0
mn = [float("inf")] * 3
mx = [float("-inf")] * 3
for m in j.get("meshes", []):
    for p in m["primitives"]:
        prims += 1
        pos = j["accessors"][p["attributes"]["POSITION"]]
        verts += pos["count"]
        idx = j["accessors"][p["indices"]]["count"] if "indices" in p else pos["count"]
        tris += idx // 3
        mn = [min(x, y) for x, y in zip(mn, pos["min"])]
        mx = [max(x, y) for x, y in zip(mx, pos["max"])]

print(f"file        {a.glb} ({len(b) / 1e6:.2f} MB), glTF {version}, generator {j['asset'].get('generator')}")
print(f"meshes      {len(j.get('meshes', []))}, primitives {prims}, vertices {verts}, triangles {tris}")
print(f"nodes       {len(j.get('nodes', []))}, skins {len(j.get('skins', []))}, animations {len(j.get('animations', []))}")
print(f"materials   {len(j.get('materials', []))}, textures {len(j.get('textures', []))}")
for i, img in enumerate(j.get("images", [])):
    bv = j["bufferViews"][img["bufferView"]]
    raw = blob[bv.get("byteOffset", 0):bv.get("byteOffset", 0) + bv["byteLength"]]
    im = Image.open(io.BytesIO(raw))
    print(f"image {i}     {img.get('mimeType')} {im.size[0]}x{im.size[1]} {im.mode} ({len(raw) / 1e6:.2f} MB)")
    if a.save_textures:
        a.save_textures.mkdir(parents=True, exist_ok=True)
        im.save(a.save_textures / f"{a.glb.stem}_tex{i}.png")
size = [y - x for x, y in zip(mn, mx)]
print(f"bbox min    {[round(v, 4) for v in mn]}")
print(f"bbox max    {[round(v, 4) for v in mx]}")
print(f"bbox size   x={size[0]:.4f} y={size[1]:.4f} z={size[2]:.4f}  (glTF is Y-up; tallest axis: {'xyz'[size.index(max(size))]})")
