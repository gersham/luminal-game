#!/usr/bin/env python3
"""Generate the original, bounded spinal-mount discharge effect (no dependencies)."""
import math
from pathlib import Path
import random
import struct
import wave

rng = random.Random(708)
rate = 24000
samples = []
low = 0.0
for i in range(int(1.6 * rate)):
    t = i / rate
    noise = rng.uniform(-1.0, 1.0)
    low = 0.94 * low + 0.06 * noise
    attack = min(1.0, t / 0.006)
    bass = math.sin(2 * math.pi * (48*t + 26*(1-math.exp(-7*t))/7))
    sweep = math.sin(2 * math.pi * (160*t + 1250*(1-math.exp(-14*t))/14))
    signal = attack * (0.62*bass*math.exp(-3.5*t) + 0.2*sweep*math.exp(-7*t)
                       + 0.3*noise*math.exp(-28*t) + 1.2*low*math.exp(-3*t))
    samples.append(signal * min(1.0, (1.6-t)/0.05))
scale = 0.24 / max(abs(x) for x in samples)
path = Path(__file__).resolve().parents[1] / 'assets/audio/spinal.wav'
with wave.open(str(path), 'wb') as out:
    out.setnchannels(1)
    out.setsampwidth(2)
    out.setframerate(rate)
    out.writeframes(b''.join(struct.pack('<h', round(x*scale*32767)) for x in samples))
