# Celestial surface atlases

Four matching 1024 × 1536 RGB PNG atlases, generated with Curds / OpenAI
`gpt-image-2.5-flare` on 2026-09-29. Each atlas has two columns and three rows,
512 × 512 pixels per tile, without gutters:

| Row | Left | Right |
| --- | --- | --- |
| 1 | Inhabited ocean world | Gas giant |
| 2 | Dry rock | Ice world |
| 3 | Cratered moon | Volcanic moon |

`style.txt` is the shared art direction. Luminal adds familiar blue-green
continents, ochre/cream gas bands, rust rock, pale blue ice, lunar greys and
charcoal basalt. The other themes use their adjacent prompt files and the
Luminal atlas as a reference image to preserve layout and art style.

Sanitized command: `curds -no-tui -provider openai -aspect-ratio 2:3 -quality high -prompt <theme prompt> [-input-image <luminal.png>] -output <theme.png>`.

The renderer wraps these albedo maps onto globes and computes sunlight direction,
star tint and eclipse visibility at runtime. Star glow and surface granulation
are procedural. Tiny bodies retain simple dots. Ship marker concepts in
`concepts/ship-icons` are independent and are not integrated.
