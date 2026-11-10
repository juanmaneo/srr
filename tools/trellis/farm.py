#!/usr/bin/env python3
"""Queue: image -> TRELLIS glb for the ten manga girls. Waits out the ZeroGPU quota, keeps going, never prints the token.

A *recipe* is a way of asking: which Space (TRELLIS or TRELLIS.2) and whether the sprite is shaded
as an inflated figure first (`inflate.py`). TRELLIS reads a flat cel drawing as a poster and returns
a thin standee ("flat", rejected); one try in six or so used to work. Every attempt is recorded in
`state.json`, and the next one uses the recipe with the best record (Laplace score, so a recipe
nobody tried yet beats one that mostly failed). The state is the file system plus that file, so the
farm can be restarted at any time.

Outputs, under the work directory (`$SRR_WORK`, default ~/.cache/srr-work):
    out/<name>_<cell>.glb                    accepted: a figure with real depth (TRELLIS.2 meshes decimated)
    out/rejected/<name>_<cell>[_<recipe>]_<seed>.glb   flat billboards
    out/raw/<name>_<cell>_<recipe>_<seed>.glb          what TRELLIS.2 returned before the decimation
    out/prev/<name>_<cell>_<recipe>_<seed>_{clay,shaded}.jpg   previews of a TRELLIS.2 call
The crops of the sheets go to input/. Run it detached from the editor (see README.md).
"""
import json, os, re, shutil, subprocess, struct, sys, time
from pathlib import Path

W = Path(os.environ.get("SRR_WORK", Path.home() / ".cache/srr-work"))
REPO = Path(__file__).resolve().parents[2]
TOOLS = REPO / "tools/trellis"
NAMES = "brunette blonde redhead pink white black green violet cyan raven".split()
CELLS = ("c3r2", "c2r1")
INFLATE = 0.8
# Recipe name -> (space, inflate strength). Order breaks ties between equal scores.
RECIPES = {"t2i": ("trellis2", INFLATE), "t2": ("trellis2", 0.0), "t1i": ("trellis1", INFLATE), "t1": ("trellis1", 0.0)}
# (cell of the sheet, seed): the arms-spread "hit" cell rigs best, PunchHigh is the fallback.
CANDIDATES = [("c3r2", 1234), ("c3r2", 4321), ("c2r1", 1234), ("c3r2", 7), ("c2r1", 4321)]
CAP = 10  # attempts per girl before the farm leaves her alone
TARGET_FACES = 16000  # what a TRELLIS.2 mesh is decimated to
DEADLINE = time.time() + 72 * 3600
STATE = W / "state.json"


def log(*a):
    print(time.strftime("%F %T"), *a, flush=True)


# ---------------------------------------------------------------------------------------------
# State: every attempt, `{girl, recipe, cell, seed, result, depth, time}`
# ---------------------------------------------------------------------------------------------

def migrate():
    """The first run after this file got recipes: the plain TRELLIS results already on disk."""
    attempts = []
    for glb in sorted((W / "out" / "rejected").glob("*.glb")):
        m = re.fullmatch(r"([a-z]+)_(c\dr\d)_(?:(t\d\w*)_)?(\d+)", glb.stem)
        if m:
            attempts.append({"girl": m[1], "recipe": m[3] or "t1", "cell": m[2], "seed": int(m[4]), "result": "flat"})
    for name in NAMES:
        for cell in CELLS:
            if (W / "out" / f"{name}_{cell}.glb").exists():
                attempts.append({"girl": name, "recipe": "t1", "cell": cell, "seed": 1234, "result": "ok"})
    return attempts


def load():
    if STATE.exists():
        return json.loads(STATE.read_text())
    attempts = migrate()
    save(attempts)
    return attempts


def save(attempts):
    tmp = STATE.with_suffix(".tmp")
    tmp.write_text(json.dumps(attempts, indent=1))
    tmp.replace(STATE)


def score(attempts, recipe):
    mine = [a for a in attempts if a["recipe"] == recipe and a["result"] in ("ok", "flat")]
    return (sum(a["result"] == "ok" for a in mine) + 1) / (len(mine) + 2)


def done(name):
    return any((W / "out" / f"{name}_{cell}.glb").exists() for cell in CELLS)


