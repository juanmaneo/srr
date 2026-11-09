#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "scipy", "pillow"]
# ///
"""Rig a TRELLIS model with the movements of the puppet.

    rig.py model.glb out.glb --clips clips.json [--config model.rig.json] [--preview prefix]

`clips.json` is `cargo run --no-default-features --example models -- --clips`: the skeleton of
the puppet and every keyframe of every clip, the very poses of the stick figure of the arena.

The static mesh TRELLIS made is cleaned (its backdrop plates dropped), stood on the floor at
the puppet's height, given the puppet's eleven joints (placed by measuring the mesh, the
`--config` overriding what the measure got wrong) and a smooth skin, and every bone is made to
point where the same bone of the puppet points, clip by clip: the model plays the same
movements as the stick figure, shortest-arc rotations from the pose it was modelled in.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import clean  # noqa: E402
import glbio  # noqa: E402

HEIGHT = 1.8  # metres, the puppet's
GENERATOR = "srr rig (TRELLIS)"
JOINTS = ["hips", "spine", "head", "arm_l", "elbow_l", "arm_r", "elbow_r", "leg_l", "knee_l", "leg_r", "knee_r"]
HIPS, SPINE, HEAD, ARM_L, ELBOW_L, ARM_R, ELBOW_R, LEG_L, KNEE_L, LEG_R, KNEE_R = range(11)
PARENT = [-1, HIPS, SPINE, SPINE, ARM_L, SPINE, ARM_R, HIPS, LEG_L, HIPS, LEG_R]
# The direction each bone has in the puppet at rest, and the joint its bone ends at (`None`: an
# end point of the fit: head top, hands, ankles).
UP = np.array([0.0, 1.0, 0.0])
DOWN = np.array([0.0, -1.0, 0.0])
REST = [None, UP, UP, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN]


# ---------------------------------------------------------------------------------------------
# Quaternions (x, y, z, w), the glTF order
# ---------------------------------------------------------------------------------------------

def q_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return np.array([aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
                     aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz])


def q_conj(q):
    return np.array([-q[0], -q[1], -q[2], q[3]])


def q_rotate(q, v):
    """`v` (n, 3) or (3,) turned by the unit quaternion `q`."""
    u, w = q[:3], q[3]
    return v + 2 * np.cross(u, np.cross(u, v) + w * v)


def q_arc(a, b):
    """The shortest rotation taking the direction `a` to the direction `b`."""
    a, b = a / np.linalg.norm(a), b / np.linalg.norm(b)
    w = 1.0 + float(np.dot(a, b))
    if w < 1e-6:  # opposite: turn half way round any axis across `a`
        axis = np.cross(a, [1.0, 0.0, 0.0])
        if np.linalg.norm(axis) < 1e-3:
            axis = np.cross(a, [0.0, 0.0, 1.0])
        axis /= np.linalg.norm(axis)
        return np.array([*axis, 0.0])
    q = np.array([*np.cross(a, b), w])
    return q / np.linalg.norm(q)


def rot_x(angle, v):
    c, s = np.cos(angle), np.sin(angle)
    return np.array([v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c])


# ---------------------------------------------------------------------------------------------
# The mesh: clean, stand it up, size it
# ---------------------------------------------------------------------------------------------

def load(path: Path, yaw: float):
    mesh = glbio.read(path)
    mesh, plates = clean.drop_plates(mesh)
    mesh, flakes = clean.drop_flakes(mesh, axis=plates.get("axis", 2))
    mesh, tiny = clean.drop_tiny(mesh)
    print(f"clean: {plates['plates']} plates, {flakes['flakes']} flakes, "
          f"{tiny['components'] - tiny['kept']} specks dropped; {len(mesh.triangles)} triangles left")
    a = np.radians(yaw)
    turn = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]], dtype=np.float32)
    mesh.positions = mesh.positions @ turn.T
    mesh.normals = mesh.normals @ turn.T
    return mesh


def stand(mesh, config: dict | None = None) -> float:
    """Scales the mesh to the puppet's height, its soles on the floor, centred between its feet
    in x and z (`config["centre"]` = [x, z] of the unscaled... standing model overrides).
    Returns the scale."""
    p = mesh.positions
    low, high = float(p[:, 1].min()), float(p[:, 1].max())
    scale = HEIGHT / (high - low)
    p = (p - [0.0, low, 0.0]) * scale
    soles = p[p[:, 1] < 0.05 * HEIGHT]
    centre = [float((soles[:, 0].min() + soles[:, 0].max()) / 2), float(np.median(soles[:, 2]))]
    if config and "centre" in config:
        centre = config["centre"]
    p -= [centre[0], 0.0, centre[1]]
    mesh.positions = p.astype(np.float32)
    return scale


# ---------------------------------------------------------------------------------------------
# The joints
# ---------------------------------------------------------------------------------------------

def fit(mesh, config: dict) -> dict[str, np.ndarray]:
    """The eleven joints and the end points (head top, hands, ankles) of the mesh, measured.
    `config["joints"]` overrides any of them, in metres of the standing model."""
    p = mesh.positions
    h = HEIGHT
    at = {}
    head = p[p[:, 1] > 0.9 * h]
    centre = float((head[:, 0].min() + head[:, 0].max()) / 2)
    # The body's depth: the trunk, not the hair or the cape that hang behind it.
    trunk = p[(p[:, 1] > 0.5 * h) & (p[:, 1] < 0.75 * h) & (np.abs(p[:, 0] - centre) < 0.15)]
    depth = float(np.median(trunk[:, 2])) if len(trunk) else 0.0
    at["hips"] = np.array([centre, 0.52 * h, depth])
    at["neck"] = np.array([centre, 0.76 * h, depth])
    at["head_top"] = np.array([centre, h, depth])
    shoulder = at["hips"] + 0.88 * (at["neck"] - at["hips"])
    # Hands: the farthest reach of the mesh sideways at the height of the arms.
    band = p[(p[:, 1] > shoulder[1] - 0.30 * h) & (p[:, 1] < shoulder[1] + 0.12 * h)]
    for side, sign in (("r", 1.0), ("l", -1.0)):
        at[f"arm_{side}"] = shoulder + np.array([sign * 0.07 * h, 0.0, 0.0])
        mine = band[(band[:, 0] - centre) * sign > 0]
        reach = (mine[:, 0] - centre) * sign
        tip = mine[reach >= np.percentile(reach, 99.0)]
        at[f"hand_{side}"] = np.array([float(tip[:, 0].mean()), float(tip[:, 1].mean()), float(tip[:, 2].mean())])
        at[f"elbow_{side}"] = (at[f"arm_{side}"] + at[f"hand_{side}"]) / 2
        # Feet: the lowest vertices on each side.
        sole = p[p[:, 1] < 0.06 * h]
        mine = sole[(sole[:, 0] - centre) * sign > 0]
        foot = np.array([float(np.median(mine[:, 0])), 0.05, float(np.median(mine[:, 2]))])
        at[f"ankle_{side}"] = foot
        at[f"leg_{side}"] = np.array([centre + sign * 0.05 * h, at["hips"][1], depth])
        at[f"knee_{side}"] = (at[f"leg_{side}"] + foot) / 2
    for name, value in config.get("joints", {}).items():
        at[name] = np.array(value, dtype=float)
    for side in "lr":  # keep the derived joints between their ends when only an end was given
        for joint, (a, b) in {"elbow": ("arm", "hand"), "knee": ("leg", "ankle")}.items():
            if f"{joint}_{side}" not in config.get("joints", {}):
                at[f"{joint}_{side}"] = (at[f"{a}_{side}"] + at[f"{b}_{side}"]) / 2
    return at


def bones(at):
    """`(joint, start, end)` of the bone each joint owns."""
    return [
        (SPINE, at["hips"], at["neck"]),
        (HEAD, at["neck"], at["head_top"]),
        (ARM_L, at["arm_l"], at["elbow_l"]), (ELBOW_L, at["elbow_l"], at["hand_l"]),
        (ARM_R, at["arm_r"], at["elbow_r"]), (ELBOW_R, at["elbow_r"], at["hand_r"]),
        (LEG_L, at["leg_l"], at["knee_l"]), (KNEE_L, at["knee_l"], at["ankle_l"]),
        (LEG_R, at["leg_r"], at["knee_r"]), (KNEE_R, at["knee_r"], at["ankle_r"]),
    ]


def joint_positions(at) -> np.ndarray:
    pos = np.zeros((11, 3))
    pos[HIPS] = at["hips"]
    pos[SPINE] = at["hips"]
    pos[HEAD] = at["neck"]
    for side, (arm, elbow, leg, knee) in (("l", (ARM_L, ELBOW_L, LEG_L, KNEE_L)), ("r", (ARM_R, ELBOW_R, LEG_R, KNEE_R))):
        pos[arm], pos[elbow] = at[f"arm_{side}"], at[f"elbow_{side}"]
        pos[leg], pos[knee] = at[f"leg_{side}"], at[f"knee_{side}"]
    return pos


# ---------------------------------------------------------------------------------------------
# The skin
# ---------------------------------------------------------------------------------------------

def segment_distance(p, a, b, ray=False):
    """The distance from each point of `p` to the segment a-b (`ray`: to the half line from a
    through b, so what lies beyond the end, a hand's fingers, still belongs to the bone)."""
    ab = b - a
    t = ((p - a) @ ab) / max(float(ab @ ab), 1e-12)
    t = np.maximum(t, 0.0) if ray else np.clip(t, 0.0, 1.0)
    return np.linalg.norm(p - (a + t[:, None] * ab), axis=1)


def welded(mesh):
    """`(ids, count)`: a number per vertex, shared by the vertices that sit at the same place
    (the texture seams split them) so that the skin treats a seam as no seam."""
    key = np.round(mesh.positions.astype(np.float64) * 1e4).astype(np.int64)
    _, ids = np.unique(key, axis=0, return_inverse=True)
    ids = ids.reshape(-1)
    return ids, int(ids.max()) + 1


def diffuse(mesh, score: np.ndarray, rounds: int, keep: float) -> np.ndarray:
    """Smooths per-vertex scores over the surface: each round every vertex keeps `keep` of
    its own and takes the rest from the mean of its neighbours. A hair lock or a piece of
    cloth, joined to nothing else, ends up moving as one; a body keeps its local joints."""
    from scipy.sparse import coo_matrix

    ids, count = welded(mesh)
    t = ids[mesh.triangles]
    rows = np.concatenate([t[:, 0], t[:, 1], t[:, 2], t[:, 1], t[:, 2], t[:, 0]])
    cols = np.concatenate([t[:, 1], t[:, 2], t[:, 0], t[:, 0], t[:, 1], t[:, 2]])
    adjacency = coo_matrix((np.ones(len(rows)), (rows, cols)), shape=(count, count)).tocsr()
    adjacency.data[:] = 1.0
    degree = np.asarray(adjacency.sum(axis=1)).reshape(-1)
    degree[degree == 0] = 1.0
    # The score of a welded vertex: the mean of the vertices at that place.
    sums = np.zeros((count, score.shape[1]))
    np.add.at(sums, ids, score)
    sums /= np.bincount(ids, minlength=count)[:, None]
    for _ in range(rounds):
        sums = keep * sums + (1.0 - keep) * (adjacency @ sums) / degree[:, None]
    return sums[ids]


def skin(mesh, at, config: dict):
    """Per vertex the four strongest joints and their weights: each vertex leans to the bones
    near it (`fade`, metres) and then the surface smooths that, so what hangs together moves
    together (a lock of hair stays with the head, a limb bends at its joints)."""
    p = mesh.positions.astype(np.float64)
    fade = config.get("fade", 0.05)
    trunk = config.get("trunk", 0.03)
    distal = (ELBOW_L, ELBOW_R, KNEE_L, KNEE_R)
    distance = np.full((len(p), 11), np.inf)
    for joint, start, end in bones(at):
        distance[:, joint] = segment_distance(p, start, end, ray=joint in distal)
    # The hips stand for the pelvis: from the hip joints down to the crotch.
    pelvis = at["hips"] - [0.0, config.get("pelvis", 0.18), 0.0]
    distance[:, HIPS] = segment_distance(p, pelvis, at["hips"])
    for joint in (HIPS, SPINE, HEAD):
        distance[:, joint] -= trunk
    nearest = distance.min(axis=1, keepdims=True)
    score = np.exp(-(((distance - nearest) / fade) ** 2))
    score = diffuse(mesh, score, config.get("rounds", 40), config.get("keep", 0.5))
    top = np.argsort(score, axis=1)[:, ::-1][:, :4]
    weights = np.take_along_axis(score, top, axis=1)
    weights /= weights.sum(axis=1, keepdims=True)
    return top.astype(np.uint8), weights.astype(np.float32)


# ---------------------------------------------------------------------------------------------
# The movements
# ---------------------------------------------------------------------------------------------

def world_angles(pitch):
    """The world pitch of each joint of the puppet: the sum of its chain."""
    angle = np.zeros(11)
    for joint in range(11):
        angle[joint] = pitch[joint] + (angle[PARENT[joint]] if PARENT[joint] >= 0 else 0.0)
    return angle


def retarget(clips: dict, at, scale_hips: float):
    """For each clip, the keys `(time, hips translation, local rotation per joint)` that make
    every bone of the model point where the puppet's does."""
    pos = joint_positions(at)
    bind = np.zeros((11, 3))
    for joint, start, end in bones(at):
        bind[joint] = (end - start) / np.linalg.norm(end - start)
    out = []
    for clip in clips["clips"]:
        keys: list = []
        for key in clip["keys"]:
            angle = world_angles(key["pitch"])
            world = [np.array([0.0, 0.0, 0.0, 1.0])] * 11
            for joint in range(1, 11):
                target = rot_x(angle[joint], REST[joint])
                world[joint] = q_arc(bind[joint], target)
            local = []
            for joint in range(11):
                parent = PARENT[joint]
                q = world[joint] if parent < 0 else q_mul(q_conj(world[parent]), world[joint])
                local.append(q / np.linalg.norm(q))
            hips = np.array([pos[HIPS][0], key["hips"][1] * scale_hips, pos[HIPS][2] + key["hips"][2] * scale_hips])
            if keys:  # the shorter way round between two keys: no flipped signs
                local = [q if float(np.dot(q, before)) >= 0 else -q for q, before in zip(local, keys[-1][2])]
            keys.append((key["t"], hips, local))
        out.append((clip["name"], clip["seconds"], keys))
    return out, pos


def pose_vertices(mesh, joints, weights, pos, hips, local):
    """The mesh skinned by one key: the software twin of the engine's skinning."""
    world_q = [None] * 11
    world_p = np.zeros((11, 3))
    for joint in range(11):
        parent = PARENT[joint]
        if parent < 0:
            world_q[joint], world_p[joint] = local[joint], hips
        else:
            world_q[joint] = q_mul(world_q[parent], local[joint])
            world_p[joint] = world_p[parent] + q_rotate(world_q[parent], pos[joint] - pos[parent])
    p = mesh.positions.astype(np.float64)
    out = np.zeros_like(p)
    for slot in range(4):
        for joint in np.unique(joints[:, slot]):
            rows = (joints[:, slot] == joint) & (weights[:, slot] > 0)
            moved = q_rotate(world_q[joint], p[rows] - pos[joint]) + world_p[joint]
            out[rows] += weights[rows, slot, None] * moved
    return out


# ---------------------------------------------------------------------------------------------
# The glTF binary
# ---------------------------------------------------------------------------------------------

def write(path: Path, mesh, joints, weights, pos, animations) -> None:
    b = glbio.Bin()
    position = b.add(mesh.positions.astype(np.float32), "VEC3", 5126, 34962, bounds=True)
    normal = b.add(mesh.normals.astype(np.float32), "VEC3", 5126, 34962)
    uv = b.add(mesh.uvs.astype(np.float32), "VEC2", 5126, 34962)
    joint_acc = b.add(joints.astype(np.uint8), "VEC4", 5121, 34962)
    weight_acc = b.add(weights.astype(np.float32), "VEC4", 5126, 34962)
    index = b.add(mesh.triangles.astype(np.uint32).reshape(-1), "SCALAR", 5125, 34963)
    inverse = np.zeros((11, 16), dtype=np.float32)
    for joint in range(11):
        matrix = np.eye(4, dtype=np.float32)
        matrix[:3, 3] = -pos[joint]
        inverse[joint] = matrix.T.reshape(-1)  # column-major
    ibm = b.add(inverse, "MAT4", 5126)
    clips = []
    for name, seconds, keys in animations:
        times = np.array([k[0] for k in keys], dtype=np.float32)
        t_acc = b.add(times, "SCALAR", 5126, bounds=True)
        samplers, channels = [], []
        for joint in range(11):
            values = np.array([k[2][joint] for k in keys], dtype=np.float32)
            samplers.append({"input": t_acc, "interpolation": "LINEAR", "output": b.add(values, "VEC4", 5126)})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": joint + 1, "path": "rotation"}})
        values = np.array([k[1] for k in keys], dtype=np.float32)
        samplers.append({"input": t_acc, "interpolation": "LINEAR", "output": b.add(values, "VEC3", 5126)})
        channels.append({"sampler": len(samplers) - 1, "target": {"node": 1, "path": "translation"}})
        clips.append({"name": name, "channels": channels, "samplers": samplers})
    image = b.view(mesh.texture)
    nodes = [{"name": "root", "children": [1, 12]}]
    for joint in range(11):
        offset = pos[joint] if PARENT[joint] < 0 else pos[joint] - pos[PARENT[joint]]
        node = {"name": JOINTS[joint], "translation": [round(float(v), 5) for v in offset]}
        kids = [c + 1 for c in range(11) if PARENT[c] == joint]
        if kids:
            node["children"] = kids
        nodes.append(node)
    nodes.append({"name": "body", "mesh": 0, "skin": 0})
    doc = {
        "asset": {"version": "2.0", "generator": GENERATOR},
        "scene": 0, "scenes": [{"name": "model", "nodes": [0]}], "nodes": nodes,
        "meshes": [{"name": "body", "primitives": [{"attributes": {
            "POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv, "JOINTS_0": joint_acc, "WEIGHTS_0": weight_acc},
            "indices": index, "material": 0}]}],
        "skins": [{"name": "skeleton", "inverseBindMatrices": ibm, "skeleton": 1, "joints": list(range(1, 12))}],
        "materials": [{"name": "body", "doubleSided": True, "pbrMetallicRoughness": {
            "baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0,
            "roughnessFactor": 1.0}}],
        "textures": [{"sampler": 0, "source": 0}],
        "images": [{"name": "texture", "mimeType": "image/png", "bufferView": image}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 33071, "wrapT": 33071}],
        "animations": clips, "accessors": b.accessors, "bufferViews": b.views,
        "buffers": [{"byteLength": len(b.bytes)}],
    }
    Path(path).write_bytes(glbio.pack(doc, bytes(b.bytes)))


# ---------------------------------------------------------------------------------------------
# A look without a renderer: points splatted with their texture colour
# ---------------------------------------------------------------------------------------------

def texture_colours(mesh):
    import io

    from PIL import Image

    image = np.asarray(Image.open(io.BytesIO(mesh.texture)).convert("RGB"))
    h, w = image.shape[:2]
    u = np.clip((mesh.uvs[:, 0] * w).astype(int), 0, w - 1)
    v = np.clip((mesh.uvs[:, 1] * h).astype(int), 0, h - 1)
    return image[v, u]


def splat(points, colours, view: str, size=(300, 400), box=(-1.2, 1.2, -0.1, 2.0), skeleton=None):
    """Orthographic dots: `front` looks down -z at x/y, `side` looks at z/y."""
    from PIL import Image, ImageDraw

    u = points[:, 0] if view == "front" else -points[:, 2]
    depth = points[:, 2] if view == "front" else points[:, 0]
    order = np.argsort(depth)
    img = Image.new("RGB", size, (60, 60, 66))
    px = ((u - box[0]) / (box[1] - box[0]) * size[0]).astype(int)
    py = ((box[3] - points[:, 1]) / (box[3] - box[2]) * size[1]).astype(int)
    pixels = img.load()
    for i in order:
        x, y = px[i], py[i]
        for dx in (0, 1):
            for dy in (0, 1):
                if 0 <= x + dx < size[0] and 0 <= y + dy < size[1]:
                    pixels[x + dx, y + dy] = tuple(int(c) for c in colours[i])
    if skeleton is not None:
        draw = ImageDraw.Draw(img)
        for a, b in skeleton:
            ua, ub = (a[0], b[0]) if view == "front" else (-a[2], -b[2])
            pa = ((ua - box[0]) / (box[1] - box[0]) * size[0], (box[3] - a[1]) / (box[3] - box[2]) * size[1])
            pb = ((ub - box[0]) / (box[1] - box[0]) * size[0], (box[3] - b[1]) / (box[3] - box[2]) * size[1])
            draw.line([pa, pb], fill=(0, 255, 120), width=2)
            draw.ellipse([pa[0] - 3, pa[1] - 3, pa[0] + 3, pa[1] + 3], outline=(255, 255, 0))
    return img


def fit_view(prefix: Path, mesh, at, view: str = "front", size=(1040, 800), box=(-1.3, 1.3, -0.05, 1.95)):
    """The standing mesh with a 10 cm grid (labelled every 50 cm) and the fitted joints: to read
    the coordinates for a `--config` off."""
    from PIL import ImageDraw

    colours = texture_colours(mesh)
    img = splat(mesh.positions, colours, view, size=size, box=box, skeleton=skeleton_lines(at))
    draw = ImageDraw.Draw(img)
    sx = size[0] / (box[1] - box[0])
    sy = size[1] / (box[3] - box[2])
    for i in range(int(box[0] * 10), int(box[1] * 10) + 1):
        x = (i / 10 - box[0]) * sx
        major = i % 5 == 0
        draw.line([(x, 0), (x, size[1])], fill=(110, 110, 140) if major else (80, 80, 90), width=1)
        if major:
            draw.text((x + 2, size[1] - 12), f"{i / 10:.1f}", fill=(255, 255, 255))
    for i in range(int(box[2] * 10), int(box[3] * 10) + 1):
        y = (box[3] - i / 10) * sy
        major = i % 5 == 0
        draw.line([(0, y), (size[0], y)], fill=(110, 110, 140) if major else (80, 80, 90), width=1)
        if major:
            draw.text((2, y - 12), f"{i / 10:.1f}", fill=(255, 255, 255))
    img.save(f"{prefix}_{view}.png")


def skeleton_lines(at):
    return [(start, end) for _, start, end in bones(at)] + [(at["hips"], at["leg_l"]), (at["hips"], at["leg_r"]),
                                                              (at["neck"], at["arm_l"]), (at["neck"], at["arm_r"])]


JOINT_COLOURS = np.array([(200, 200, 200), (255, 140, 0), (255, 255, 0), (0, 200, 255), (0, 90, 255), (255, 0, 200),
                          (170, 0, 120), (0, 255, 90), (0, 150, 60), (255, 80, 80), (150, 30, 30)], dtype=np.uint8)


def weights_view(prefix: Path, mesh, joints, weights, pos, animations, name="idle"):
    """Vertices coloured by the joint that holds them most, bind pose and one clip, front and side."""
    from PIL import Image

    colours = JOINT_COLOURS[joints[:, 0]]
    key = next(a for a in animations if a[0] == name)[2][0]
    moved = pose_vertices(mesh, joints, weights, pos, key[1], key[2])
    tiles = [splat(mesh.positions, colours, "front", size=(520, 400), box=(-1.3, 1.3, -0.1, 1.9)),
             splat(moved, colours, "front", size=(520, 400), box=(-1.3, 1.3, -0.1, 1.9)),
             splat(moved, colours, "side", size=(520, 400), box=(-1.3, 1.3, -0.1, 1.9))]
    sheet = Image.new("RGB", (520 * 3, 400))
    for i, tile in enumerate(tiles):
        sheet.paste(tile, (i * 520, 0))
    sheet.save(f"{prefix}_weights.png")


def preview(prefix: Path, mesh, joints, weights, pos, animations, at, names=("idle", "punch", "kick", "ko", "walk_f", "mount")):
    from PIL import Image

    colours = texture_colours(mesh)
    tiles = [splat(mesh.positions, colours, "front", skeleton=skeleton_lines(at)),
             splat(mesh.positions, colours, "side", skeleton=skeleton_lines(at))]
    by_name = {a[0]: a for a in animations}
    for name in names:
        if name not in by_name:
            continue
        keys = by_name[name][2]
        key = keys[len(keys) // 2] if name != "punch" and name != "kick" else keys[2]
        moved = pose_vertices(mesh, joints, weights, pos, key[1], key[2])
        tiles.append(splat(moved, colours, "side"))
    sheet = Image.new("RGB", (300 * min(len(tiles), 4), 400 * ((len(tiles) + 3) // 4)))
    for i, tile in enumerate(tiles):
        sheet.paste(tile, ((i % 4) * 300, (i // 4) * 400))
    sheet.save(f"{prefix}_sheet.png")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("model", type=Path)
    ap.add_argument("output", type=Path, nargs="?")
    ap.add_argument("--clips", type=Path, required=True)
    ap.add_argument("--config", type=Path, help="JSON: {yaw, joints, reach, fade}")
    ap.add_argument("--preview", type=Path, help="write <prefix>_sheet.png: the fit and a few keys")
    ap.add_argument("--weights", type=Path, help="write <prefix>_weights.png: who holds which vertex")
    ap.add_argument("--fit", type=Path, help="write <prefix>_front.png / _side.png: the joints on a metre grid")
    a = ap.parse_args()
    config = json.loads(a.config.read_text()) if a.config else {}
    clips = json.loads(a.clips.read_text())

    mesh = load(a.model, config.get("yaw", 0.0))
    scale = stand(mesh, config)
    at = fit(mesh, config)
    joints, weights = skin(mesh, at, config)
    puppet_hips = clips["joints"][0]["offset"][1]  # the puppet's hips at rest, in metres
    animations, pos = retarget(clips, at, scale_hips=float(at["hips"][1]) / puppet_hips)
    print(f"model: {len(mesh.positions)} vertices, scale {scale:.3f}, hips at {at['hips'][1]:.3f} m")
    if a.fit:
        fit_view(a.fit, mesh, at, "front")
        fit_view(a.fit, mesh, at, "side")
    if a.weights:
        weights_view(a.weights, mesh, joints, weights, pos, animations)
    if a.preview:
        preview(a.preview, mesh, joints, weights, pos, animations, at)
    if a.output:
        write(a.output, mesh, joints, weights, pos, animations)
        print(f"wrote {a.output} ({a.output.stat().st_size / 1e6:.2f} MB, {len(animations)} clips)")


if __name__ == "__main__":
    main()
