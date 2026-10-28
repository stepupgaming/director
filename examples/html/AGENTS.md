# Purpose

- Own executable HTML/CSS/HyperFrames reference compositions for the native compiler.

## Ownership

- `native-motion.html` covers layout, typography, SVG, gradients, grouped opacity and CSS animation at 1080p/30fps.
- `timeline-motion.html` drives title, rule and card motion from a `director-timeline/1` script at 1080p/30fps.
- `gsap-motion.html` drives the same kind of motion from a GSAP timeline (labels, stagger, yoyo, GSAP eases) that compiles to native tracks at 1080p/30fps. `crates/director-html/tests/gsap.rs` checks its tracks.
- `motion-path.html` flies a box along a dashed SVG route inside a scaled viewBox and a rotated wrapper with MotionPath `align` and `autoRotate`, morphs a square into a star and back with MorphSVG, sharpens the title from a 12px blur and sets it pink at 2.25 s, pulses a box-shadow glow on the rocket, wipes the morph in with a clip-path inset, at 1080p/30fps. `crates/director-html/tests/gsap.rs` checks the route end and the pixels.
- `kinetic-rewind.html` cuts three GSAP-punched words and a progress bar on a film clock, then a `time` track rewinds the film from 3.2 s to 0 s under a blur, contrast and desaturate look and a flickering REWIND label on the composition clock, and cuts to an end card at 1080p/30fps. `crates/director-html/tests/rewind.rs` covers the clock contract.
- `kinetic-beats.html` sets a 128 BPM grid on its root, cuts three words on bars, kicks a ring on every beat through `director.beat`, flashes into the end card from beat-time timeline keys and fires `../assets/tone.wav` on the cut at 1080p/30fps. `crates/director-html/tests/beats.rs` covers the beat contract.
- `kinetic-counters.html` rolls a `$2,048,000` revenue reel with two spins and a `+312%` growth reel down from `000` with `data-reel`, under an edge fade mask and motion blur at 1080p/30fps. `crates/director-html/tests/reel.rs` covers the reel contract.
- `split-text.html` splits a title into lines, words and chars with GSAP SplitText and animates an inline-block badge and an inline span on a paragraph at 1080p/30fps.
- `image-background.html`, `text-decoration.html`, and `nested-rule.html` are native references for a data-URI background, a solid underline, and a nested `& .title` rule. `crates/director-html/tests/authoring_slices.rs` renders them.

## Local Contracts

- Keep sample assets local or embedded. Native compilation must succeed without browser extraction.
- CSS-driven composition roots declare `data-no-timeline` so upstream HyperFrames does not wait for GSAP registration.
- Generated PNG/MP4 and parity reports belong under ignored `target/`.

## Work Guidance

- Add compiler regressions when examples exercise new behavior.

## Verification

- `cargo run -p director-cli --bin director-engine -- html check examples/html/native-motion.html`
- `cargo run -p director-cli --bin director-engine -- html render examples/html/native-motion.html -o target/html-native.mp4`

## Child DOX Index

- None.
