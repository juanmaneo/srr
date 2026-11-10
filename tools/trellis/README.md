# TRELLIS tools: a manga sprite -> a rigged `.glb` that plays the puppet's movements

Experiment and asset tooling, and the one exception to "the project is entirely written in Rust":
it is offline, Python and Blender, and **nothing here is used by the game, the build, `cargo xtask`,
the tests or CI**. The game only reads the `.glb` files these scripts produce
(`assets/models/manga/<variant>.glb`, see `docs/3d-asset-pipeline.md`), and audits them like any other.
Scratch files (crops, raw TRELLIS output, renders) go to a work directory outside the repo,
`$SRR_WORK` (default `~/.cache/srr-work`).

| Script               | What it does                                                                         |
|----------------------|--------------------------------------------------------------------------------------|
| `trellis_glb.py`     | image -> textured `.glb`, through the public TRELLIS Hugging Face Space              |
| `trellis2_glb.py`    | the same through the TRELLIS.2 Space (two GPU calls; stops after the first on a flat preview) |
| `inflate.py`         | shades a flat sprite as an inflated figure, to look less like a poster to TRELLIS    |
| `blender_decimate.py`| decimates a TRELLIS.2 mesh (100k+ triangles, WebP) to ~16k triangles, PNG texture    |
| `clean.py`           | drops TRELLIS's backdrop plates and flakes from a mesh                               |
| `rig.py`             | cleans, stands, skins and rigs a mesh, and gives it the 25 clips of the puppet       |
| `farm.py`            | the queue: crops the ten sheets, generates and cleans one mesh per girl, waits out the ZeroGPU quota, skips the girls that are done |
| `look.sh`            | rigs one mesh (`rig.py`) and renders a contact sheet of clips with Blender           |
| `glbio.py`           | minimal GLB reader/writer shared by the others                                       |
| `glb_info.py`        | statistics of a GLB (triangles, extents, textures, clips)                            |
| `blender_preview.py` | Blender render of a static GLB (four sides, textured and clay)                       |
| `blender_clips.py`   | Blender render of a rigged GLB at chosen moments of its clips (`idle@0 punch@0.5 ...`) |

Python 3.10+, run with `uv run <script>` (each script declares its dependencies); Blender
is only needed for the `blender_*` scripts (the two previews and the decimation).

## 1. Generate (`trellis_glb.py`)

* Space: **`trellis-community/TRELLIS`** (ZeroGPU, MIT, model `JeffreyXiang/TRELLIS-image-large`).
  The original `microsoft/TRELLIS` was in `CONFIG_ERROR` on 2026-11-08 (a torch version ZeroGPU
  does not accept), and the other working Spaces are ZeroGPU too: they share one quota.
* One call, `/generate_and_extract_glb`, is the generation and the GLB export.

```sh
# crop cell column 3, row 2 (x 720..960, y 640..960) of a manga sheet, Lanczos 3x, keep alpha
python3 -c "from PIL import Image; Image.open('assets/skins/manga/redhead.png').convert('RGBA').crop((720,640,960,960)).resize((720,960), Image.LANCZOS).save('redhead_c3r2.png')"

uv run tools/trellis/trellis_glb.py redhead_c3r2.png redhead_c3r2.glb \
    --seed 1234 --ss-steps 25 --slat-steps 25 --texture-size 1024 --simplify 0.95 --timeout 1700 --use-token
```

