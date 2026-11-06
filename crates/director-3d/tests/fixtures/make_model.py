"""Write the director-3d test model as GLB.

Run: blender -b --factory-startup --python crates/director-3d/tests/fixtures/make_model.py -- crates/director-3d/tests/fixtures/model.glb

The scene holds a red plastic cube that turns a quarter over two seconds, a
gold sphere that rises beside it, a grey ground, a sun and a camera. The two
animated objects export as two clips, which place frame f at f / 30 seconds.

With `-- --eevee crates/director-3d/tests/fixtures` the script exports
nothing. It writes `sky.hdr`, an equirectangular sky that depends only on
elevation, lights the same scene with it in Eevee and renders frames 1 and 46
at 160x90 over a transparent film with the Standard view transform into
`eevee-1.png` and `eevee-46.png`.
"""

import math
import os
import sys

import bpy

args = sys.argv[sys.argv.index("--") + 1 :]
eevee = args[1] if args[0] == "--eevee" else None
out = args[0]

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


def sky(path, width=128, height=64):
    """A grey ground, a brighter zenith and the same radiance at every azimuth,
    so both renderers' equirectangular conventions read it alike."""
    image = bpy.data.images.new("Sky", width, height, float_buffer=True, alpha=False)
    pixels = []
    for row in range(height):  # Blender stores the bottom row first.
        s = math.sin((row + 0.5) / height * math.pi - math.pi / 2)
        v = 0.6 + 0.4 * s if s >= 0 else 0.6 - 0.35 * math.sqrt(-s)
        pixels += [v, v, v, 1.0] * width
    image.pixels = pixels
    image.file_format = "HDR"
    image.filepath_raw = path
    image.save()
    return image


if eevee:
    world = bpy.data.worlds.new("Sky")
    scene.world = world
    if world.node_tree is None:
        world.use_nodes = True
    nodes = world.node_tree.nodes
    environment = nodes.new("ShaderNodeTexEnvironment")
    environment.image = sky(os.path.abspath(os.path.join(eevee, "sky.hdr")))
    background = next(n for n in nodes if n.type == "BACKGROUND")
    background.inputs["Strength"].default_value = 1.0
    world.node_tree.links.new(environment.outputs["Color"], background.inputs["Color"])
    try:
        scene.render.engine = "BLENDER_EEVEE"
    except TypeError:
        scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.eevee.taa_render_samples = 64
    scene.render.resolution_x = 160
    scene.render.resolution_y = 90
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True
    scene.display_settings.display_device = "sRGB"
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.0
    scene.view_settings.gamma = 1.0
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    for frame in (1, 46):
        scene.frame_set(frame)
        scene.render.filepath = os.path.abspath(os.path.join(eevee, f"eevee-{frame}.png"))
        bpy.ops.render.render(write_still=True)
        print("rendered", scene.render.filepath)
    sys.exit(0)

bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_cameras=True,
    export_lights=True,
    export_animations=True,
    export_apply=True,
)
print("wrote", out)
