# Luminal audio

Generated on 2026-09-27 with Curds 0.6.0 (upstream `bbdffe7`). No credentials
are needed to play these embedded assets; the private Curds configuration is
outside the repository.

- Effects: Replicate `stability-ai/stable-audio-2.5`, via
  `curds -no-tui -provider replicate -model sfx -duration 1|2 -prompt … -output …wav`.
  Eight distinct prompts requested a quiet tactile click, two-note contact cue,
  hollow sonar ping, contained launch whoosh, short beam discharge, muffled
  metallic impact, compact distant explosion, and restrained three-note alert.
  All exclude speech, music, and harsh/continuous alarms.
- Music: Replicate `elevenlabs/music`, via
  `curds -no-tui -provider replicate -model music -duration 60 -prompt … -output …wav`.
  Prompt: looping ambient science-fiction background; no beat, drums,
  percussion or pulse; slowly evolving orchestral/synth pads and distant
  wordless choir-like textures; restrained cosmic awe and gentle tension;
  no speech, lyrics, demanding melody, or dramatic stingers.

Effects have leading silence removed, are limited to one second, normalized
to -24 LUFS / -9 dBTP then attenuated by 0.7, faded out, and stored as mono
24 kHz PCM WAV. The 60-second music render is turned into a 56-second loop:
the last four seconds crossfade into the first four, followed continuously by
the middle section. Music is normalized to -28 LUFS / -9 dBTP, attenuated by
0.7, and stored as stereo 24 kHz PCM WAV.

Playback defaults: effects 35%, music 25%, with quieter UI/beam/ping gains.
The UI offers separate sliders and a global mute. Effect playback is capped
at four voices, with wall-clock cooldowns and no backlog at high game speeds.
Combat cues come only from the received player view, preserving light delays.
Music has a three-second initial fade-in. Audio device failure is nonfatal.

The contact cue was replaced on 2026-09-29 by the original synthesized warning
in `scripts/generate-contact-audio.py`: two low, descending bell strikes,
1.8 seconds, mono 24 kHz PCM, peak 0.24. It has priority over routine weapon
sounds and can replace an occupied voice; its two-second cooldown still applies.
The spinal cue is likewise generated locally by `scripts/generate-spinal-audio.py`.
