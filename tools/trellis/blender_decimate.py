# Decimate a textured mesh to a triangle budget, keeping its UVs and its base colour texture.
#   blender -b --python tools/trellis/blender_decimate.py -- in.glb out.glb 14000
# TRELLIS.2 returns 100k-500k triangles with WebP textures; the game wants ~10-20k and PNG.
# Smooth normals are rebuilt: a decimated mesh keeps no usable custom normals.
import sys

import bpy

src, dst, target = sys.argv[sys.argv.index("--") + 1:][:3]
target = int(target)

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
assert len(meshes) == 1, f"expected one mesh object, found {len(meshes)}"
obj = meshes[0]
bpy.context.view_layer.objects.active = obj
obj.select_set(True)

before = len(obj.data.polygons)
if before > target:
    modifier = obj.modifiers.new("decimate", "DECIMATE")
    modifier.decimate_type = "COLLAPSE"
    modifier.ratio = target / before
    modifier.use_collapse_triangulate = True
    bpy.ops.object.modifier_apply(modifier=modifier.name)
bpy.ops.mesh.customdata_custom_splitnormals_clear()
bpy.ops.object.shade_smooth()
print(f"DECIMATE {before} -> {len(obj.data.polygons)} faces")

bpy.ops.export_scene.gltf(filepath=dst, export_format="GLB", export_image_format="AUTO", export_apply=True,
                          export_normals=True, export_texcoords=True, export_cameras=False, export_lights=False)
