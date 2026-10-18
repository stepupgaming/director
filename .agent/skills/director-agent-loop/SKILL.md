---
name: director-agent-loop
description: Check, contact sheet, loudness, snapshot, onion, still, and render loop for a Director composition.
---

# Director agent loop

Use this before calling a Director HTML, Rhai, capture, or `.dirproj` render finished. The binary is `director-engine`. The input path is positional.

1. `director-engine check <file> --every 0.25`
   Stdout is JSON with `exit_code` and `findings`. `off_canvas` and `missing_font` are errors. `clipped` is a warning. Exit 2 when an error is present. Load failure prints stderr and exits 1 with no JSON. This is not `html check`.
2. `director-engine sheet <file> -n 8 -c 4 --cell-width 320 -o sheet.png`
   Stdout is JSON. The PNG is a grid of frames. More than 64 frames, or a raster above 16777216 pixels, exits 1. Paint or write failure exits 2.
3. `director-engine audio <file>`
   Stdout is JSON. Non-finite levels are null. A non-finite sample or more than 57600000 samples sets `error` and skips analysis. Exit 0 when the report prints, including silence.
4. `director-engine snapshot <file> --golden <name>.png -t <seconds>`
   A channel delta above 16 counts. More than 0.1% different pixels exits 2 and writes `<name>.actual.png` and a red `<name>.diff.png`. `--update` replaces the golden and exits 0.
5. `director-engine onion <file> -n 4 -o onion.png`
   Blends up to 8 frames. Weights are 1 through n, so later frames count more.
6. `director-engine still <file> -t <seconds> -o still.png`
7. `director-engine render <file> --scale 0.5 -o draft.mp4`, then the same command without `--scale` for the full frame.

`--scale` must be finite and in `(0, 1]`. Layout width and height stay put. The encoded size rounds each side down to an even count. Scale 1 keeps the layout size, including odd sizes.
