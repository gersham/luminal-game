# Ship marker concept — review only

Generated raster concept sheets, not used by the game. Open `ship-class-recognition-hires.png`
in an image editor. The black background is part of this review sheet; production
sprites would need transparent backgrounds and small-size readability checks.

Rows: green, blue, red. All ships point upwards.
Columns: picket, frigate, destroyer, cruiser, battleship, civilian.

Class recognition comes from silhouette: compact dart, needle hull, forked bow,
broad shoulders, heavy hull with engine banks, and cargo pods respectively.
This is the recommended approach for tiny rotating map markers. Engine count or
small flank bars could reinforce class at larger zoom levels; avoid lettering
that becomes unreadable when rotated. Keep the same hull geometry across colours.

Generated with Curds / OpenAI `gpt-image-2.5-flare`, 2026-09-29.
Current prompt: `serious-prompt.txt`. The original `ship-class-colours.png` and
`prompt.txt` are the rejected, overly cartoonish first direction. The revision
uses restrained flat silhouettes, muted colours and minimal structural lines. Output: 1536 × 1024 RGB PNG.
Sanitized command: `curds -no-tui -provider openai -aspect-ratio 3:2 -quality high -prompt <prompt text> -output <ship-class-colours-serious.png>`.

## Recognition-chart revision

`ship-class-recognition.png` is the third direction: thin coloured outlines,
black interiors and industrial hull geometry. The first two sheets were rejected.
Prompt: `recognition-prompt.txt`. Curds / OpenAI `gpt-image-2.5-flare`;
1536 × 1024 RGB PNG, verified and visually reviewed. Still not integrated.
Sanitized command: `curds -no-tui -provider openai -aspect-ratio 3:2 -quality high -prompt <recognition prompt> -output <ship-class-recognition.png>`.

## Higher-resolution master

`ship-class-recognition-hires.png`: verified 3072 × 2048 RGB PNG (four times
as many pixels as the previous sheet), regenerated using that sheet as an image
reference. Minor linework and colour differences are possible in regeneration.
Prompt: `recognition-hires-prompt.txt`. Curds / OpenAI `gpt-image-2.5-flare`.
Sanitized command: `curds -no-tui -provider openai -size 3072x2048 -quality high -input-image <ship-class-recognition.png> -prompt <hires prompt> -output <ship-class-recognition-hires.png>`.
Opened in Pinta for review; not applied to the game.
