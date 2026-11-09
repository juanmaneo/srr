# Headless preview of a TRELLIS .glb (Blender 4.x/5.x, Workbench engine, no GPU needed).
#   blender -b --python tools/trellis/blender_preview.py -- model.glb out_prefix
# Writes <out_prefix>_{tex,clay}_{front,right,back,left}.png (orthographic, 90 degree steps).
import math
import sys

import bpy
from mathutils import Vector

glb, prefix = sys.argv[sys.argv.index("--") + 1:][:2]

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=glb)
meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
corners = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
mn = Vector([min(c[i] for c in corners) for i in range(3)])
mx = Vector([max(c[i] for c in corners) for i in range(3)])
center = (mn + mx) / 2
size = mx - mn
print("PREVIEW bbox (blender Z-up) min", tuple(mn), "max", tuple(mx), "size", tuple(size))

scene = bpy.context.scene
scene.render.engine = "BLENDER_WORKBENCH"
scene.render.resolution_x, scene.render.resolution_y = 600, 800
scene.render.film_transparent = False
scene.world = bpy.data.worlds.new("w")
scene.world.color = (0.55, 0.55, 0.58)
scene.view_settings.view_transform = "Standard"

cam_data = bpy.data.cameras.new("cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = size.z * 1.15
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam

passes = {
    "tex": dict(light="FLAT", color_type="TEXTURE"),
    "clay": dict(light="STUDIO", color_type="SINGLE"),
}
for pname, cfg in passes.items():
    sh = scene.display.shading
    sh.light = cfg["light"]
    sh.color_type = cfg["color_type"]
    sh.single_color = (0.8, 0.8, 0.8)
    for view, deg in (("front", 0), ("right", 90), ("back", 180), ("left", 270)):
        a = math.radians(deg)
        d = max(size) * 4
        cam.location = center + Vector((math.sin(a) * d, -math.cos(a) * d, 0))
        cam.rotation_euler = (math.pi / 2, 0, a)
        scene.render.filepath = f"{prefix}_{pname}_{view}.png"
        bpy.ops.render.render(write_still=True)
        print("PREVIEW wrote", scene.render.filepath)
