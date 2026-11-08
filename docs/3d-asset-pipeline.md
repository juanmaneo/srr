# 3D asset pipeline: from manga skins to cel-shaded fighters

The third-person view (`Tab`) can show each fighter as a rigged, cel-shaded 3D model instead
of the software-drawn wooden mannequin. Nobody models anything by hand: every model comes
out of an AI image-to-3D service (Meshy or Tripo), starting from the manga art that is
already in the repo. The game draws it with a toon shader and an ink outline, so the result
reads like the manga skins.

All of this is optional. You only get it in a build with the `mode3d` Cargo feature
(`cargo run --features mode3d`). If a character has no model, or its model is broken, that
character keeps its mannequin. The rules, the netcode and the replays never see the models.

## What the game expects

```
assets/models/
  <character>/                  lower-case name: chalk, torch, brick, zip
    <character>.glb             the one canonical mesh: glTF 2.0 binary, rigged, with clips
    skins/
      <variant>.png             optional texture variants, named like assets/skins/manga/
```

* **One mesh per character.** A skin is only a texture. The ten variants of
  `assets/skins/manga/` (`brunette`, `blonde`, `redhead`, `pink`, `white`, `black`, `green`,
  `violet`, `cyan`, `raven`) map to `skins/brunette.png` and so on. The texture replaces the
  base colour of every material of the model, so it must use the model's UV layout. If a
  skin is missing, the model wears its own texture.
* **Humanoid skeleton**, in T-pose or A-pose at rest. Bone names don't matter: each clip
  animates the skeleton it was exported with.
* **Any scale and units.** The game measures the rest pose and scales the model to
  `MODEL_HEIGHT`, with its feet on the floor. The model must face **+Z** (the glTF
  convention) and stand on Y-up.
* **Named animation clips**, inside the same `.glb`. The first thirteen are required:

  | Clip        | Played when                                        | Playback                     |
  |-------------|----------------------------------------------------|------------------------------|
  | `idle`      | standing, or walking without moving                | loop                         |
  | `walk_f`    | walking forward                                    | loop                         |
  | `walk_b`    | backing off (blocking while moving)                | loop                         |
  | `crouch`    | crouching                                          | loop                         |
  | `jump`      | airborne                                           | once                         |
  | `block`     | blocking in place                                  | loop                         |
  | `hit_light` | hit stun                                           | once                         |
  | `hit_heavy` | hit stun in the air or from the 2nd combo hit      | once                         |
  | `ko`        | knocked out                                        | once                         |
  | `punch`     | punches                                            | scrubbed by the frame data   |
  | `kick`      | kicks                                              | scrubbed by the frame data   |
  | `sweep`     | the leg that reaps in the sweep technique          | scrubbed by the frame data   |
  | `special`   | the character's special move                       | scrubbed by the frame data   |

  If one of them is missing, the game logs an error naming the model and the clip, and the
  fighter plays `idle` instead.

  The next twelve are optional: the movements the thirteen merge. A model without one plays
  the *fallback* below, exactly what every model played before they existed, and the audit
  only notes the gap.

  | Clip         | Played when                                       | Playback                     | Fallback |
  |--------------|---------------------------------------------------|------------------------------|----------|
  | `grab`       | the grab, the throw's grip, the clinch (both)     | scrubbed / held pose         | `punch`  |
  | `jump_kick`  | a kick thrown in the air                          | scrubbed by the frame data   | `kick`   |
  | `throw`      | top of the shoulder throw's projection            | loop                         | `sweep`  |
  | `scissor`    | top of the flying scissor sweep                   | loop                         | `sweep`  |
  | `mount`      | top: kneeling astride the opponent                | loop                         | `crouch` |
  | `submission` | top: an arm locked around the neck                | loop                         | `crouch` |
  | `scarf_hold` | top: sitting at the opponent's side               | loop                         | `crouch` |
  | `armbar`     | top: on the back across the opponent              | loop                         | `crouch` |
  | `swept`      | bottom of the sweep, falling backwards            | once                         | `ko`     |
  | `thrown`     | bottom of the throw, flying over the shoulder     | once                         | `ko`     |
  | `pinned`     | bottom of the mount                               | once                         | `ko`     |
  | `caught`     | bottom of a submission, scarf hold or armbar      | once                         | `ko`     |

  These thirty movements are the ones the stick figure of the arena shows, and the
  [chalk sprite sheet](../assets/skins/chalk/chalk.png) has a cell for each; the manga sheets
  paint twenty-two of them. `src/front/moves.rs` names them once, and `Move::clip`,
  `Move::pose` say which clip and which manga cell stand for each.
* **In-place clips, no root motion.** The game moves the fighter, not the animation. Root
  motion would drift away from the hitboxes, so export or convert every clip "in
  place". A clip that still moves its hips only moves the drawing, never the fighter.
* **Attack clips are scrubbed, not played.** The game picks the time in `punch`, `kick`,
  `sweep`, `special`, `grab` and `jump_kick` from the move's frame data. The first 40 % of the clip covers
  start-up, 40–60 % covers the active frames, and the rest covers recovery. So author (or
  pick) clips whose contact pose falls around the middle of the clip. The tuning guide's
  "3D view" section lists the knobs.

## The workflow

### 1. Make the model (Meshy or Tripo)

