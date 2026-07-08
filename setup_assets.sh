#!/bin/bash
set -e

mkdir -p assets

echo "⬇️ Downloading Assets..."

# 1. Fonts (committed under crates/director-core/assets/fonts/ for deterministic rendering)
echo "  - Fetching Fonts..."
FONT_DIR="crates/director-core/assets/fonts"
mkdir -p "$FONT_DIR"
curl -L -o "$FONT_DIR/Roboto-Bold.ttf" "https://github.com/google/fonts/raw/main/apache/roboto/Roboto-Bold.ttf"
curl -L -o "$FONT_DIR/Roboto-Italic.ttf" "https://github.com/google/fonts/raw/main/apache/roboto/Roboto-Italic.ttf"
curl -L -o "$FONT_DIR/Inter-Regular.ttf" "https://github.com/google/fonts/raw/main/ofl/inter/Inter-Regular.ttf"
curl -L -o "$FONT_DIR/Inter-Bold.ttf" "https://github.com/google/fonts/raw/main/ofl/inter/Inter-Bold.ttf"
curl -L -o "$FONT_DIR/Poppins-Bold.ttf" "https://github.com/google/fonts/raw/main/ofl/poppins/Poppins-Bold.ttf"
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
