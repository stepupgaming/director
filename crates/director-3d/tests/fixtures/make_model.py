"""Write the director-3d test model as GLB.

Run: blender -b --factory-startup --python crates/director-3d/tests/fixtures/make_model.py -- crates/director-3d/tests/fixtures/model.glb

The scene holds a red plastic cube that turns a quarter over two seconds, a
gold sphere that rises beside it, a grey ground, a sun and a camera. The two
animated objects export as two clips.
"""

import math
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = 30
scene.frame_start = 1
scene.frame_end = 61


def material(name, color, metallic=0.0, roughness=0.5):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    return m


def assign(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


def linear(obj):
    action = obj.animation_data.action
    for curve in action.fcurves if hasattr(action, "fcurves") else []:
        for key in curve.keyframe_points:
            key.interpolation = "LINEAR"


bpy.ops.mesh.primitive_plane_add(size=8, location=(0, 0, 0))
assign(bpy.context.object, material("Ground", (0.3, 0.3, 0.32), roughness=0.8))

bpy.ops.mesh.primitive_cube_add(size=1.2, location=(-0.9, 0, 0.6))
cube = bpy.context.object
cube.name = "Cube"
assign(cube, material("Plastic", (0.8, 0.1, 0.06), roughness=0.35))
cube.rotation_euler = (0, 0, 0)
cube.keyframe_insert("rotation_euler", frame=1)
cube.rotation_euler = (0, 0, math.radians(90))
cube.keyframe_insert("rotation_euler", frame=61)
linear(cube)

bpy.ops.mesh.primitive_uv_sphere_add(segments=32, ring_count=16, radius=0.5, location=(0.9, 0, 0.5))
sphere = bpy.context.object
sphere.name = "Sphere"
for poly in sphere.data.polygons:
    poly.use_smooth = True
assign(sphere, material("Gold", (1.0, 0.77, 0.34), metallic=1.0, roughness=0.25))
sphere.keyframe_insert("location", frame=1)
sphere.location = (0.9, 0, 1.4)
sphere.keyframe_insert("location", frame=61)
linear(sphere)

bpy.ops.object.light_add(type="SUN", location=(0, 0, 6), rotation=(math.radians(40), math.radians(15), math.radians(30)))
bpy.context.object.data.energy = 3.0

bpy.ops.object.camera_add(location=(0, -6, 2.4))
camera = bpy.context.object
camera.data.lens = 40
target = bpy.data.objects["Cube"].location.copy()
target.x = 0
target.z = 0.7
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
scene.camera = camera

bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_cameras=True,
    export_lights=True,
    export_animations=True,
    export_apply=True,
)
print("wrote", out)
