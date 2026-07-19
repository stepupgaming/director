#!/bin/bash
set -e

mkdir -p assets

echo "⬇️ Downloading Assets..."

# 1. Fonts (committed under crates/director-core/assets/fonts/ for deterministic rendering)
echo "  - Fetching Fonts..."
FONT_DIR="crates/director-core/assets/fonts"
mkdir -p "$FONT_DIR"
# All of these are required for export/CI determinism (Plan 06).
# Prefer jsDelivr (raw githubusercontent often returns HTML without following).
curl -L -o "$FONT_DIR/Roboto-Regular.ttf" "https://cdn.jsdelivr.net/gh/googlefonts/roboto@v2.138/src/hinted/Roboto-Regular.ttf"
curl -L -o "$FONT_DIR/Roboto-Bold.ttf" "https://cdn.jsdelivr.net/gh/googlefonts/roboto@v2.138/src/hinted/Roboto-Bold.ttf"
curl -L -o "$FONT_DIR/Roboto-Italic.ttf" "https://cdn.jsdelivr.net/gh/googlefonts/roboto@v2.138/src/hinted/Roboto-Italic.ttf"
# Inter ships as a variable font in google/fonts; use it for Regular + Bold slots.
curl -L -o "$FONT_DIR/Inter-Regular.ttf" "https://cdn.jsdelivr.net/gh/google/fonts@main/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf"
cp -f "$FONT_DIR/Inter-Regular.ttf" "$FONT_DIR/Inter-Bold.ttf"
curl -L -o "$FONT_DIR/Poppins-Bold.ttf" "https://cdn.jsdelivr.net/gh/google/fonts@main/ofl/poppins/Poppins-Bold.ttf"
# Optional emoji fallback (used by load_font_fallback as assets/fonts/emoji.ttf)
curl -L -o assets/fonts/emoji.ttf "https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoColorEmoji.ttf" || echo "  (emoji font fetch failed; optional)"

# 2. Images (Abstract Tech)
echo "  - Fetching Images..."
curl -L -o assets/img1.jpg "https://picsum.photos/seed/tech1/800/600"
curl -L -o assets/img2.jpg "https://picsum.photos/seed/tech2/800/600"
curl -L -o assets/img3.jpg "https://picsum.photos/seed/tech3/800/600"
curl -L -o assets/img4.jpg "https://picsum.photos/seed/tech4/800/600"
curl -L -o assets/img5.jpg "https://picsum.photos/seed/tech5/800/600"
curl -L -o assets/img6.jpg "https://picsum.photos/seed/tech6/800/600"

# 3. Video
echo "  - Fetching Background Video..."
# Use Jellyfish as fallback for high quality video test
curl -L -o assets/background.mp4 "https://test-videos.co.uk/vids/jellyfish/mp4/h264/1080/Jellyfish_1080_10s_5MB.mp4"

# 4. Audio (Upbeat)
echo "  - Fetching Audio..."
# Using SoundHelix-Song-1.mp3 (Techno/Trance style)
curl -L -o assets/music.mp3 "https://www.soundhelix.com/examples/mp3/SoundHelix-Song-1.mp3"

echo "✅ Assets downloaded to assets/"
ls -lh assets/
