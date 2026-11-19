#!/usr/bin/env python3
"""Synthesize Luminal's effect sounds: layered, stereo, 48 kHz, no dependencies.

Every sound is built from the same few parts: pitched bodies with a glide, shaped noise,
inharmonic partials for metal, and a small stereo reverb so nothing ends on a hard edge.
The output is deterministic. Run it again after changing a recipe:

    python3 scripts/generate-sfx.py [name ...]

`contact.wav`, `ambient.wav`, `launch.wav`, `beam.wav`, `impact.wav` and `alert.wav` are not
made here; see assets/audio/README.md.
"""
import math
from pathlib import Path
import random
import struct
import sys
import wave

RATE = 48000
PEAK = 0.24
TAU = 2 * math.pi
OUT = Path(__file__).resolve().parents[1] / 'assets/audio'


def silence(seconds):
    return [0.0] * int(seconds * RATE)


def env(t, attack, decay):
    """Linear attack, then exponential decay at `decay` nepers per second."""
    if t < 0:
        return 0.0
    return min(1.0, t / attack) * math.exp(-decay * max(0.0, t - attack))


def tone(seconds, freq, amp=1.0, attack=0.003, decay=6.0, start=0.0, vibrato=None):
    """A sine whose pitch follows `freq(t)` (Hz), phase-continuous."""
    out = silence(start + seconds)
    phase = 0.0
    first = int(start * RATE)
    for i in range(int(seconds * RATE)):
        t = i / RATE
        f = freq(t) if callable(freq) else freq
        if vibrato:
            f *= 1.0 + vibrato[1] * math.sin(TAU * vibrato[0] * t)
        phase += TAU * f / RATE
        out[first + i] = amp * env(t, attack, decay) * math.sin(phase)
    return out


def glide(a, b, rate):
    """Exponential glide from `a` to `b` Hz."""
    return lambda t: b + (a - b) * math.exp(-rate * t)


def noise(seconds, rng, amp=1.0):
    return [amp * rng.uniform(-1.0, 1.0) for _ in range(int(seconds * RATE))]


def lowpass(x, cutoff):
    """One pole. `cutoff` may be a function of time."""
    out, y = [], 0.0
    for i, v in enumerate(x):
        c = cutoff(i / RATE) if callable(cutoff) else cutoff
        a = 1.0 - math.exp(-TAU * c / RATE)
        y += a * (v - y)
        out.append(y)
    return out


def highpass(x, cutoff):
    low = lowpass(x, cutoff)
    return [v - l for v, l in zip(x, low)]


def bandpass(x, center, q=2.0):
    """RBJ band-pass, constant peak gain. `center` may be a function of time."""
    out = []
    x1 = x2 = y1 = y2 = 0.0
    for i, v in enumerate(x):
        f = center(i / RATE) if callable(center) else center
        w = TAU * min(f, RATE * 0.45) / RATE
        alpha = math.sin(w) / (2 * q)
        a0 = 1 + alpha
        y = (alpha * v - alpha * x2 + 2 * math.cos(w) * y1 - (1 - alpha) * y2) / a0
        x2, x1, y2, y1 = x1, v, y1, y
        out.append(y)
    return out


def shape(x, attack, decay, start=0.0):
    return [v * env(i / RATE - start, attack, decay) for i, v in enumerate(x)]


def mix(*parts):
    out = silence(0)
    for part in parts:
        if len(part) > len(out):
            out.extend([0.0] * (len(part) - len(out)))
        for i, v in enumerate(part):
            out[i] += v
    return out


def delayed(x, seconds, amp=1.0):
    return silence(seconds) + [v * amp for v in x]


def pan(mono, position):
    """Equal power. `position` is -1 (left) to 1 (right), or a function of time."""
    left, right = [], []
    for i, v in enumerate(mono):
        p = position(i / RATE) if callable(position) else position
        angle = (max(-1.0, min(1.0, p)) + 1.0) * math.pi / 4
        left.append(v * math.cos(angle))
        right.append(v * math.sin(angle))
    return left, right


def stereo(*layers):
    """Sum `(mono, position)` layers into a stereo pair."""
    lefts, rights = [], []
    for mono, position in layers:
        l, r = pan(mono, position)
        lefts.append(l)
        rights.append(r)
    return mix(*lefts), mix(*rights)


def reverb(left, right, wet=0.25, decay=0.78, tail=1.2):
    """Four combs and two all-passes a side, with different lengths left and right."""
    def side(x, combs, allpasses):
        x = x + silence(tail)
        acc = [0.0] * len(x)
        for length in combs:
            buf = [0.0] * length
            damp = 0.0
            for i, v in enumerate(x):
                j = i % length
                y = buf[j]
                damp = 0.7 * y + 0.3 * damp
                buf[j] = v + damp * decay
                acc[i] += y
        acc = [v / len(combs) for v in acc]
        for length in allpasses:
            buf = [0.0] * length
            for i, v in enumerate(acc):
                j = i % length
                y = buf[j]
                buf[j] = v + 0.5 * y
                acc[i] = y - 0.5 * v
        return [d + wet * w for d, w in zip(x, acc)]
    return (side(left, [1687, 1933, 2251, 2543], [347, 113]),
            side(right, [1733, 1999, 2203, 2617], [331, 127]))


