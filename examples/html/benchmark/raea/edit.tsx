// The plan 11 B3 benchmark clip as a raea composition: the same six 20 s scenes, layout and motion as
// ../index.html, with each GSAP tween written as a clamped interpolate on the frame. GSAP's power3 is a
// quartic, power2 a cubic, and fromTo shows its from values before it starts, as a clamp does.
// stage.py builds the folder raea renders from: this file, media/ and fonts.gen.ts, which exports the two
// Roboto faces as data URLs (raea 0.1.0-canary.2 on Windows misreads a font file path). The SVG star turns
// inside a div because raea hands a CSS transform on <svg> to usvg, which cannot parse `deg` units.
import { ROBOTO_BOLD, ROBOTO_REGULAR } from "./fonts.gen";
import { AbsoluteFill, Audio, Composition, Easing, Img, Sequence, Video, interpolate, registerRoot, staticFile, useCurrentFrame } from "raea";

const FPS = 30;
const SCENE = 20 * FPS;

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const power3Out = Easing.out(Easing.poly(4));
const power2In = Easing.poly(3);
const expoInOut = Easing.inOut(Easing.exp);
const backOut = Easing.out(Easing.back(1.7));
const linear = Easing.linear;

/** A tween from `start` seconds lasting `dur` seconds between `from` and `to`. */
function tween(frame: number, start: number, dur: number, from: number, to: number, easing = power3Out) {
  return interpolate(frame, [start * FPS, (start + dur) * FPS], [from, to], { ...clamp, easing });
}

const SCENES = [
  { title: "Native video, from HTML.", sub: "Text, shapes, a photo, footage and sound in one pass.", color: "#f472b6", photo: "photo_a.jpg" },
  { title: "Layout is Taffy.", sub: "Flexbox and grid lay out every scene before Skia paints it.", color: "#60a5fa", photo: "photo_b.jpg" },
  { title: "GSAP compiles to tracks.", sub: "Timelines become keyframe tracks at compile time.", color: "#34d399", photo: "photo_a.jpg" },
  { title: "Footage decodes in process.", sub: "Video layers stream frames through FFmpeg.", color: "#fbbf24", photo: "photo_b.jpg" },
  { title: "Audio mixes in the same pass.", sub: "A bed under every scene, encoded with the picture.", color: "#a78bfa", photo: "photo_a.jpg" },
  { title: "Same source, two engines.", sub: "This page renders in HyperFrames and in Director.", color: "#f87171", photo: "photo_b.jpg" },
];

const CSS = `
  @font-face { font-family: Roboto; src: url("${ROBOTO_REGULAR}"); font-weight: 400; }
  @font-face { font-family: Roboto; src: url("${ROBOTO_BOLD}"); font-weight: 700; }
  * { box-sizing: border-box; }
  .bench { position: absolute; inset: 0; overflow: hidden; background: linear-gradient(150deg, #0b1020 0%, #1b2440 100%); font-family: Roboto; color: #fff; }
  .title { position: absolute; left: 120px; top: 150px; width: 1000px; margin: 0; font-size: 84px; font-weight: 700; line-height: 1.1; }
  .sub { position: absolute; left: 124px; top: 360px; width: 900px; margin: 0; font-size: 34px; color: #c7d2fe; }
  .bar { position: absolute; left: 124px; top: 450px; width: 560px; height: 10px; border-radius: 5px; transform-origin: 0 50%; }
  .shape { position: absolute; top: 560px; width: 180px; height: 180px; border-radius: 28px; }
  .s1 { left: 124px; } .s2 { left: 344px; border-radius: 90px; } .s3 { left: 564px; }
  .star { position: absolute; left: 820px; top: 560px; width: 180px; height: 180px; }
  .star svg { display: block; width: 180px; height: 180px; }
  .frame { position: absolute; left: 124px; top: 800px; width: 420px; height: 200px; overflow: hidden; border-radius: 20px; }
  .frame img { display: block; width: 420px; height: 200px; object-fit: cover; }
  .footage { position: absolute; left: 1160px; top: 150px; width: 640px; height: 360px; border-radius: 24px; }
  .count { position: absolute; left: 1160px; top: 560px; width: 640px; margin: 0; font-size: 160px; font-weight: 700; text-align: right; }
  .progress { position: absolute; left: 0; top: 1068px; width: 1920px; height: 12px; background: #f472b6; transform-origin: 0 50%; }
`;

function Scene({ index }: { index: number }) {
  const f = useCurrentFrame();
  const s = SCENES[index];
  const titleOpacity = tween(f, 0.2, 0.9, 0, 1) * tween(f, 19.2, 0.6, 1, 0, power2In);
  const titleX = tween(f, 0.2, 0.9, -120, 0);
  const titleY = tween(f, 19.2, 0.6, 0, -40, power2In);
  const shapes = [s.color, "#e5e7eb", s.color].map((background, i) => {
    const start = 1.2 + 0.15 * i;
    const y = tween(f, start, 0.8, 120, 0, backOut);
    const rotation = tween(f, start, 0.8, -8, 0, backOut) + tween(f, 4 + 0.5 * i, 6, 0, 360, linear);
    const opacity = tween(f, start, 0.8, 0, 1, backOut);
    return <div key={i} className={`shape s${i + 1}`} style={{ background, opacity, transform: `translate(0px, ${y}px) rotate(${rotation}deg)` }} />;
  });
  const starRotation = tween(f, 0, 20, 0, 720, linear);
  const starScale = tween(f, 0, 20, 0.7, 1, linear);
  return (
    <AbsoluteFill>
      <h1 className="title" style={{ opacity: titleOpacity, transform: `translate(${titleX}px, ${titleY}px)` }}>{s.title}</h1>
      <p className="sub" style={{ opacity: tween(f, 0.6, 0.8, 0, 1), transform: `translate(0px, ${tween(f, 0.6, 0.8, 30, 0)}px)` }}>{s.sub}</p>
      <div className="bar" style={{ background: s.color, transform: `scaleX(${tween(f, 0.9, 1.2, 0, 1, expoInOut)})` }} />
      {shapes}
      <div className="star" style={{ transform: `rotate(${starRotation}deg) scale(${starScale})` }}>
        <svg viewBox="0 0 100 100">
          <polygon points="50,4 61,38 97,38 68,59 79,94 50,73 21,94 32,59 3,38 39,38" fill={s.color} />
        </svg>
      </div>
      <div className="frame">
        <Img src={staticFile(s.photo)} style={{ transform: `scale(${tween(f, 0, 20, 1, 1.2, linear)})` }} />
      </div>
      <p className="count" style={{ opacity: tween(f, 1.5, 0.7, 0, 1), transform: `translate(0px, ${tween(f, 1.5, 0.7, 80, 0)}px)` }}>
        {String(index + 1).padStart(2, "0")}
      </p>
      <Video className="footage" src={staticFile("clip20.mp4")} muted />
      <Audio src={staticFile("bed20.wav")} volume={0.5} />
    </AbsoluteFill>
  );
}

function Bench() {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill className="bench">
      <style>{CSS}</style>
      {SCENES.map((_, i) => (
        <Sequence key={i} from={i * SCENE} durationInFrames={SCENE}>
          <Scene index={i} />
        </Sequence>
      ))}
      <div className="progress" style={{ transform: `scaleX(${interpolate(f, [0, 120 * FPS], [0, 1], clamp)})` }} />
    </AbsoluteFill>
  );
}

registerRoot(() => <Composition id="bench" component={Bench} width={1920} height={1080} fps={FPS} durationInFrames={6 * SCENE} />);