def pick(attempts):
    """`(girl, recipe, cell, seed)` to try next, or None when nothing is left."""
    todo = [n for n in NAMES if not done(n)]
    tried = {(a["girl"], a["recipe"], a["cell"], a["seed"]) for a in attempts}
    count = lambda girl: sum(a["girl"] == girl and a["result"] in ("ok", "flat") for a in attempts)
    best = sorted(RECIPES, key=lambda r: (-score(attempts, r), list(RECIPES).index(r)))
    for recipe in best:
        # The girls this recipe has tried least first, so that a recipe is judged on several of them.
        mine = lambda girl: sum(a["girl"] == girl and a["recipe"] == recipe for a in attempts)
        for girl in sorted(todo, key=lambda g: (mine(g), NAMES.index(g))):
            if count(girl) >= CAP:
                continue
            for cell, seed in CANDIDATES:
                if (girl, recipe, cell, seed) not in tried:
                    return girl, recipe, cell, seed
    return None


# ---------------------------------------------------------------------------------------------
# Meshes
# ---------------------------------------------------------------------------------------------

def deep_enough(glb: Path) -> tuple[bool, float]:
    """The figure (the plates dropped) must have a real depth, not be a billboard."""
    sys.path.insert(0, str(TOOLS))
    import glbio, clean
    mesh = glbio.read(glb)
    mesh, info = clean.drop_plates(mesh)
    mesh, _ = clean.drop_tiny(mesh)
    if len(mesh.triangles) < 500:
        log("   almost nothing left after the plates", info)
        return False, 0.0
    size = mesh.positions.max(0) - mesh.positions.min(0)
    log("   figure size", [round(float(v), 3) for v in size], "plates", info["plates"])
    depth = float(size.min()) / float(size.max())
    return depth >= 0.10, depth


def decimate(raw: Path, out: Path) -> bool:
    """Blender's collapse decimation to `TARGET_FACES`, PNG texture, smooth normals."""
    cmd = ["blender", "-b", "--python", str(TOOLS / "blender_decimate.py"), "--", str(raw), str(out), str(TARGET_FACES)]
    try:
        p = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True, timeout=900)
    except (OSError, subprocess.SubprocessError) as error:
        log("   decimation failed:", error)
        return False
    line = next((l for l in p.stdout.splitlines() if l.startswith("DECIMATE")), "")
    log("   ", line or f"decimation failed ({p.returncode}): {(p.stdout + p.stderr)[-300:]}")
    return p.returncode == 0 and out.exists()


def prepare_rig(name, cell):
    """Draws the fit grids of an accepted mesh (`out/<name>_<cell>_fit_front.png`, `_side.png`: the joints the
    rigger measured on a 10 cm grid), so that reading the joints off them is the only manual step
    left (see README.md). Never fatal: the mesh is kept whatever happens here."""
    clips = W / "clips.json"
    if not clips.exists():
        log("   no", clips, "(cargo run --no-default-features --example models -- --clips): no fit grids")
        return
    cmd = ["uv", "run", str(TOOLS / "rig.py"), str(W / "out" / f"{name}_{cell}.glb"),
           "--clips", str(clips), "--fit", str(W / "out" / f"{name}_{cell}_fit")]
    env = dict(os.environ, UV_CACHE_DIR=str(W / "uv-cache"))
    try:
        p = subprocess.run(cmd, cwd=REPO, env=env, capture_output=True, text=True, timeout=900)
        log("   fit grids", "written" if p.returncode == 0 else f"failed ({p.returncode}): {p.stderr[-300:]}")
    except (OSError, subprocess.SubprocessError) as error:
        log("   fit grids failed:", error)


def crop(name, cell, inflate):
    """The cell `c<column>r<row>` of the manga sheet of `name` (cells of 240x320), Lanczos 3x,
    shaded as an inflated figure when `inflate` > 0."""
    from PIL import Image
    suffix = f"_i{round(inflate * 100)}" if inflate else ""
    img = W / "input" / f"{name}_{cell}{suffix}.png"
    if not img.exists():
        column, row = int(cell[1]), int(cell[3])
        sheet = Image.open(REPO / "assets/skins/manga" / f"{name}.png").convert("RGBA")
        box = (240 * column, 320 * row, 240 * (column + 1), 320 * (row + 1))
        img.parent.mkdir(parents=True, exist_ok=True)
        picture = sheet.crop(box).resize((720, 960), Image.LANCZOS)
        if inflate:
            sys.path.insert(0, str(TOOLS))
            from inflate import inflate as shade
            picture = shade(picture, inflate)
        picture.save(img)
    return img