def write(name, pair, fade=0.04):
    left, right = pair
    n = max(len(left), len(right))
    left = left + [0.0] * (n - len(left))
    right = right + [0.0] * (n - len(right))
    # Drop a tail that has already died, then close on a short fade.
    floor = 0.0008 * max(max(abs(v) for v in left), max(abs(v) for v in right))
    while n > RATE // 20 and abs(left[n - 1]) < floor and abs(right[n - 1]) < floor:
        n -= 1
    left, right = left[:n], right[:n]
    for k in range(min(n, int(fade * RATE))):
        g = k / (fade * RATE)
        left[n - 1 - k] *= g
        right[n - 1 - k] *= g
    scale = PEAK / max(max(abs(v) for v in left), max(abs(v) for v in right))
    with wave.open(str(OUT / f'{name}.wav'), 'wb') as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(b''.join(struct.pack('<hh', round(l * scale * 32767), round(r * scale * 32767))
                                 for l, r in zip(left, right)))
    print(f'{name}.wav  {n / RATE:.2f} s')


# --- recipes -------------------------------------------------------------------------

def click(rng):
    """A console key: a dull thunk under a short bright tick."""
    tick = tone(0.05, 2900, 0.5, 0.0005, 260)
    grain = shape(bandpass(noise(0.03, rng), 4200, 1.2), 0.0004, 420)
    thunk = tone(0.08, glide(240, 150, 60), 0.9, 0.001, 70)
    return reverb(*stereo((tick, 0.1), (grain, -0.1), (thunk, 0.0)), wet=0.10, decay=0.5, tail=0.15)


def ping(rng):
    """Active sonar: a clean struck tone that rings out into the dark, with one far echo."""
    body = tone(2.2, glide(1190, 1160, 3), 1.0, 0.004, 2.4)
    under = tone(2.0, 580, 0.22, 0.01, 3.2)
    glass = tone(1.0, 3284, 0.10, 0.002, 7.0)
    strike = shape(bandpass(noise(0.02, rng), 2400, 1.5), 0.0005, 300)
    dry = mix(body, under, glass, [v * 0.5 for v in strike])
    echo = lowpass(delayed(dry, 0.36, 0.22), 1800)
    return reverb(*stereo((dry, -0.12), (echo, 0.55)), wet=0.42, decay=0.84, tail=2.0)


def pd(rng):
    """Point defence: three quick, small pulses. Busy, never loud."""
    layers = []
    for k, (start, side) in enumerate([(0.0, -0.4), (0.075, 0.35), (0.16, -0.1)]):
        pulse = tone(0.07, glide(3100 - 250 * k, 1700, 55), 0.7, 0.0008, 70, start=start)
        tick = delayed(shape(highpass(noise(0.02, rng), 4000), 0.0004, 350), start, 0.4)
        layers += [(pulse, side), (tick, side)]
    return reverb(*stereo(*layers), wet=0.16, decay=0.6, tail=0.4)


def spinal(rng):
    """The spinal mount: a bass drop, a wide tearing sweep, and the hull ringing after it."""
    bass = tone(2.0, glide(74, 46, 7), 1.0, 0.006, 2.6)
    sweep = tone(1.2, glide(1400, 150, 12), 0.5, 0.004, 5.5)
    crack = shape(noise(0.25, rng), 0.0005, 30)
    rumble = shape(lowpass(noise(2.2, rng), 220), 0.02, 2.2)
    ring = mix(tone(1.8, 311, 0.10, 0.02, 2.8), tone(1.6, 467.5, 0.07, 0.02, 3.4), tone(1.2, 829, 0.04, 0.02, 4.6))
    return reverb(*stereo((bass, 0.0), (sweep, lambda t: -0.6 + 1.2 * min(1.0, t / 0.6)), ([v * 0.45 for v in crack], 0.0),
                          ([v * 2.4 for v in rumble], -0.15), (ring, 0.3)),
                  wet=0.3, decay=0.82, tail=1.6)


