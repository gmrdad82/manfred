import json
import math
import random
import struct
import sys


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def record(seed):
    r = random.Random(seed)
    calls = []
    for k in range(40):
        calls.append(["random", [], bits(r.random())])
        calls.append(["uniform", [bits(-0.04), bits(0.04)], bits(r.uniform(-0.04, 0.04))])
        calls.append(["uniform", [bits(0.0), bits(math.pi)], bits(r.uniform(0, math.pi))])
        calls.append(["randrange", [1, 16], r.randrange(1, 16)])
        calls.append(["randrange", [0, 1000003], r.randrange(0, 1000003)])
        calls.append(["randrange", [-5, 3], r.randrange(-5, 3)])
        calls.append(["randrange", [0, 1 << 40], r.randrange(0, 1 << 40)])
    return {"seed": seed, "calls": calls}


json.dump(
    {
        "command": "python3 crates/generators/tests/pyrandom.py > crates/generators/tests/pyrandom.json",
        "python": sys.version.split()[0],
        "seeds": [record(s) for s in (14, 0, 2026, -7, 1 << 40 | 5)],
    },
    sys.stdout,
    separators=(",", ":"),
)
print()
