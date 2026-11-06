"""Write the director-3d camera solve fixture.

Run: blender -b --factory-startup --python crates/director-3d/tests/fixtures/make_solve.py -- crates/director-3d/tests/fixtures

A yard of Voronoi cells holds five textured crates and two pillars under a sun
and `sky.hdr`, and a handheld-height camera walks sideways past them over 60
frames at 30 fps while it pans. The solve's world puts the ground at y = 0,
the origin where frame 0's optical axis meets the ground, and frame 0's view
along -Z, so this script writes everything in that frame:

- `solve.mp4`: the plate, Eevee's 480x270 renders of frames 0 to 59 with the
  blue box hidden, H.264 yuv420p. Plate frame k shows from k / 30 seconds.
- `solve.json`: the true camera, its vertical field of view in radians, the
  height of frame 0's camera above the ground in metres, and each frame's
  camera position and glTF rotation quaternion (x, y, z, w).
- `solve.glb`: the blue box, a plain `Ground` stand-in and the sun, with no
  camera, placed in the solve's world.
- `solve-10.png` and `solve-50.png`: Eevee's renders of frames 10 and 50 with
  the box, the ground truth a composite over the plate aims at.

Every render uses the Standard view transform. `ffmpeg` encodes the plate.
"""

import json
import math
import os
import shutil
import subprocess
import sys
import tempfile

import bpy
from mathutils import Matrix, Vector

out = os.path.abspath(sys.argv[sys.argv.index("--") + 1])
FRAMES = 60

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = 30
scene.frame_start = 0
scene.frame_end = FRAMES - 1


def material(name, color, roughness=0.5):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    return m


def cells(name, scale, tint):
    """Voronoi cells of random colours: sharp junctions the tracker can hold."""
    m = material(name, (0.3, 0.3, 0.3), roughness=0.8)
    nodes, links = m.node_tree.nodes, m.node_tree.links
    coords = nodes.new("ShaderNodeTexCoord")
    voronoi = nodes.new("ShaderNodeTexVoronoi")
    voronoi.inputs["Scale"].default_value = scale
    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    mix.inputs["B"].default_value = (*tint, 1.0)
    links.new(coords.outputs["Object"], voronoi.inputs["Vector"])
    links.new(voronoi.outputs["Color"], mix.inputs["A"])
    links.new(mix.outputs["Result"], nodes["Principled BSDF"].inputs["Base Color"])
    return m


