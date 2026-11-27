# Caption keys

Generated from `crates/director-core/src/node/caption_keys.rs` by `director-engine captions keys --format markdown`. Do not edit by hand.

Pixel keys are frame pixels. `preset` applies first and its pixel values scale with the frame height from a 1920 px frame; every other key overrides it.

## Type

| Key | Takes | Default | What it does |
|---|---|---|---|
| `size` | 0.005 to 0.5 | unset | Font size as a fraction of the frame height; sets font_size. 0.035 is 67 px on a 1920 px frame. |
| `font_size` | px | 42 | Font size in frame pixels. |
| `font_family` | font family | Poppins | Font family. Bundled: Poppins (bold), Inter, Roboto, Archivo (variable weight), JetBrains Mono. |
| `font_weight` (`weight`) | 100 to 900 or a name | 700 | Font weight. |
| `uppercase` | true / false | false | Paint every word in capitals. |
| `text_direction` | auto / ltr / rtl | auto | Paragraph direction; auto reads the first strong character. |

## Colour

| Key | Takes | Default | What it does |
|---|---|---|---|
| `color` | hex colour | #FFFFFF | Words not yet spoken. |
| `highlight_color` | hex colour | #FFD60A | The word being spoken. |
| `spoken_color` | hex colour | #C7C7C7 | Words already spoken. |
| `future_opacity` | 0 to 1 | 0.92 | Opacity of words not yet spoken. |
| `active_opacity` | 0 to 1 | 1 | Opacity of the word being spoken. |
| `spoken_opacity` | 0 to 1 | 0.55 | Opacity of words already spoken. |
| `color_transition_duration` | 0 to 1 s | 0 | Seconds the highlight fades in before a word starts; 0 lands it on the word's first frame. |

## Outline, shadow and glow

| Key | Takes | Default | What it does |
|---|---|---|---|
| `stroke_enabled` | true / false | true | Paint the outline. |
| `stroke_width` | px | 2 | Outline width outside the glyph edge, round joined. |
| `stroke_color` | hex colour | #000000EB | Outline colour. |
| `text_shadow_enabled` | true / false | true | Paint the drop shadow. |
| `text_shadow_color` | hex colour | #000000D9 | Drop shadow colour. |
| `text_shadow_blur` | px | 4 | Drop shadow blur. |
| `text_shadow_offset_x` | px | 0 | Drop shadow offset to the right. |
| `text_shadow_offset_y` | px | 2 | Drop shadow offset down. |
| `glow_enabled` | true / false | true | Glow around the word being spoken, in its highlight colour. |
| `glow_blur` | px | 4 | Glow blur. |

## Pill

| Key | Takes | Default | What it does |
|---|---|---|---|
| `pill_enabled` | true / false | true | Paint a rounded box behind each line. |
| `pill_color` | hex colour | #0000009E | Box colour. |
| `pill_padding_h` | px | 18 | Box padding left and right of the line. |
| `pill_padding_v` | px | 10 | Box padding above and below the line. |
| `pill_radius` | px | 14 | Box corner radius. |

## Placement

| Key | Takes | Default | What it does |
|---|---|---|---|
| `caption_position` | anchor | bottom | Anchor: bottom, top, center, left, right, top_left, top_right, bottom_left, bottom_right, free(y) with y from 0 to 1, or free_xy(x, y) in frame pixels. |
| `top_margin` | px | 60 | Gap above a top anchor. |
| `bottom_margin` | px | 110 | Gap below a bottom anchor. |
| `left_margin` | px | 60 | Gap left of a left anchor. |
| `right_margin` | px | 60 | Gap right of a right anchor. |
| `safe_area` (`safe_area_preset`) | desktop / instagram_reel / instagram_story / linkedin / mobile / tiktok / twitter / youtube_shorts | unset | Platform safe area; sets the top and bottom margins at its 1080x1920 size. |
| `layout_width_fraction` | 0.05 to 1 | 0.86 | Text column width as a fraction of the caption box. |

## Lines

| Key | Takes | Default | What it does |
|---|---|---|---|
| `auto_phrase_break` | true / false | true | Split cues into display lines. |
| `line_break` | width / words | width | width fits each line to the measured column, balanced, ending lines at clause ends, with no article, preposition or conjunction at a line end and no short word alone (a word wider than the column shrinks to fit); words cuts fixed chunks of max_words_per_line. |
| `max_words_per_line` | whole, at least 1 | 4 | Most words on one line. |
| `max_lines` | whole, at least 1 | 1 | Lines shown together in replace mode; each page of lines replaces the last. |
| `phrase_line_mode` | replace / stack | replace | replace shows one page of lines at a time; stack keeps every revealed line of the cue. |
| `line_spacing` | px | 16 | Gap between stacked lines. |
| `line_reveal_offset_1` | 0 to 1 | 0.45 | When an estimated cue's second line appears, as a fraction of the cue. |
| `line_reveal_duration_1` | 0 to 2 s | 0.2 | Seconds the second line fades in. |

## Motion

| Key | Takes | Default | What it does |
|---|---|---|---|
| `active_pop_scale` | 1 to 1.1 | 1.04 | Scale of the word being spoken on its first frames. |
| `active_pop_duration` | 0.05 to 0.15 s | 0.08 | Seconds the pop settles over. |

## Emphasis

| Key | Takes | Default | What it does |
|---|---|---|---|
| `emphasis_color` | hex colour | unset | Colour of the emphasis words while they are not being spoken. |
| `emphasis_words` | words | unset | Words to paint in emphasis_color, matched without case or edge punctuation. |

## Transcript and cues

Give the words with one of `file` (HTML `src`: a word-timed `.json`, `.vtt` or `.srt`), `words` (Rhai `[#{ text, start, end }]`) or `media` (transcribed with the speech model). A word-timed transcript groups into cues by these rules; `.vtt` and `.srt` cues keep their own times.

| Key | Takes | Default | What it does |
|---|---|---|---|
| `max_cue_chars` | whole, at least 1 | 42 | Most characters on a cue; a cue cut for length ends at its last clause end, or before its trailing weak words. |
| `max_cue_duration` | s | 5 | Longest cue. |
| `max_cue_gap` | s | 0.8 | A pause of at least this long ends the cue. |
| `break_on_sentence` | true / false | true | A word ending in `.`, `?` or `!` ends the cue. |
| `cue_hold` | s | 0.5 | Seconds a cue stays up after its last word, cut short by the next cue, so short pauses never blank the screen. 0 ends each cue on its last word. |

## Placement track

`positions` moves the caption over time: Rhai `positions: [#{ at: 4.0, caption_position: "free(0.4)" }]`, HTML `<position at="4s" caption-position="top">` children. Each entry takes `at` (seconds) and any of `caption_position`, `top_margin`, `bottom_margin`, `left_margin`, `right_margin`; unset keys keep the caption's own value. The latest entry at or before the current time wins, so one caption can sit at its own height in every shot. `free(y)` centres the line at `y` of the frame height.

## Presets

| Preset | Look |
|---|---|
| `bold-outline` | Heavy capitals with a thick black outline and a yellow active word, three words a line. |
| `karaoke` | Two-line pages; spoken words fill with the highlight colour as the voice reaches them. |
| `pill` | White words on a dark rounded box per line with a yellow active word. |
| `pop` | Two big words at a time, a green active word that pops, heavy outline and shadow. |
| `subtitle` | Plain two-line subtitles: regular weight, soft shadow, no highlight. |
| `boxed` | Black words on a white box per line with a red active word. |