1. Take the character's art from `assets/skins/manga/<variant>.png`. One sheet per variant;
   use the brunette (the default) as the canonical look. Crop one full-body frame: a front
   view, arms slightly away from the body if you can, plain background.
2. **Meshy** → *Image to 3D*. Upload the crop, choose the anime/cartoon art style, enable
   texturing, and keep the polycount modest (under 30k triangles; the toon shader doesn't
   need detail). **Tripo** works the same way (*Image to Model*, stylised).
3. **Auto-rig** the result as a humanoid: Meshy's *Rigging*, Tripo's *Auto Rig*, or Mixamo's
   auto-rigger if the service doesn't rig. Check that the wrists and ankles are placed right.
4. **Export as GLB** with the skeleton (Meshy: *Download* → `glb`, "with skeleton").

### 2. Add the clips

1. Take the clips from Meshy's animation presets, or from Mixamo's fighting pack (Mixamo
   gives FBX; tick **In Place** on every walk and attack so there is no root motion).
2. Convert FBX to GLB if needed. Blender: *File → Import → FBX*, then
   *File → Export → glTF 2.0 (.glb)*, with *Animation* and *Skinning* checked and
   *Group by NLA track* on.
3. Put every clip in the **one** `.glb` of the character and give each the exact name from the
   table above. In Blender, that means renaming the actions (or NLA tracks) before export.

### 3. Drop it in and check it

```sh
mkdir -p assets/models/torch
cp ~/Downloads/torch.glb assets/models/torch/torch.glb
cargo xtask models            # audits every model: clips found, missing clips, skins
cargo run --features mode3d   # Tab to the third-person view
```

`cargo xtask models` runs `examples/models.rs` without any engine. It reads each GLB's JSON
chunk and lists the clips it carries, the tool that generated it, and the skins present.
It exits non-zero on a broken file, a missing clip, or a stray skin file that matches no
variant. A missing model or skin is only reported, never an error.

### 4. Skins (texture variants)

Re-texture the canonical mesh once per variant. You can use Meshy's *Retexture* with the
variant's manga sheet as the reference image, or repaint the exported texture. Then save
each result as `assets/models/<character>/skins/<variant>.png`. Keep the UV layout of the
canonical mesh: re-texture it, don't regenerate it. A regenerated mesh has a new UV layout,
and every skin painted for the old one would be scrambled.

## The puppet: the placeholder and the CI fixture

`assets/models/chalk/` holds a **wooden puppet**, Pinocchio style: balls for the joints,
tubes for the limbs and the barrel torso, a long nose, painted eyes, a cap of hair, big
shoes, about 14,000 smooth vertices on an eleven-joint skeleton (hips, spine, head, two
shoulders, elbows, hips and knees). It carries all 25 clips and a 288×192 atlas of grain:
lacquered hair, rings of wood, the weave of cloth, leather, eyes; each of the ten skins is
the same atlas in the manga palette of a variant (so a skin keeps the UV layout, as the
rules above ask). It is generated in code (`src/front/puppet.rs`) and committed, so:

* CI boots the 3D view with a real rigged, animated GLB and checks it (`tests/render.rs`
  with `--features mode3d`) without downloading anything or spending a service's credits;
* you have something to look at, and to tune the shader against, before any real model
  exists;
* a test (`tests/models.rs`) checks that the committed files are still byte-identical to the
  generator.

**It moves like the stick figure.** The puppet has the stick figure's proportions (sixty-four
pixels to the metre) and every keyframe of every clip is a pose of the stick figure
(`stick::clip_poses`), solved bone by bone onto the skeleton (`puppet::solve`): each bone
points where the same bone of the stick figure points. A test walks the skeleton forwards
and compares. So the chalk does the same thing in 2D, as a sprite cell and in 3D, and a
movement added to the stick figure shows up in all three. Only the cartoon stretch of the
limbs is lost, the bones keeping their length.

To regenerate the placeholders (after changing the generator):

```sh
cargo run --no-default-features --example models -- --write chalk   # one character
cargo run --no-default-features --example models -- --write         # every one without a model
cargo run --no-default-features --example sheets -- --write         # the chalk sprite sheet
```

`--write` never overwrites a real model. Any GLB that the puppet (or the cube man it
replaced) didn't generate (its `asset.generator` says so) is left alone. To replace the
placeholder with a real Chalk, overwrite `chalk.glb` and its skins.

## The chalk sprite sheet

`assets/skins/chalk/chalk.png` draws the stick figure in each of the thirty movements, in
the grid of the manga sheets (five columns, right-facing over its mirror), 400×320 cells,
generated from the very poses the arena inks (`src/front/chalk.rs`, `examples/sheets.rs`).
It is the source art an image-to-3D service can turn into a model of the chalk, the way the
manga sheets are the source art of theirs.

## Determinism: why the models can't break a replay

The front end describes the stage: for each fighter, which character, which skin, which clip,
and where in that clip (`src/front/model.rs`). The engine draws that description. Nothing
flows back. The clip follows the fighter's state and its move's frame data, never the other
way round. The fighter's position comes from the rules, and so do the hitboxes. The same replay
plays the same way with every model, with the puppet, or with no model at all. The
intentional glitches documented in the tuning guide are rules, so they behave exactly as
before.
