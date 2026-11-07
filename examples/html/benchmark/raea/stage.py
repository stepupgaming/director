"""Stage the B3 benchmark clip as a raea project: python stage.py <out-dir>.

Writes edit.tsx, fonts.gen.ts (the two Roboto faces as data URLs) and media/ with the
photos, the WAV bed and clip20.mp4 re-encoded without B-frames, which raea
0.1.0-canary.2 refuses. Render with `RAEA_EXPORT_CACHE=0 raea render edit.tsx -o out.mp4`
inside <out-dir>. Needs FFmpeg on PATH.
"""

import base64
import shutil
import subprocess
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
assets = here.parent / "assets"
out = Path(sys.argv[1])
media = out / "media"
media.mkdir(parents=True, exist_ok=True)
shutil.copy(here / "edit.tsx", out / "edit.tsx")
for name in ["photo_a.jpg", "photo_b.jpg", "bed20.wav"]:
    shutil.copy(assets / name, media / name)
subprocess.run(
    ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(assets / "clip20.mp4"),
     "-c:v", "libx264", "-bf", "0", "-crf", "12", "-preset", "medium", "-pix_fmt", "yuv420p", "-an",
     str(media / "clip20.mp4")],
    check=True,
)


def font(name: str) -> str:
    return "data:font/ttf;base64," + base64.b64encode((assets / name).read_bytes()).decode()


(out / "fonts.gen.ts").write_text(
    f'export const ROBOTO_REGULAR = "{font("Roboto-Regular.ttf")}";\n'
    f'export const ROBOTO_BOLD = "{font("Roboto-Bold.ttf")}";\n',
    encoding="utf-8",
    newline="\n",
)
