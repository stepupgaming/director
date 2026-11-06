"""Build the 3D engine spike scene in Blender and export it as GLB.

Run: blender -b --factory-startup --python spikes/engine-3d/scene/make_scene.py -- <out.glb>

The scene holds extruded bevelled type, PBR spheres (chrome, plastic,
clearcoat, glass), an emissive ring, a monkey head, a ground plane, a sun
and a spot light, and a camera that orbits the type over four seconds.
"""

import math
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = 30
scene.frame_start = 1
scene.frame_end = 120


def material(name, color, metallic=0.0, roughness=0.5, **extra):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    for key, value in extra.items():
        bsdf.inputs[key].default_value = value
    return m


def assign(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


def smooth(obj):
    for poly in obj.data.polygons:
        poly.use_smooth = True


# Ground.
bpy.ops.mesh.primitive_plane_add(size=40, location=(0, 0, 0))
ground = bpy.context.object
ground.name = "Ground"
assign(ground, material("Ground", (0.18, 0.18, 0.2), roughness=0.85))

# Extruded, bevelled type, converted to a mesh so every engine reads the same triangles.
bpy.ops.object.text_add(location=(0, 0, 0.02))
text = bpy.context.object
text.name = "Title"
text.data.body = "DIRECTOR"
text.data.align_x = "CENTER"
text.data.size = 1.4
text.data.extrude = 0.18
text.data.bevel_depth = 0.035
text.data.bevel_resolution = 4
text.rotation_euler = (math.radians(90), 0, 0)
text.location = (0, 0, 0.28)
bpy.ops.object.convert(target="MESH")
assign(text, material("Gold", (1.0, 0.77, 0.34), metallic=1.0, roughness=0.22))

# Spheres along the front.
spheres = [
    ("Chrome", (-3.0, -2.2), material("Chrome", (0.95, 0.95, 0.95), metallic=1.0, roughness=0.04)),
    ("Plastic", (-1.0, -2.6), material("Plastic", (0.8, 0.08, 0.06), roughness=0.38)),
    ("Clearcoat", (1.0, -2.6), material("Clearcoat", (0.05, 0.22, 0.75), metallic=0.6, roughness=0.45,
                                         **{"Coat Weight": 1.0, "Coat Roughness": 0.03})),
    ("Glass", (3.0, -2.2), material("Glass", (1.0, 1.0, 1.0), roughness=0.02,
                                     **{"Transmission Weight": 1.0, "IOR": 1.45})),
]
for name, (x, y), mat in spheres:
    bpy.ops.mesh.primitive_uv_sphere_add(segments=64, ring_count=32, radius=0.55, location=(x, y, 0.55))
    obj = bpy.context.object
    obj.name = name
    smooth(obj)
    assign(obj, mat)

# Emissive ring behind the type, for bloom.
bpy.ops.mesh.primitive_torus_add(major_radius=2.6, minor_radius=0.05, location=(0, 1.6, 1.4),
                                 rotation=(math.radians(90), 0, 0), major_segments=128, minor_segments=16)
ring = bpy.context.object
ring.name = "Ring"
smooth(ring)
assign(ring, material("Neon", (0.02, 0.02, 0.02), roughness=0.4,
                      **{"Emission Color": (0.2, 0.75, 1.0, 1.0), "Emission Strength": 12.0}))

# Monkey head.
bpy.ops.mesh.primitive_monkey_add(size=1.0, location=(4.6, 0.6, 0.75), rotation=(0, 0, math.radians(-30)))
monkey = bpy.context.object
monkey.name = "Monkey"
modifier = monkey.modifiers.new("Subsurf", "SUBSURF")
modifier.levels = 2
bpy.ops.object.modifier_apply(modifier="Subsurf")
smooth(monkey)
assign(monkey, material("Copper", (0.95, 0.64, 0.54), metallic=1.0, roughness=0.35))

# Lights.
bpy.ops.object.light_add(type="SUN", location=(0, 0, 10), rotation=(math.radians(50), math.radians(10), math.radians(35)))
sun = bpy.context.object
sun.name = "Sun"
sun.data.energy = 3.0
sun.data.angle = math.radians(2.0)

bpy.ops.object.light_add(type="SPOT", location=(-4, -4, 5))
spot = bpy.context.object
spot.name = "Spot"
spot.data.energy = 900.0
spot.data.color = (1.0, 0.55, 0.3)
spot.data.spot_size = math.radians(45)
spot.data.spot_blend = 0.3
direction = bpy.data.objects["Title"].location - spot.location
spot.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()

# Orbit camera: a parent empty spins once around the type over four seconds.
bpy.ops.object.empty_add(location=(0, 0, 0.8))
rig = bpy.context.object
rig.name = "Orbit"
bpy.ops.object.camera_add(location=(0, -9.5, 2.6))
camera = bpy.context.object
camera.name = "Camera"
camera.data.lens = 35
camera.parent = rig
look = (rig.location - camera.location)
camera.rotation_euler = look.to_track_quat("-Z", "Y").to_euler()
scene.camera = camera
rig.rotation_euler = (0, 0, math.radians(-40))
rig.keyframe_insert("rotation_euler", frame=1)
rig.rotation_euler = (0, 0, math.radians(40))
rig.keyframe_insert("rotation_euler", frame=121)
for curve in rig.animation_data.action.fcurves if hasattr(rig.animation_data.action, "fcurves") else []:
    for key in curve.keyframe_points:
        key.interpolation = "LINEAR"

bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_cameras=True,
    export_lights=True,
    export_animations=True,
    export_apply=True,
)
print("wrote", out)
