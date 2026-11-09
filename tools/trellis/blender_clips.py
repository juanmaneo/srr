# Headless render of a rigged model playing some of its clips (Blender 4.x/5.x, Workbench).
#   blender -b --python tools/trellis/blender_clips.py -- model.glb out_prefix [clip@fraction ...]
# Imports the glTF with Blender's own importer (an independent check of the skin and of the
# animations) and writes <out_prefix>_<clip>_<view>.png, orthographic, `side` and `front`.
import math
import sys

import bpy
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1:]
glb, prefix = args[:2]
wanted = args[2:] or ["idle@0", "punch@0.5", "kick@0.5", "ko@1", "walk_f@0.25", "mount@0.5"]

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=glb)
scene = bpy.context.scene
armature = next(o for o in scene.objects if o.type == "ARMATURE")
print("CLIPS", sorted(a.name for a in bpy.data.actions))

scene.render.engine = "BLENDER_WORKBENCH"
scene.render.resolution_x, scene.render.resolution_y = 400, 500
scene.world = bpy.data.worlds.new("w")
scene.world.color = (0.5, 0.5, 0.55)
scene.view_settings.view_transform = "Standard"
shading = scene.display.shading
shading.light = "FLAT"
shading.color_type = "TEXTURE"

camera_data = bpy.data.cameras.new("cam")
camera_data.type = "ORTHO"
camera_data.ortho_scale = 2.4
camera = bpy.data.objects.new("cam", camera_data)
scene.collection.objects.link(camera)
scene.camera = camera

for item in wanted:
    name, _, fraction = item.partition("@")
    action = bpy.data.actions.get(name)
    if action is None:
        print("MISSING clip", name)
        continue
    armature.animation_data_create()
    armature.animation_data.action = action
    start, end = action.frame_range
    scene.frame_set(int(round(start + (end - start) * float(fraction or 0))))
    bpy.context.view_layer.update()
    for view, angle in (("side", 90), ("front", 0)):
        a = math.radians(angle)
        distance = 8
        camera.location = Vector((math.sin(a) * distance, -math.cos(a) * distance, 0.9))
        camera.rotation_euler = (math.pi / 2, 0, a)
        scene.render.filepath = f"{prefix}_{name}_{view}.png"
        bpy.ops.render.render(write_still=True)
        print("WROTE", scene.render.filepath)
