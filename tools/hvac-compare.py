#!/usr/bin/env python3
"""Runs an HVAC scenario against whichever UI runs on nest-qemu (stock
nlclient or the client) and records what it switched: nest-qemu's
backplate is restarted with the scenario's room and logs every frame;
mode and targets are set over MQTT (TESTING.md, "nestlocal and Home
Assistant, simulated"). Two runs, one per UI, compare with --diff.

    client-ng/tools/hvac-compare.py run heat-band stock.json
    client-ng/tools/hvac-compare.py run heat-band ours.json
    client-ng/tools/hvac-compare.py diff stock.json ours.json

Scenarios are steps of (seconds from start, "mode"/"target"/... value);
the backplate's room is fast (degrees an hour) so cycles come in minutes.
The UI is set to off first; the steps start once the backplate has
reported the room, and only what is switched from then on counts.
"""
import json
import os
import re
import signal
import struct
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QEMU = Path(os.environ.get("NEST_QEMU", next((p for p in (ROOT.parent / "nest-qemu", ROOT.parent / "qemu") if (p / "run.sh").exists()),
                                              ROOT.parent / "nest-qemu")))
SERIAL = os.environ.get("NEST_SERIAL", "09AA01AC25160NMZ")
PORT = os.environ.get("MQTT_PORT", "18831")

SCENARIOS = {
    # Heat at 21 °C from 19 °C: the band's bottom, the overshoot limit, a
    # restart, two cycles.
    "heat-band": dict(room=dict(temp=19.0, ambient=15, heat_rate=12, cool_rate=12, room_tau=2),
                      steps=[(0, "temperature_scale", "C"), (1, "mode", "heat"), (3, "target", "21")], seconds=2400),
    # Off, the room falling past the lower safety temperature (5 °C).
    "safety": dict(room=dict(temp=5.4, ambient=2, heat_rate=12, cool_rate=12, room_tau=1),
                   steps=[(0, "temperature_scale", "C")], seconds=1200),
    # Cool at 19 °C from 20.3 °C.
    "cool-band": dict(room=dict(temp=20.3, ambient=21, heat_rate=30, cool_rate=30, room_tau=1),
                      steps=[(0, "temperature_scale", "C"), (1, "mode", "cool"), (6, "target", "19")], seconds=1800),
}


def backplate(room, log):
    """nest-qemu's backplate, restarted with this room, logging frames."""
    subprocess.run(["pkill", "-f", "ui/backplate.py"], check=False)
    time.sleep(1)
    flash = ROOT / ".work/client-qemu/nand-state.bin.backplate"
    cmd = [sys.executable, "-u", str(QEMU / "ui/backplate.py"), "--serial", "tcp:127.0.0.1:8105",
           "--flash", str(flash), "--debug", "--temp", str(room["temp"]), "--ambient", str(room["ambient"]),
           "--heat-rate", str(room["heat_rate"]), "--cool-rate", str(room["cool_rate"]), "--room-tau", str(room["room_tau"])]
    out = open(log, "w")
    return subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, start_new_session=True), out


def mqtt(topic, value):
    subprocess.run(["mosquitto_pub", "-p", PORT, "-t", f"nestlocal/{SERIAL}/set/{topic}", "-m", str(value)], check=True)


MARK = "-- scenario starts\n"


def events(log):
    """The switches in a backplate log after the scenario's mark, each with
    the room last reported."""
    out, room, started = [], None, False
    for line in open(log):
        if line == MARK:
            started = True
        m = re.match(r"(<-|->) type=0x([0-9a-f]+) len=\d+(?: crc=\w+)? ?(\S*)", line)
        if not m:
            continue
        d, k, p = m.groups()
        if d == "->" and k == "0002" and len(p) >= 8:
            room = struct.unpack("<h", bytes.fromhex(p[:4]))[0] / 100
        if started and d == "<-" and k == "0082":
            out.append({"room": room, "circuit": int(p[:2], 16), "on": p[2:4] == "01"})
    return out


def run(name, path):
    s = SCENARIOS[name]
    log = Path(path).with_suffix(".log")
    mqtt("mode", "off")
    proc, out = backplate(s["room"], log)
    try:
        while "-> type=0x0002" not in log.read_text():
            time.sleep(0.5)
        time.sleep(2)
        out.write(MARK)
        out.flush()
        start = time.time()
        for at, topic, value in s["steps"]:
            time.sleep(max(0, start + at - time.time()))
            mqtt(topic, value)
        time.sleep(max(0, start + s["seconds"] - time.time()))
    finally:
        os.killpg(proc.pid, signal.SIGTERM)
        out.close()
    json.dump({"scenario": name, "switches": events(log)}, open(path, "w"), indent=1)
    print(f"{path}: {len(events(log))} switch frames")


def diff(a, b):
    sa, sb = json.load(open(a))["switches"], json.load(open(b))["switches"]
    same = True
    for i in range(max(len(sa), len(sb))):
        x = sa[i] if i < len(sa) else None
        y = sb[i] if i < len(sb) else None
        key = lambda e: e and (e["circuit"], e["on"])
        mark = "  " if key(x) == key(y) and x and y and abs(x["room"] - y["room"]) <= 0.011 else "!!"
        same &= mark == "  "
        print(f"{mark} {x} | {y}")
    sys.exit(0 if same else 1)


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "run":
        run(sys.argv[2], sys.argv[3])
    elif len(sys.argv) == 4 and sys.argv[1] == "diff":
        diff(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)
