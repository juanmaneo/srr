"""Minimal glTF binary reader/writer for the rigging tools (numpy only).

Reads what TRELLIS (trimesh) writes: one mesh, one primitive, one embedded PNG texture.
Writes a skinned model with named animation clips in the layout the game expects
(`docs/3d-asset-pipeline.md`): a root node holding the skeleton and the skinned mesh.
"""
from __future__ import annotations

import json
import struct
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

COMPONENT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


@dataclass
class Mesh:
    positions: np.ndarray  # (n, 3) float32
    normals: np.ndarray  # (n, 3) float32
    uvs: np.ndarray  # (n, 2) float32
    triangles: np.ndarray  # (m, 3) uint32
    texture: bytes  # PNG bytes, embedded as is
    generator: str = ""
    extra: dict = field(default_factory=dict)


def split(glb: bytes) -> tuple[dict, bytes]:
    magic, version, length = struct.unpack("<4sII", glb[:12])
    assert magic == b"glTF" and version == 2 and length == len(glb), "not a glTF binary"
    offset, doc, blob = 12, None, b""
    while offset < len(glb):
        size, kind = struct.unpack("<I4s", glb[offset:offset + 8])
        chunk = glb[offset + 8:offset + 8 + size]
        offset += 8 + size
        if kind == b"JSON":
            doc = json.loads(chunk)
        elif kind == b"BIN\x00":
            blob = chunk
    assert doc is not None
    return doc, blob


def accessor(doc: dict, blob: bytes, index: int) -> np.ndarray:
    acc = doc["accessors"][index]
    view = doc["bufferViews"][acc["bufferView"]]
    dtype = np.dtype(COMPONENT[acc["componentType"]])
    width = WIDTH[acc["type"]]
    start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride = view.get("byteStride")
    if stride and stride != dtype.itemsize * width:
        rows = np.frombuffer(blob, dtype=np.uint8, count=(acc["count"] - 1) * stride + dtype.itemsize * width, offset=start)
        data = np.stack([np.frombuffer(rows[i * stride:i * stride + dtype.itemsize * width].tobytes(), dtype=dtype)
                         for i in range(acc["count"])])
        return data.reshape(acc["count"], width) if width > 1 else data.reshape(acc["count"])
    data = np.frombuffer(blob, dtype=dtype, count=acc["count"] * width, offset=start)
    return data.reshape(acc["count"], width) if width > 1 else data


def read(path: Path) -> Mesh:
    doc, blob = split(Path(path).read_bytes())
    prims = [p for m in doc["meshes"] for p in m["primitives"]]
    assert len(prims) == 1, f"expected one primitive, found {len(prims)}"
    prim = prims[0]
    attrs = prim["attributes"]
    positions = accessor(doc, blob, attrs["POSITION"]).astype(np.float32)
    normals = accessor(doc, blob, attrs["NORMAL"]).astype(np.float32) if "NORMAL" in attrs else np.zeros_like(positions)
    uvs = accessor(doc, blob, attrs["TEXCOORD_0"]).astype(np.float32)
    triangles = accessor(doc, blob, prim["indices"]).astype(np.uint32).reshape(-1, 3)
    material = doc["materials"][prim["material"]]
    texture_index = material["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    image = doc["images"][doc["textures"][texture_index]["source"]]
    view = doc["bufferViews"][image["bufferView"]]
    start = view.get("byteOffset", 0)
    texture = blob[start:start + view["byteLength"]]
    return Mesh(positions, normals, uvs, triangles, texture, doc.get("asset", {}).get("generator", ""))


def pack(doc: dict, blob: bytes) -> bytes:
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    blob = blob + b"\x00" * (-len(blob) % 4)
    total = 12 + 8 + len(text) + 8 + len(blob)
    return (struct.pack("<4sII", b"glTF", 2, total) + struct.pack("<I4s", len(text), b"JSON") + text
            + struct.pack("<I4s", len(blob), b"BIN\x00") + blob)


class Bin:
    """The binary chunk under construction, with its buffer views and accessors."""

    def __init__(self) -> None:
        self.bytes = bytearray()
        self.views: list[dict] = []
        self.accessors: list[dict] = []

    def view(self, data: bytes, target: int | None = None) -> int:
        self.bytes.extend(b"\x00" * (-len(self.bytes) % 4))
        entry = {"buffer": 0, "byteOffset": len(self.bytes), "byteLength": len(data)}
        if target:
            entry["target"] = target
        self.bytes.extend(data)
        self.views.append(entry)
        return len(self.views) - 1

    def add(self, array: np.ndarray, kind: str, component: int, target: int | None = None,
            bounds: bool = False, normalized: bool = False) -> int:
        array = np.ascontiguousarray(array)
        entry = {"bufferView": self.view(array.tobytes(), target), "componentType": component,
                 "count": int(array.shape[0]), "type": kind}
        if bounds:
            entry["min"] = [float(v) for v in array.min(axis=0)] if array.ndim > 1 else [float(array.min())]
            entry["max"] = [float(v) for v in array.max(axis=0)] if array.ndim > 1 else [float(array.max())]
        if normalized:
            entry["normalized"] = True
        self.accessors.append(entry)
        return len(self.accessors) - 1


def write_static(path: Path, mesh: Mesh) -> None:
    """A plain textured mesh (no skeleton): for previews of the cleaning steps."""
    bin_ = Bin()
    pos = bin_.add(mesh.positions.astype(np.float32), "VEC3", 5126, 34962, bounds=True)
    nor = bin_.add(mesh.normals.astype(np.float32), "VEC3", 5126, 34962)
    uvs = bin_.add(mesh.uvs.astype(np.float32), "VEC2", 5126, 34962)
    idx = bin_.add(mesh.triangles.astype(np.uint32).reshape(-1), "SCALAR", 5125, 34963)
    image = bin_.view(mesh.texture)
    doc = {
        "asset": {"version": "2.0", "generator": "srr tools"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "name": "body"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "NORMAL": nor, "TEXCOORD_0": uvs},
                                    "indices": idx, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0,
                                                "roughnessFactor": 1.0}}],
        "textures": [{"sampler": 0, "source": 0}],
        "images": [{"mimeType": "image/png", "bufferView": image}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 33071, "wrapT": 33071}],
        "accessors": bin_.accessors, "bufferViews": bin_.views, "buffers": [{"byteLength": len(bin_.bytes)}],
    }
    Path(path).write_bytes(pack(doc, bytes(bin_.bytes)))