def assign(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


bpy.ops.mesh.primitive_plane_add(size=40, location=(0, 0, 0))
ground = bpy.context.object
ground.name = "Ground"
assign(ground, cells("Yard", 1.5, (0.9, 0.85, 0.75)))

props = []
for i, (x, y, size, tint) in enumerate([
    (-2.6, 1.8, 0.8, (0.9, 0.5, 0.3)),
    (2.4, 2.6, 1.0, (0.4, 0.8, 0.5)),
    (-0.8, 4.4, 0.7, (0.8, 0.8, 0.3)),
    (3.6, -0.6, 0.6, (0.7, 0.4, 0.8)),
    (-3.8, -1.2, 0.9, (0.5, 0.7, 0.9)),
]):
    bpy.ops.mesh.primitive_cube_add(size=size, location=(x, y, size / 2), rotation=(0, 0, 0.4 * i))
    crate = bpy.context.object
    crate.name = f"Crate{i}"
    assign(crate, cells(f"Crate{i}", 4.0, tint))
    props.append(crate)
for i, (x, y) in enumerate([(-1.8, 7.5), (2.8, 8.5)]):
    bpy.ops.mesh.primitive_cylinder_add(radius=0.3, depth=3.0, location=(x, y, 1.5))
    pillar = bpy.context.object
    pillar.name = f"Pillar{i}"
    assign(pillar, cells(f"Pillar{i}", 3.0, (0.85, 0.8, 0.8)))
    props.append(pillar)

bpy.ops.mesh.primitive_cube_add(size=0.8, location=(0.4, 0.6, 0.4))
box = bpy.context.object
box.name = "Box"
assign(box, material("Blue", (0.08, 0.25, 0.7), roughness=0.4))

bpy.ops.object.light_add(type="SUN", location=(0, 0, 6), rotation=(math.radians(-50), 0, math.radians(30)))
sun = bpy.context.object
sun.data.energy = 3.0

bpy.ops.object.camera_add()
camera = bpy.context.object
camera.data.sensor_fit = "VERTICAL"
camera.data.lens_unit = "FOV"
camera.data.angle_y = math.radians(42)
camera.data.clip_start = 0.05
scene.camera = camera
for frame in range(FRAMES):
    t = frame / (FRAMES - 1)
    camera.location = Vector((-2.2 + 4.0 * t, -6.5 + 1.0 * t, 1.6 + 0.15 * math.sin(math.pi * t)))
    target = Vector((-0.4 + 1.4 * t, 1.0, 0.5))
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
scene.eevee.taa_render_samples = 32
scene.render.resolution_x = 480
scene.render.resolution_y = 270
scene.render.resolution_percentage = 100
scene.render.film_transparent = False
scene.display_settings.display_device = "sRGB"
scene.view_settings.view_transform = "Standard"
scene.view_settings.look = "None"
scene.view_settings.exposure = 0.0
scene.view_settings.gamma = 1.0
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGB"

# The solve's world in Blender coordinates: origin where frame 0's optical
# axis meets the ground, up along +Z, frame 0's view along -Z_w.
scene.frame_set(0)
eye = camera.matrix_world.translation.copy()
forward = -camera.matrix_world.to_3x3().col[2]
origin = eye + forward * (-eye.z / forward.z)
up = Vector((0, 0, 1))
z_w = -Vector((forward.x, forward.y, 0)).normalized()
x_w = up.cross(z_w)
basis = Matrix((x_w, up, z_w))  # rows: Blender to solve axes


def solve_point(p):
    return basis @ (p - origin)


truth = {"fps": 30, "width": 480, "height": 270, "yfov": camera.data.angle_y, "camera_height": eye.z, "frames": []}
for frame in range(FRAMES):
    scene.frame_set(frame)
    m = camera.matrix_world
    q = (basis @ m.to_3x3()).to_quaternion()
    truth["frames"].append({"position": list(solve_point(m.translation)), "rotation": [q.x, q.y, q.z, q.w]})
with open(os.path.join(out, "solve.json"), "w") as f:
    json.dump(truth, f, indent=1)


def render(path):
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


for frame in (10, 50):
    scene.frame_set(frame)
    render(os.path.join(out, f"solve-{frame}.png"))

box.hide_render = True
frames = tempfile.mkdtemp(prefix="director-solve-")
for frame in range(FRAMES):
    scene.frame_set(frame)
    render(os.path.join(frames, f"plate-{frame:04d}.png"))
subprocess.run(
    [
        "ffmpeg", "-y", "-loglevel", "error", "-framerate", "30", "-start_number", "0",
        "-i", os.path.join(frames, "plate-%04d.png"),
        "-c:v", "libx264", "-preset", "veryslow", "-crf", "14", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart", os.path.join(out, "solve.mp4"),
    ],
    check=True,
)
shutil.rmtree(frames)
box.hide_render = False

# Export the box, a plain ground and the sun in the solve's world. glTF's
# exporter maps Blender (x, y, z) to (x, z, -y), so the root takes the solve
# frame back through that swap.
for obj in props + [camera]:
    bpy.data.objects.remove(obj)
assign(ground, material("Ground", (0.3, 0.3, 0.3), roughness=0.8))
swap = Matrix(((1, 0, 0), (0, 0, -1), (0, 1, 0)))
bpy.ops.object.empty_add(location=(0, 0, 0))
root = bpy.context.object
root.name = "Solve"
root.matrix_world = (swap @ basis).to_4x4() @ Matrix.Translation(-origin)
for obj in (ground, box, sun):
    world_matrix = obj.matrix_world.copy()
    obj.parent = root
    obj.matrix_parent_inverse = Matrix.Identity(4)
    obj.matrix_world = root.matrix_world @ world_matrix
bpy.ops.export_scene.gltf(
    filepath=os.path.join(out, "solve.glb"),
    export_format="GLB",
    export_cameras=False,
    export_lights=True,
    export_animations=False,
    export_apply=True,
)
print("wrote", out)
