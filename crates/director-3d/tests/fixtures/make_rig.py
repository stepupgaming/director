"""Write the director-3d skinning and morph test model as GLB.

Run: blender -b --factory-startup --python crates/director-3d/tests/fixtures/make_rig.py -- crates/director-3d/tests/fixtures/rig.glb

The scene holds a blue column skinned to a two-bone armature whose upper bone
bends 80 degrees over two seconds on the left, a white sphere whose "Squash"
shape key goes from 0 to 1 over the same two seconds on the right, a grey
ground, a sun and a camera.
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


def material(name, color, roughness=0.5):
    m = bpy.data.materials.new(name)
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    return m


def assign(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


def linear(action):
    for curve in action.fcurves if hasattr(action, "fcurves") else []:
        for key in curve.keyframe_points:
            key.interpolation = "LINEAR"


bpy.ops.mesh.primitive_plane_add(size=8, location=(0, 0, 0))
assign(bpy.context.object, material("Ground", (0.3, 0.3, 0.32), roughness=0.8))

# A column 2 m tall with enough rings to bend smoothly.
bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=0.25, depth=2.0, location=(-1.0, 0, 1.0))
column = bpy.context.object
column.name = "Column"
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.subdivide(number_cuts=15)
bpy.ops.object.mode_set(mode="OBJECT")
for poly in column.data.polygons:
    poly.use_smooth = True
assign(column, material("Blue", (0.1, 0.3, 0.9), roughness=0.4))

bpy.ops.object.armature_add(location=(-1.0, 0, 0))
rig = bpy.context.object
rig.name = "Rig"
bpy.ops.object.mode_set(mode="EDIT")
lower = rig.data.edit_bones[0]
lower.name = "Lower"
lower.head = (0, 0, 0)
lower.tail = (0, 0, 1.0)
upper = rig.data.edit_bones.new("Upper")
upper.head = (0, 0, 1.0)
upper.tail = (0, 0, 2.0)
upper.parent = lower
upper.use_connect = True
bpy.ops.object.mode_set(mode="OBJECT")

column.select_set(True)
rig.select_set(True)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.parent_set(type="ARMATURE_AUTO")

bpy.ops.object.mode_set(mode="POSE")
bone = rig.pose.bones["Upper"]
bone.rotation_mode = "XYZ"
bone.rotation_euler = (0, 0, 0)
bone.keyframe_insert("rotation_euler", frame=1)
bone.rotation_euler = (0, 0, math.radians(-80))
bone.keyframe_insert("rotation_euler", frame=61)
bpy.ops.object.mode_set(mode="OBJECT")
linear(rig.animation_data.action)

bpy.ops.mesh.primitive_uv_sphere_add(segments=32, ring_count=16, radius=0.6, location=(1.0, 0, 0.6))
ball = bpy.context.object
ball.name = "Ball"
for poly in ball.data.polygons:
    poly.use_smooth = True
assign(ball, material("White", (0.85, 0.85, 0.85), roughness=0.3))
ball.shape_key_add(name="Basis")
squash = ball.shape_key_add(name="Squash")
for point in squash.data:
    # Flatten to 40 % height with the bottom kept on the ground.
    point.co.z = point.co.z * 0.4 - 0.36
    point.co.x *= 1.35
    point.co.y *= 1.35
squash.value = 0.0
squash.keyframe_insert("value", frame=1)
squash.value = 1.0
squash.keyframe_insert("value", frame=61)
linear(ball.data.shape_keys.animation_data.action)

bpy.ops.object.light_add(type="SUN", location=(0, 0, 6), rotation=(math.radians(40), math.radians(15), math.radians(30)))
bpy.context.object.data.energy = 3.0

bpy.ops.object.camera_add(location=(0, -6, 2.0))
camera = bpy.context.object
camera.data.lens = 35
target = bpy.data.objects["Ball"].location.copy()
target.x = 0
target.z = 1.0
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
scene.camera = camera

bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_cameras=True,
    export_lights=True,
    export_animations=True,
    export_skins=True,
    export_morph=True,
)
print("wrote", out)
