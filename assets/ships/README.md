# Ship recognition art

Generated 2026-09-29 using the `telemetryos-eng:curds` skill and curds 0.3.1,
OpenAI provider, installed default model `gpt-image-2.5-flare`. These are original
generated recognition illustrations inspired by their respective settings, not
official technical drawings. The user describes this game as personal use only,
with no intention to distribute it.

All four PNGs are 1024 × 1024, white line art on black. The application may use
line brightness as alpha to tint the art against its own background. Preserve
the aspect ratio of the selected source region when drawing it.

Each sheet was generated with:

```sh
curds -no-tui -provider openai -aspect-ratio 1:1 -prompt "$PROMPT" -output assets/ships/THEME.png
```

The prompt requested a WWII-style spacecraft recognition chart, five horizontal
rows ordered by increasing capability, noses to the right, white contours and
sparse internal lines on black, without labels, shading, stars or ocean ships.
Theme-specific designs were:

- **Luminal:** sensor picket with boom/dish, radiator-equipped frigate,
  multi-engine destroyer, armored cruiser, axial-weapon battleship.
- **Grim Dark:** gothic scout, Sword-inspired escort, Cobra-inspired destroyer,
  Lunar-inspired cruiser, Emperor-inspired battleship.
- **Imperium:** Scout/Courier wedge, needle patrol corvette, spearhead destroyer,
  flattened triangular cruiser, spherical dreadnought.
- **Culture:** drone picket capsule, light offensive ovoid, rapid offensive
  knife-like slab, general offensive ellipsoid, heavy offensive rounded slab.

All images were opened and visually inspected. Generation did not produce
exactly equal row spacing; use these full-width pixel Y bands (exclusive end),
in the game's five ascending tiers. Divide coordinates by 1024 for texture UVs.
Grim Dark produced an extra intermediate ship; its fourth generated row is
intentionally omitted.

| Sheet | Tier 1 | Tier 2 | Tier 3 | Tier 4 | Tier 5 |
|---|---|---|---|---|---|
| `luminal.png` | 10–145 | 145–337 | 337–522 | 522–728 | 728–1005 |
| `grim-dark.png` | 10–171 | 173–318 | 320–470 | 607–791 | 793–1017 |
| `imperium.png` | 10–155 | 158–303 | 306–489 | 490–688 | 689–1015 |
| `culture.png` | 40–175 | 195–340 | 365–505 | 520–740 | 755–985 |
