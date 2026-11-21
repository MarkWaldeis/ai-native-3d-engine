"""Regenerate the small, dependency-free mono WAV used by the audio garden."""
import math
from pathlib import Path
import struct
import wave

rate = 16000
duration = 1
samples = []
for i in range(rate * duration):
    t = i / rate
    envelope = min(1, t / 0.01) * math.exp(-6 * t)
    value = (math.sin(2 * math.pi * 660 * t) + 0.3 * math.sin(2 * math.pi * 990 * t)) * envelope
    samples.append(struct.pack("<h", round(value * 10000)))
with wave.open(str(Path(__file__).with_name("chime.wav")), "wb") as output:
    output.setparams((1, 2, rate, 0, "NONE", "not compressed"))
    output.writeframes(b"".join(samples))
