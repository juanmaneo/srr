#!/usr/bin/env python3
"""Queue: image -> TRELLIS glb for the ten manga girls. Waits out the ZeroGPU quota, keeps going, never prints the token.

Outputs: out/<name>_<cell>.glb (accepted: has real depth) or out/rejected/<name>_<cell>_<seed>.glb
(flat billboard), under the work directory (`$SRR_WORK`, default ~/.cache/srr-work). The crops of
the sheets go to input/ there. State is the file system, so the farm can be restarted at any time.
Run it detached from the editor, e.g. `systemd-run --user python3 -u tools/trellis/farm.py`.
"""
import re, shutil, subprocess, sys, time, os, struct, json
from pathlib import Path

W = Path(os.environ.get("SRR_WORK", Path.home() / ".cache/srr-work"))
REPO = Path(__file__).resolve().parents[2]
NAMES = "brunette blonde redhead pink white black green violet cyan raven".split()
# (cell of the sheet, seed): the arms-spread "hit" cell rigs best, PunchHigh is the fallback.
PLAN = [("c3r2", 1234), ("c3r2", 4321), ("c3r2", 7), ("c2r1", 1234), ("c2r1", 4321)]
DEADLINE = time.time() + 72 * 3600

def done(name):
    return any((W / "out" / f"{name}_{cell}.glb").exists() for cell in ("c3r2", "c2r1"))

def log(*a):
    print(time.strftime("%F %T"), *a, flush=True)

def bbox(glb: Path):
    b = glb.read_bytes()
    n = struct.unpack("<I", b[12:16])[0]
    j = json.loads(b[20:20 + n])
    mn = [1e9] * 3; mx = [-1e9] * 3
    for m in j["meshes"]:
        for p in m["primitives"]:
            a = j["accessors"][p["attributes"]["POSITION"]]
            mn = [min(x, y) for x, y in zip(mn, a["min"])]
            mx = [max(x, y) for x, y in zip(mx, a["max"])]
    return [y - x for x, y in zip(mn, mx)]

def deep_enough(glb: Path) -> bool:
    """The figure (the plates dropped) must have a real depth, not be a billboard."""
    sys.path.insert(0, str(REPO / "tools/trellis"))
    import glbio, clean
    mesh = glbio.read(glb)
    mesh, info = clean.drop_plates(mesh)
    mesh, _ = clean.drop_tiny(mesh)
    if len(mesh.triangles) < 500:
        log("   almost nothing left after the plates", info)
        return False
    low, high = mesh.positions.min(0), mesh.positions.max(0)
    size = high - low
    log("   figure size", [round(float(v), 3) for v in size], "plates", info["plates"])
    return float(size.min()) >= 0.10 * float(size.max())

def prepare_rig(name, cell):
    """Draws the fit grids of an accepted mesh (`out/<name>_fit_front.png`, `_side.png`: the joints the
    rigger measured on a 10 cm grid), so that reading the joints off them is the only manual step
    left (see README.md). Never fatal: the mesh is kept whatever happens here."""
    clips = W / "clips.json"
    if not clips.exists():
        log("   no", clips, "(cargo run --no-default-features --example models -- --clips): no fit grids")
        return
    cmd = ["uv", "run", str(REPO / "tools/trellis/rig.py"), str(W / "out" / f"{name}_{cell}.glb"),
           "--clips", str(clips), "--fit", str(W / "out" / f"{name}_{cell}_fit")]
    env = dict(os.environ, UV_CACHE_DIR=str(W / "uv-cache"))
    try:
        p = subprocess.run(cmd, cwd=REPO, env=env, capture_output=True, text=True, timeout=900)
        log("   fit grids", "written" if p.returncode == 0 else f"failed ({p.returncode}): {p.stderr[-300:]}")
    except (OSError, subprocess.SubprocessError) as error:
        log("   fit grids failed:", error)


def crop(name, cell):
    """The cell `c<column>r<row>` of the manga sheet of `name` (cells of 240x320), Lanczos 3x."""
    from PIL import Image
    img = W / "input" / f"{name}_{cell}.png"
    if not img.exists():
        column, row = int(cell[1]), int(cell[3])
        sheet = Image.open(REPO / "assets/skins/manga" / f"{name}.png").convert("RGBA")
        box = (240 * column, 320 * row, 240 * (column + 1), 320 * (row + 1))
        img.parent.mkdir(parents=True, exist_ok=True)
        sheet.crop(box).resize((720, 960), Image.LANCZOS).save(img)
    return img


def run_one(name, cell, seed):
    img = crop(name, cell)
    tmp = W / "out" / f"{name}_try.glb"
    cmd = ["uv", "run", str(REPO / "tools/trellis/trellis_glb.py"), str(img), str(tmp), "--seed", str(seed),
           "--ss-steps", "25", "--slat-steps", "25", "--texture-size", "1024", "--simplify", "0.95",
           "--timeout", "1700", "--use-token"]
    env = dict(os.environ, UV_CACHE_DIR=str(W / "uv-cache"))
    p = subprocess.run(cmd, cwd=REPO, env=env, capture_output=True, text=True)
    out = p.stdout + p.stderr
    last = [l for l in out.splitlines() if l.strip()][-3:]
    log("   exit", p.returncode, " | ".join(last)[-400:])
    return p.returncode, out, tmp

def main():
    (W / "out" / "rejected").mkdir(parents=True, exist_ok=True)
    while time.time() < DEADLINE:
        todo = [n for n in NAMES if not done(n)]
        if not todo:
            log("ALL DONE"); return
        log("missing:", " ".join(todo))
        quota = False
        for name in todo:
            tries = len(list((W / "out" / "rejected").glob(f"{name}_*.glb")))
            if tries >= len(PLAN):
                log(name, "gave up after", tries, "flat results"); continue
            cell, seed = PLAN[tries]
            log("generate", name, cell, "seed", seed)
            code, out, tmp = run_one(name, cell, seed)
            if code == 0 and tmp.exists():
                if deep_enough(tmp):
                    shutil.move(tmp, W / "out" / f"{name}_{cell}.glb")
                    for ext in (".mp4",):
                        s = tmp.with_suffix(ext)
                        if s.exists(): shutil.move(s, W / "out" / f"{name}_{cell}{ext}")
                    pre = tmp.with_name(tmp.stem + "_preprocessed.png")
                    if pre.exists(): shutil.move(pre, W / "out" / f"{name}_{cell}_preprocessed.png")
                    log("   ACCEPTED", name)
                    prepare_rig(name, cell)
                else:
                    shutil.move(tmp, W / "out" / "rejected" / f"{name}_{cell}_{seed}.glb")
                    log("   flat, rejected", name, seed)
                continue
            m = re.search(r"Try again in (\d+):(\d+):(\d+)", out)
            if "quota" in out.lower():
                wait = 1200
                if m:
                    h, mi, s = map(int, m.groups())
                    wait = max(1200, min(h * 3600 + mi * 60 + s + 120, 3 * 3600))
                log("   quota exhausted, sleeping", wait, "s")
                time.sleep(wait)
                quota = True
                break
            log("   other failure, pausing 10 min"); time.sleep(600); break
        if all(done(n) or len(list((W / "out" / "rejected").glob(f"{n}_*.glb"))) >= len(PLAN) for n in NAMES):
            log("nothing left to try"); return
    log("deadline reached")

main()