Next to the `.glb` it writes `<name>.mp4` (the Space's preview) and `<name>_preprocessed.png`
(what the model really saw). `--use-token` uses the usual `huggingface_hub` token lookup; the
script never reads or prints the token.

**Cells.** A manga sheet is 1200x3200: cells of 240x320, column `c`, row `r` at `(240c, 320r)`.
`c3r2` (the *hit*: arms away from the body, three-quarter view) gave the best figure to rig;
`c2r1` (a high punch) is the fallback.

**Flat results are the main failure, not bad luck with a seed.** TRELLIS reads a flat cel
drawing as a poster and returns a thin standee: a billboard, 0.02-0.05 m thick for a 1 m
figure. Of the first nine tries with the plain sprite, with 25 + 25 steps, one gave a figure
(the redhead, whose long dress and hair read as volume); the brunette gave the same slab with
three seeds. Defaults of the Space: seed 0, guidance 7.5 / 3.0, 12 + 12 steps, simplify
0.95 (0.90-0.98; 0.98 = fewest triangles), texture 1024 (512-2048).

**Recipes.** So the farm tries four ways of asking, and keeps statistics (`state.json`):

| Recipe | Space      | Input                                                      |
|--------|------------|------------------------------------------------------------|
| `t1`   | TRELLIS    | the plain sprite (the baseline: 1 success in 6 on disk)    |
| `t1i`  | TRELLIS    | the sprite shaded as an inflated figure (`inflate.py`)     |
| `t2`   | TRELLIS.2  | the plain sprite                                           |
| `t2i`  | TRELLIS.2  | the inflated sprite                                        |

`inflate.py` lights the sprite as if it were a balloon (the height is the square root of the
distance to the silhouette, light from the upper left), so every limb and lock of hair is a
rounded tube: a drawing that looks lit like a 3D render is read as a solid more often.

`trellis2_glb.py` (Space `microsoft/TRELLIS.2`, 4B model, PBR with WebP textures) is two GPU
calls: `/image_to_3d` returns 48 preview renders (8 views x 6 modes), and `/extract_glb` the GLB.
The latents stay in the *session* of the client, so both calls use the same `Client`. The
script measures the silhouette of the 8 normal-map views (a box of width w and depth t looks
`w|cos a| + t|sin a|` wide at the angle a; it fits w and t) and **stops after the first call**
when the depth is under 9 % of the height: a billboard costs one GPU call instead of two.
What it returns is 100k+ triangles; `farm.py` decimates it with `blender_decimate.py` to 16k
(collapse decimation keeps the UVs; `glbio.read` turns WebP into PNG), keeps the raw GLB under
`out/raw/`, then judges the depth like any other mesh. TRELLIS.2 meshes are one watertight
surface, without the backdrop plates and hair sheets of the first TRELLIS.

**Quota.** ZeroGPU gives each account a daily quota. What was observed with a Hugging Face
account: a call is refused ("120s requested vs. 145s left") when fewer than about 180 s are
left, i.e. 1.5 times what it asks, so a window gives about five generations of 25-45 s
(about 120 s of GPU), and the whole quota comes back 24 h after the first call of the window
(the answer says "try again in H:MM:SS"). Anonymous quota is zero, hence `--use-token`.
When it is used up the Space answers "exceeded your ZeroGPU quota ... try again in ~Nh": wait
for the refill. This is a job for a queue left to run in the background, detached from the
editor (an out-of-memory kill of a child must not take the editor down):

```sh
systemd-run --user --unit=trellis-farm -p MemoryMax=2G python3 -u tools/trellis/farm.py
```

A transient unit like that dies with a reboot. To keep the queue running across reboots, install
`srr-trellis-farm.service` (same caps: 2 GB, low priority, the out-of-memory killer takes it
down alone; restarted 10 minutes after a crash, left alone when the job is done):

```sh
mkdir -p ~/.config/systemd/user ~/.cache/srr-work/logs
cp tools/trellis/srr-trellis-farm.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now srr-trellis-farm
tail -f ~/.cache/srr-work/logs/trellis-farm.log
```

When a mesh is accepted, `farm.py` also draws its fit grids, `out/<name>_<cell>_fit_front.png`
and `_side.png` (it needs `$SRR_WORK/clips.json`, see below): reading the joints off them is
the first manual step of the rig.

`farm.py` crops the sheets itself. For each attempt it takes the recipe with the best Laplace
score `(successes + 1) / (tries + 2)` (a recipe nobody tried beats one that mostly failed), then
the girl that recipe has tried least, then the first of `(c3r2, 1234)`, `(c3r2, 4321)`,
`(c2r1, 1234)`, `(c3r2, 7)`, `(c2r1, 4321)` not tried yet for that pair. It keeps a result only
if it is deep enough after cleaning (`out/<name>_<cell>.glb`; the flat ones go to
`out/rejected/`), sleeps through the quota, and can be restarted at any time (the file system
and `state.json` are its state). After a quota window, `python3 -c "import json; [print(a) for a in json.load(open('$HOME/.cache/srr-work/state.json'))]"`
lists every attempt, and `out/prev/*_clay.jpg` / `_shaded.jpg` are the previews of the TRELLIS.2
calls.

## 2. What the output looks like (and `clean.py`)

* **Backdrop plates**: flat double-sided sheets through the figure (about a sixth of the
  triangles, garbage texture). The Gaussian preview in the `.mp4` doesn't show them.
  `clean.drop_plates` removes them, `clean.drop_tiny` the floating flakes.
* **A flat result**: sometimes the whole result is a billboard. After cleaning, accept a
  mesh only if its thinnest extent is at least a tenth of its largest; else change seed.
* **Orientation varies** from mesh to mesh: give `yaw` in the rig config.

## 3. Rig (`rig.py`)

`clips.json` is the puppet's skeleton and every keyframe of its 25 clips (the very poses of
the stick figure of the arena):

```sh
cargo run --no-default-features --example models -- --clips > clips.json
```

Per model:

```sh
# 1. draw the fit grid, and read the joints off redhead_fit_front.png / _side.png
uv run tools/trellis/rig.py redhead_c3r2.glb redhead.glb --clips clips.json --fit redhead_fit

# 2. write redhead.rig.json with the joints (metres of the standing model: 1.8 m tall, soles on
#    the floor, centred between the feet), and a "yaw" in degrees if the model doesn't face +Z
cat redhead.rig.json
# { "joints": { "hips": [-0.05, 0.95, 0.03], "neck": [...], "head_top": [...],
#               "arm_r": [...], "arm_l": [...], "hand_r": [...], "hand_l": [...],
#               "leg_r": [...], "leg_l": [...] } }

# 3. rig it, then look at a few clips in Blender
uv run tools/trellis/rig.py redhead_c3r2.glb redhead.glb --clips clips.json --config redhead.rig.json
blender -b --python tools/trellis/blender_clips.py -- redhead.glb redhead_clip idle@0 punch@0.5 kick@0.5 ko@1 mount@0.5
```

The config also takes `centre` (`[x, z]` of the soles, to override the centring), `reach` and
`fade` (how far a bone's skin reaches, and how it fades; see `rig.py`). The proportion heuristic
that guesses the joints alone is not reliable (hair, skirts and spread arms fool it), hence
the config. The skin is the distance to each bone, then diffused over the
welded mesh graph so that the shoulders bend rather than tear. Each bone is retargeted
by a shortest-arc rotation from the mesh's own pose (T-pose, A-pose, arms spread, whatever it
was modelled in) to the direction the same bone of the puppet takes, keyframe by keyframe.
The output's `asset.generator` is `srr rig (TRELLIS)`.

`tools/trellis/look.sh <name> <name>.rig.json` does steps 3 and the Blender check in one go for
`$SRR_WORK/out/<name>_<cell>.glb` (whichever cell the farm accepted) and writes the contact
sheet `$SRR_WORK/preview/look_<name>.png`.

Then copy the result to `assets/models/manga/<variant>.glb` and run `cargo xtask models`.
