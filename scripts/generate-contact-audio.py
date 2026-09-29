#!/usr/bin/env python3
"""Generate an original, restrained two-strike enemy contact warning."""
import math
from pathlib import Path
import struct
import wave

rate = 24000
length = 1.8
samples = []
for i in range(int(length * rate)):
    t = i / rate
    signal = 0.0
    for start, frequency in [(0.0, 196.0), (0.42, 146.83)]:
        age = t - start
        if age < 0:
            continue
        envelope = min(1.0, age / 0.012) * math.exp(-3.0 * age)
        # Low bell fundamental, slightly inharmonic metal partials, no siren.
        tone = (math.sin(2 * math.pi * frequency * age)
                + 0.32 * math.sin(2 * math.pi * frequency * 2.01 * age)
                + 0.12 * math.sin(2 * math.pi * frequency * 3.93 * age))
        signal += envelope * tone
    samples.append(signal * min(1.0, (length - t) / 0.12))
scale = 0.24 / max(abs(x) for x in samples)
path = Path(__file__).resolve().parents[1] / 'assets/audio/contact.wav'
with wave.open(str(path), 'wb') as out:
    out.setnchannels(1)
    out.setsampwidth(2)
    out.setframerate(rate)
    out.writeframes(b''.join(struct.pack('<h', round(x * scale * 32767)) for x in samples))