def explosion(rng):
    """A warhead or a ship going up, heard through the hull: mostly low, long, and far away."""
    # The second harmonic carries the boom on speakers that cannot reach the fundamental.
    boom = mix(tone(2.4, glide(58, 27, 3.0), 1.0, 0.006, 2.6), tone(1.6, glide(116, 54, 3.0), 0.4, 0.006, 3.6))
    body = lowpass(noise(2.6, rng), glide(950, 110, 2.4))
    body = [v * env(i / RATE, 0.008, 2.3) for i, v in enumerate(body)]
    crack = shape(highpass(noise(0.12, rng), 2500), 0.0005, 42)
    after = mix(*(delayed(shape(lowpass(noise(0.6, rng), 160), 0.03, 5.0), at, amp) for at, amp in ((0.27, 1.6), (0.55, 1.1), (0.92, 0.7))))
    return reverb(*stereo((boom, 0.0), ([v * 3.4 for v in body], -0.2), ([v * 0.3 for v in crack], 0.15), (after, 0.3)),
                  wet=0.36, decay=0.86, tail=2.2)


def jump_spool(rng):
    """A jump drive taking charge: a rising chord that never quite arrives."""
    seconds = 2.0
    swell = lambda t: (min(1.0, t / 1.25) ** 1.6) * math.exp(-3.2 * max(0.0, t - 1.25))
    voices = []
    for base, detune, side in ((1.0, 1.0, -0.4), (1.5, 1.004, 0.4), (2.0, 0.997, 0.0)):
        voice = tone(seconds, lambda t, b=base * detune: b * (150 + 470 * (1 - math.exp(-1.5 * t))), 0.5 / base, 0.01, 0.0)
        voices.append(([v * swell(i / RATE) for i, v in enumerate(voice)], side))
    air = bandpass(noise(seconds, rng), lambda t: 900 + 3200 * min(1.0, t / 1.3), 2.0)
    air = [v * 0.8 * swell(i / RATE) for i, v in enumerate(air)]
    return reverb(*stereo(*voices, (air, lambda t: -0.5 + t / seconds)), wet=0.3, decay=0.82, tail=1.2)


def jump(rng):
    """The jump itself: space drawn in, a deep knock, and bright fragments scattering."""
    draw = bandpass(noise(0.36, rng), lambda t: 300 + 5200 * (t / 0.36) ** 2, 1.4)
    draw = [v * (i / len(draw)) ** 2.2 for i, v in enumerate(draw)]
    knock = tone(1.3, glide(64, 33, 5), 1.0, 0.003, 4.2, start=0.36)
    snap = delayed(shape(noise(0.08, rng), 0.0004, 60), 0.36, 0.35)
    scatter = [(tone(1.2, rng.uniform(900, 3400), rng.uniform(0.05, 0.12), 0.002, rng.uniform(3.5, 8.0), start=0.36 + rng.uniform(0.0, 0.05)),
                rng.uniform(-0.8, 0.8)) for _ in range(9)]
    return reverb(*stereo(([v * 1.6 for v in draw], lambda t: -0.7 + 1.4 * min(1.0, t / 0.36)), (knock, 0.0), (snap, 0.0), *scatter),
                  wet=0.38, decay=0.86, tail=1.8)


def drive(rng):
    """The drive under thrust, as a seamless loop: low, felt more than heard."""
    seconds, cross = 3.0, 0.5
    def side(seed_shift):
        brown, y = [], 0.0
        for _ in range(int((seconds + cross) * RATE)):
            y = 0.995 * y + 0.005 * rng.uniform(-1.0, 1.0)
            brown.append(y * 18.0)
        # Nothing below the drive's own note: subsonic noise only eats headroom.
        rumble = highpass(lowpass(brown, 170), 28)
        n = int(seconds * RATE)
        # Cross-fade the end of the noise into its start. Equal power, because the two are unrelated.
        head = rumble[:n]
        for k in range(int(cross * RATE)):
            g = k / (cross * RATE)
            head[k] = head[k] * math.sin(g * math.pi / 2) + rumble[n + k] * math.cos(g * math.pi / 2)
        out = []
        for i, v in enumerate(head):
            t = i / RATE
            # Whole numbers of cycles in three seconds, so the pitched part loops on its own.
            pitched = (0.55 * math.sin(TAU * 55 * t + seed_shift) + 0.3 * math.sin(TAU * (82 + 1 / 3) * t)
                       + 0.12 * math.sin(TAU * 110 * t + 2 * seed_shift))
            out.append(v + pitched * (0.85 + 0.15 * math.sin(TAU * t / 3.0)))
        return out
    return side(0.0), side(1.1)


# Launch, beam, the hit on screens and the alert are the earlier Curds renders, kept by choice.
RECIPES = {'click': click, 'ping': ping, 'pd': pd, 'spinal': spinal, 'explosion': explosion,
           'jump-spool': jump_spool, 'jump': jump, 'drive': drive}

if __name__ == '__main__':
    for index, (name, recipe) in enumerate(RECIPES.items()):
        if sys.argv[1:] and name not in sys.argv[1:]:
            continue
        # A loop must not be faded or trimmed.
        write(name, recipe(random.Random(7080 + index)), fade=0.0 if name == 'drive' else 0.04)
