"""Write the director-3d matchmove fixture.

Run: blender -b --factory-startup --python crates/director-3d/tests/fixtures/make_track.py -- crates/director-3d/tests/fixtures

A blue box stands on a checkered ground under a sun and `sky.hdr`, and a
camera dollies past it over 61 frames at 30 fps while it keeps the box in
frame. The script writes three things into the given folder:

- `track.glb`: the ground (`Ground`, a plain stand-in), the box, the sun and
  the camera with its keyed pose, the solved camera a matchmove hands over.
- `track.mp4`: the plate, Eevee's 160x90 renders of frames 0 to 61 with the
  box hidden, standing in for footage shot without it. The GLB places frame
  f at f / 30 seconds and the plate shows frame k from k / 30 seconds, so the
  two line up on one clock. Frame 0 holds frame 1's pose.
- `track-1.png` and `track-46.png`: Eevee's renders of frames 1 and 46 with
  the box, the ground truth a composite of the plate and the GLB aims at.

Every render uses the Standard view transform. `ffmpeg` encodes the plate.
"""

import math
import os
import shutil
import subprocess
import sys
import tempfile

import bpy
from mathutils import Vector

out = os.path.abspath(sys.argv[sys.argv.index("--") + 1])

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = 30
scene.frame_start = 1
scene.frame_end = 61


def material(name, color, roughness=0.5):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    return m


def checker(name):
    """Two greys in half-metre squares, so the plate shows the camera move."""
    m = material(name, (0.3, 0.3, 0.3), roughness=0.8)
    nodes, links = m.node_tree.nodes, m.node_tree.links
    coords = nodes.new("ShaderNodeTexCoord")
    tex = nodes.new("ShaderNodeTexChecker")
    tex.inputs["Scale"].default_value = 1.0
    tex.inputs["Color1"].default_value = (0.42, 0.42, 0.4, 1.0)
    tex.inputs["Color2"].default_value = (0.22, 0.22, 0.24, 1.0)
    mapping = nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (2.0, 2.0, 2.0)
    links.new(coords.outputs["Object"], mapping.inputs["Vector"])
    links.new(mapping.outputs["Vector"], tex.inputs["Vector"])
    links.new(tex.outputs["Color"], nodes["Principled BSDF"].inputs["Base Color"])
    return m


def assign(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


bpy.ops.mesh.primitive_plane_add(size=16, location=(0, 0, 0))
ground = bpy.context.object
ground.name = "Ground"
assign(ground, checker("Checker"))

bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0.2, 0.3, 0.5))
box = bpy.context.object
box.name = "Box"
assign(box, material("Blue", (0.08, 0.25, 0.7), roughness=0.4))

bpy.ops.object.light_add(type="SUN", location=(0, 0, 6), rotation=(math.radians(-55), 0, math.radians(35)))
bpy.context.object.data.energy = 3.0

bpy.ops.object.camera_add()
camera = bpy.context.object
camera.data.lens = 35
scene.camera = camera
target = Vector((0.2, 0.3, 0.4))
for frame in range(1, 62):
    t = (frame - 1) / 60
    camera.location = Vector((-2.6 + 4.6 * t, -6.0 + 0.8 * t, 2.2 + 0.4 * t))
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.keyframe_insert("location", frame=frame)
    camera.keyframe_insert("rotation_euler", frame=frame)

world = bpy.data.worlds.new("Sky")
scene.world = world
if world.node_tree is None:
    world.use_nodes = True
nodes = world.node_tree.nodes
environment = nodes.new("ShaderNodeTexEnvironment")
environment.image = bpy.data.images.load(os.path.join(out, "sky.hdr"))
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
scene.render.film_transparent = False
scene.display_settings.display_device = "sRGB"
scene.view_settings.view_transform = "Standard"
scene.view_settings.look = "None"
scene.view_settings.exposure = 0.0
scene.view_settings.gamma = 1.0
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGB"


def render(path):
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


for frame in (1, 46):
    scene.frame_set(frame)
    render(os.path.join(out, f"track-{frame}.png"))

box.hide_render = True
frames = tempfile.mkdtemp(prefix="director-plate-")
for frame in range(0, 62):
    scene.frame_set(frame)
    render(os.path.join(frames, f"plate-{frame:04d}.png"))
subprocess.run(
    [
        "ffmpeg", "-y", "-loglevel", "error", "-framerate", "30", "-start_number", "0",
        "-i", os.path.join(frames, "plate-%04d.png"),
        "-c:v", "libx264", "-preset", "veryslow", "-crf", "6", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart", os.path.join(out, "track.mp4"),
    ],
    check=True,
)
shutil.rmtree(frames)
box.hide_render = False

# The plain stand-in exports in place of the procedural checker.
assign(ground, material("Ground", (0.3, 0.3, 0.3), roughness=0.8))
bpy.ops.export_scene.gltf(
    filepath=os.path.join(out, "track.glb"),
    export_format="GLB",
    export_cameras=True,
    export_lights=True,
    export_animations=True,
    export_apply=True,
)
print("wrote", out)
