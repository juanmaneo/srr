#!/bin/sh
# look.sh <name> [config]: rig <name>_<cell>.glb (the output of farm.py) into <name>.glb, render a few clips
# with Blender and write one contact sheet, $SRR_WORK/preview/look_<name>.png.
# $SRR_WORK is the work directory of farm.py (default ~/.cache/srr-work).
W=${SRR_WORK:-$HOME/.cache/srr-work}
REPO=$(cd "$(dirname "$0")/../.." && pwd)
cd "$REPO" || exit 1
name=$1
cfg=""
[ -n "$2" ] && cfg="--config $2"
mkdir -p "$W/preview"
src=$(ls "$W"/out/"${name}"_c[0-9]r[0-9].glb 2>/dev/null | head -n 1)
[ -n "$src" ] || { echo "no $W/out/${name}_c<col>r<row>.glb (the output of farm.py)" >&2; exit 1; }
python3 tools/trellis/rig.py "$src" "$W/out/$name.glb" --clips "$W/clips.json" $cfg || exit 1
blender -b --python tools/trellis/blender_clips.py -- "$W/out/$name.glb" "$W/out/${name}_clip" idle@0 punch@0.5 kick@0.5 ko@1 walk_f@0.25 mount@0.5 submission@0.5 jump@0.5 || exit 1
W="$W" NAME="$name" python3 - <<'PY'
import os
from PIL import Image
W, name = os.environ["W"], os.environ["NAME"]
names = ['idle', 'punch', 'kick', 'ko', 'walk_f', 'mount', 'submission', 'jump']
tiles = [Image.open(f'{W}/out/{name}_clip_{n}_side.png').convert('RGB') for n in names]
tiles += [Image.open(f'{W}/out/{name}_clip_{n}_front.png').convert('RGB') for n in ('idle', 'punch', 'mount', 'submission')]
sheet = Image.new('RGB', (400 * 4, 500 * 3))
for i, tile in enumerate(tiles):
    sheet.paste(tile, ((i % 4) * 400, (i // 4) * 500))
sheet.resize((1400, 1312)).save(f'{W}/preview/look_{name}.png')
print(f'{W}/preview/look_{name}.png')
PY
