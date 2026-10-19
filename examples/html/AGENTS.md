# Purpose

- Own executable HTML/CSS/HyperFrames reference compositions for the native compiler.

## Ownership

- `native-motion.html` covers layout, typography, SVG, gradients, grouped opacity and CSS animation at 1080p/30fps.
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
