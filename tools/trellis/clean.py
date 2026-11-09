"""Cleaning of a TRELLIS mesh: the flat slab through its middle, and the loose specks."""
from __future__ import annotations

import numpy as np
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import connected_components

from glbio import Mesh


def face_data(mesh: Mesh):
    p, t = mesh.positions, mesh.triangles
    a, b, c = p[t[:, 0]], p[t[:, 1]], p[t[:, 2]]
    n = np.cross(b - a, c - a)
    length = np.linalg.norm(n, axis=1)
    return (a + b + c) / 3, n / np.maximum(length[:, None], 1e-12), length / 2


def subset(mesh: Mesh, keep: np.ndarray) -> Mesh:
    """The mesh with only the faces where `keep`; unused vertices dropped."""
    tris = mesh.triangles[keep]
    used = np.unique(tris)
    remap = np.full(len(mesh.positions), -1, dtype=np.int64)
    remap[used] = np.arange(len(used))
    return Mesh(mesh.positions[used], mesh.normals[used], mesh.uvs[used], remap[tris].astype(np.uint32),
                mesh.texture, mesh.generator)


def slab_axis_and_band(mesh: Mesh, flat: float = 0.98):
    """The axis the slab is perpendicular to and its thickness: the spike of flat faces."""
    centre, normal, area = face_data(mesh)
    best = None
    for axis in range(3):
        flat_faces = np.abs(normal[:, axis]) > flat
        if not flat_faces.any():
            continue
        values = centre[flat_faces, axis]
        hist, edges = np.histogram(values, bins=400, weights=area[flat_faces])
        peak = hist.argmax()
        share = hist[max(peak - 3, 0):peak + 4].sum() / area.sum()
        if best is None or share > best[0]:
            best = (share, axis, (edges[peak] + edges[peak + 1]) / 2)
    return best


def remove_slab(mesh: Mesh, band: float = 0.012) -> tuple[Mesh, dict]:
    share, axis, middle = slab_axis_and_band(mesh)
    centre, normal, area = face_data(mesh)
    slab = (np.abs(normal[:, axis]) > 0.9) & (np.abs(centre[:, axis] - middle) < band)
    info = {"axis": int(axis), "middle": float(middle), "spike_share": float(share),
            "removed_faces": int(slab.sum()), "removed_area_share": float(area[slab].sum() / area.sum())}
    return subset(mesh, ~slab), info


def components(mesh: Mesh):
    t = mesh.triangles
    n = len(mesh.positions)
    rows = np.concatenate([t[:, 0], t[:, 1], t[:, 2]])
    cols = np.concatenate([t[:, 1], t[:, 2], t[:, 0]])
    graph = coo_matrix((np.ones(len(rows)), (rows, cols)), shape=(n, n))
    count, label = connected_components(graph, directed=False)
    return count, label


def drop_specks(mesh: Mesh, keep_share: float = 0.01) -> tuple[Mesh, dict]:
    """Keeps the components that are at least `keep_share` of the surface."""
    centre, normal, area = face_data(mesh)
    count, label = components(mesh)
    face_label = label[mesh.triangles[:, 0]]
    share = np.bincount(face_label, weights=area, minlength=count) / area.sum()
    keep_label = share >= keep_share
    info = {"components": int(count), "kept": int(keep_label.sum()),
            "kept_area_share": float(share[keep_label].sum())}
    return subset(mesh, keep_label[face_label]), info


def drop_plates(mesh: Mesh, flatness: float = 0.12, spans: float = 0.4) -> tuple[Mesh, dict]:
    """Drops the plates TRELLIS leaves behind the figure: components that are big in two
    directions and thin in the third (a figure's own parts, even a cape, are thicker)."""
    count, label = components(mesh)
    face_label = label[mesh.triangles[:, 0]]
    low = np.full((count, 3), np.inf)
    high = np.full((count, 3), -np.inf)
    for corner in range(3):
        vertex = mesh.positions[mesh.triangles[:, corner]]
        np.minimum.at(low, face_label, vertex)
        np.maximum.at(high, face_label, vertex)
    size = np.sort(high - low, axis=1)  # thinnest first
    plate = (size[:, 2] >= spans) & (size[:, 0] <= flatness * size[:, 2])
    centre, normal, area = face_data(mesh)
    thin = np.argsort(high - low, axis=1)[:, 0]
    axis = int(np.bincount(thin[plate], minlength=3).argmax()) if plate.any() else 2
    info = {"components": int(count), "plates": int(plate.sum()), "axis": axis,
            "plate_area_share": float(area[plate[face_label]].sum() / area.sum())}
    return subset(mesh, ~plate[face_label]), info


def drop_tiny(mesh: Mesh, share: float = 0.0005) -> tuple[Mesh, dict]:
    centre, normal, area = face_data(mesh)
    count, label = components(mesh)
    face_label = label[mesh.triangles[:, 0]]
    parts = np.bincount(face_label, weights=area, minlength=count) / area.sum()
    keep = parts >= share
    return subset(mesh, keep[face_label]), {"components": int(count), "kept": int(keep.sum())}


def drop_flakes(mesh: Mesh, axis: int = 2, depth: float = 0.07, facing: float = 0.85) -> tuple[Mesh, dict]:
    """Drops the flakes of the plate that survived it: components shallow along `axis` whose
    faces mostly face along it."""
    count, label = components(mesh)
    face_label = label[mesh.triangles[:, 0]]
    centre, normal, area = face_data(mesh)
    low = np.full(count, np.inf)
    high = np.full(count, -np.inf)
    for corner in range(3):
        z = mesh.positions[mesh.triangles[:, corner], axis]
        np.minimum.at(low, face_label, z)
        np.maximum.at(high, face_label, z)
    total = np.bincount(face_label, weights=area, minlength=count)
    along = np.bincount(face_label, weights=area * np.abs(normal[:, axis]), minlength=count) / np.maximum(total, 1e-12)
    flake = (high - low <= depth) & (along >= facing)
    return subset(mesh, ~flake[face_label]), {"components": int(count), "flakes": int(flake.sum())}