def run_one(name, recipe, cell, seed):
    """Returns `(exit code, output, raw glb path)`; exit 4 is TRELLIS.2 seeing a flat preview."""
    space, inflate = RECIPES[recipe]
    img = crop(name, cell, inflate)
    tmp = W / "out" / f"{name}_try.glb"
    env = dict(os.environ, UV_CACHE_DIR=str(W / "uv-cache"))
    if space == "trellis1":
        cmd = ["uv", "run", str(TOOLS / "trellis_glb.py"), str(img), str(tmp), "--seed", str(seed),
               "--ss-steps", "25", "--slat-steps", "25", "--texture-size", "1024", "--simplify", "0.95",
               "--timeout", "1700", "--use-token"]
    else:
        prefix = W / "out" / "prev" / f"{name}_{cell}_{recipe}_{seed}"
        cmd = ["uv", "run", str(TOOLS / "trellis2_glb.py"), str(img), str(tmp), "--seed", str(seed),
               "--resolution", "512", "--texture-size", "1024", "--timeout", "1700", "--use-token",
               "--preview", str(prefix)]
    p = subprocess.run(cmd, cwd=REPO, env=env, capture_output=True, text=True)
    out = p.stdout + p.stderr
    last = [l for l in out.splitlines() if l.strip()][-3:]
    log("   exit", p.returncode, " | ".join(last)[-400:])
    return p.returncode, out, tmp


def settle(attempts, name, recipe, cell, seed, code, tmp):
    """Records what a finished call gave. Returns the result string."""
    space, _ = RECIPES[recipe]
    stem = f"{name}_{cell}_{seed}" if recipe == "t1" else f"{name}_{cell}_{recipe}_{seed}"
    result, depth = "flat", 0.0
    if code == 4:
        log("   flat in the preview, rejected", name, recipe, seed)
    elif code == 0 and tmp.exists():
        mesh = tmp
        if space == "trellis2":
            (W / "out" / "raw").mkdir(parents=True, exist_ok=True)
            raw = W / "out" / "raw" / f"{stem}.glb"
            shutil.move(tmp, raw)
            mesh = W / "out" / f"{name}_try_dec.glb"
            if not decimate(raw, mesh):
                mesh = None
        ok, depth = deep_enough(mesh) if mesh else (False, 0.0)
        if ok:
            shutil.move(mesh, W / "out" / f"{name}_{cell}.glb")
            for ext in (".mp4",):
                s = tmp.with_suffix(ext)
                if s.exists(): shutil.move(s, W / "out" / f"{name}_{cell}{ext}")
            pre = tmp.with_name(tmp.stem + "_preprocessed.png")
            if pre.exists(): shutil.move(pre, W / "out" / f"{name}_{cell}_preprocessed.png")
            log("   ACCEPTED", name, recipe, "depth", round(depth, 3))
            result = "ok"
        elif mesh:
            shutil.move(mesh, W / "out" / "rejected" / f"{stem}.glb")
            log("   flat, rejected", name, recipe, seed)
    else:
        return None
    attempts.append({"girl": name, "recipe": recipe, "cell": cell, "seed": seed, "result": result,
                     "depth": round(depth, 3), "time": time.strftime("%F %T")})
    save(attempts)
    if result == "ok":
        prepare_rig(name, cell)
    return result


def main():
    failures = {}  # choice -> consecutive calls that failed for a reason other than the quota
    for sub in ("rejected", "raw", "prev"):
        (W / "out" / sub).mkdir(parents=True, exist_ok=True)
    while time.time() < DEADLINE:
        attempts = load()
        todo = [n for n in NAMES if not done(n)]
        if not todo:
            log("ALL DONE"); return
        choice = pick(attempts)
        if choice is None:
            log("nothing left to try:", " ".join(todo)); return
        name, recipe, cell, seed = choice
        log("missing:", " ".join(todo), "| scores", {r: round(score(attempts, r), 2) for r in RECIPES})
        log("generate", name, "recipe", recipe, cell, "seed", seed)
        code, out, tmp = run_one(name, recipe, cell, seed)
        if settle(attempts, name, recipe, cell, seed, code, tmp) is not None:
            continue
        m = re.search(r"Try again in (\d+):(\d+):(\d+)", out)
        if "quota" in out.lower():
            wait = 1200
            if m:
                h, mi, s = map(int, m.groups())
                wait = max(1200, min(h * 3600 + mi * 60 + s + 120, 3 * 3600))
            log("   quota exhausted, sleeping", wait, "s")
            time.sleep(wait)
            continue
        failures[choice] = failures.get(choice, 0) + 1
        if failures[choice] >= 2:
            # The same call failed twice: it may be spending GPU seconds each time. Leave it, and
            # let the next choice (and a human reading the log) take over.
            attempts.append({"girl": name, "recipe": recipe, "cell": cell, "seed": seed, "result": "error",
                             "time": time.strftime("%F %T")})
            save(attempts)
            log("   failed twice, set aside:", name, recipe, cell, seed)
        log("   other failure, pausing 10 min"); time.sleep(600)
    log("deadline reached")


if __name__ == "__main__":
    main()
