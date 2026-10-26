#!/usr/bin/env python3
"""
An MCP server (stdio) for the desktop build of client-ng: build and launch
it in its window, look at the screen (a frame, a burst, a recording),
press the button, turn the wheel, play input sequences, change the
simulated backplate's room and wiring, and read the client's log. The
tools that the desktop shares with nest-qemu have nest-qemu's MCP server's
names and arguments (../qemu/ui/mcp_server.py).

Everything goes through the client's control server (--control, by
default 127.0.0.1:8095; README.md, "On the desktop"). Standard library
only; recordings are encoded with ffmpeg when it is on PATH and kept as
PNG frames when it is not.

    desktop-mcp.py [--url http://127.0.0.1:8095]

.mcp.json at the repository's root registers it for Claude Code.
"""
import argparse
import base64
import bisect
import datetime
import http.client
import json
import os
import re
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORK = os.path.join(REPO, ".work", "desktop")
LOG = os.path.join(WORK, "client.log")
RECORDINGS = os.path.join(WORK, "recordings")
# Its own target folder: a plain cargo build or test does not replace the
# desktop binary under it.
TARGET = os.path.join(REPO, "target", "desktop")
PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")

url = "http://127.0.0.1:8095"
child = None                # the client launch started, while it runs


class ToolError(Exception):
    """A failure the agent should read, returned as an error result."""


# ---------------------------------------------------------------- HTTP

def request(method, path, body=None, timeout=10):
    """One request to the client: returns the body's bytes."""
    data = None
    if body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
    req = urllib.request.Request(url + path, data=data, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read()
    except urllib.error.HTTPError as e:
        raise ToolError(f"{method} {path}: HTTP {e.code} {e.read().decode(errors='replace').strip()}") from None
    except (urllib.error.URLError, OSError) as e:
        raise ToolError(f"no client at {url} ({e}): start it with launch") from None


def post(path, body=None):
    return request("POST", path, body if body is not None else b"")


def up():
    try:
        request("GET", "/state", timeout=2)
        return True
    except ToolError:
        return False


# ---------------------------------------------------------------- frames

def grab():
    return request("GET", "/screen.png")


def png_size(png):
    return struct.unpack(">II", png[16:24])


def image(png):
    return {"type": "image", "data": base64.b64encode(png).decode("ascii"), "mimeType": "image/png"}


def text(s):
    return {"type": "text", "text": s if isinstance(s, str) else json.dumps(s, indent=1)}


def save(png, path):
    path = os.path.abspath(os.path.expanduser(path))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)
    return path


def settle(quiet_ms, timeout_s):
    """Wait until the screen has not changed for quiet_ms; returns the
    frame and whether it settled before timeout_s."""
    frame = grab()
    deadline = time.monotonic() + timeout_s
    stable_since = time.monotonic()
    while time.monotonic() < deadline:
        if (time.monotonic() - stable_since) * 1000 >= quiet_ms:
            return frame, True
        time.sleep(0.05)
        now = grab()
        if now != frame:
            frame, stable_since = now, time.monotonic()
    return frame, False


class Recorder:
    """Follows the client's /screen stream (a frame each time it shows
    one) into PNG files with their arrival times, then encodes them into a
    constant-rate video, holding each frame until the next."""

    def __init__(self, path, fps, max_s):
        self.path, self.fps, self.max_s = path, fps, max_s
        self.dir = tempfile.mkdtemp(prefix="frames-", dir=os.path.dirname(path))
        self.frames, self.times = [], []
        self.error = None
        self.started = time.monotonic()
        self.stopped = None
        self.conn = None
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        u = urllib.parse.urlsplit(url)
        try:
            self.conn = http.client.HTTPConnection(u.hostname, u.port or 80, timeout=30)
            self.conn.request("GET", "/screen")
            resp = self.conn.getresponse()
            data = []
            while self.stopped is None:
                if time.monotonic() - self.started > self.max_s:
                    self.stopped = time.monotonic()
                    break
                line = resp.readline()
                if not line:
                    break
                line = line.rstrip(b"\r\n")
                if line.startswith(b"data:"):
                    data.append(line[5:].strip())
                elif not line and data:
                    self.add(base64.b64decode(b"".join(data)))
                    data = []
        except (OSError, http.client.HTTPException, ValueError) as e:
            if self.stopped is None:
                self.error = str(e)

    def add(self, png):
        name = os.path.join(self.dir, f"{len(self.frames):06d}.png")
        with open(name, "wb") as f:
            f.write(png)
        t = time.monotonic() - self.started
        self.frames.append((t, name))
        self.times.append(t)

    def stop(self):
        if self.stopped is None:
            self.stopped = time.monotonic()
        if self.conn and self.conn.sock:
            try:
                self.conn.sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.thread.join(5)
        return self.stopped - self.started

    def shown_at(self, t):
        """PNG bytes of the frame on screen t s in."""
        i = max(bisect.bisect_right(self.times, t) - 1, 0)
        with open(self.frames[i][1], "rb") as f:
            return f.read()

    def encode(self, duration):
        """The video at self.path, or None (no ffmpeg, or it failed)."""
        ffmpeg = shutil.which("ffmpeg")
        if not ffmpeg or not self.frames:
            return None
        p = subprocess.Popen([ffmpeg, "-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", str(self.fps),
                              "-i", "-", "-vf", "format=yuv420p", "-movflags", "+faststart", self.path],
                             stdin=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for tick in range(max(round(duration * self.fps), 1)):
                p.stdin.write(self.shown_at(tick / self.fps))
            p.stdin.close()
        except BrokenPipeError:
            pass
        err = p.stderr.read().decode(errors="replace")
        if p.wait():
            self.error = "ffmpeg: " + err.strip()[-500:]
            return None
        return self.path

    def keyframes(self, n, duration):
        """What the screen showed at n times spread over the recording; a
        frame shown at several of them comes once."""
        out = []
        for i in range(n if self.frames else 0):
            t = duration * i / max(n - 1, 1)
            png = self.shown_at(t)
            if not out or out[-1][1] != png:
                out.append((t, png))
        return out


recorder = None
recorder_lock = threading.Lock()


def record_start(fps=30, path=None, max_s=300):
    global recorder
    with recorder_lock:
        if recorder:
            raise ToolError("a recording is already running: record_stop first")
        request("GET", "/state")
        if not path:
            path = os.path.join(RECORDINGS, datetime.datetime.now().strftime("%Y%m%d-%H%M%S") + ".mp4")
        path = os.path.abspath(os.path.expanduser(path))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        recorder = Recorder(path, fps, max_s)
        return path


def record_stop(keyframes=4, keep_frames=False):
    global recorder
    with recorder_lock:
        rec, recorder = recorder, None
    if not rec:
        raise ToolError("no recording is running")
    duration = rec.stop()
    video = rec.encode(duration)
    shots = rec.keyframes(keyframes, duration)
    info = {"duration_s": round(duration, 2), "frames": len(rec.frames), "video": video}
    if video and not keep_frames:
        shutil.rmtree(rec.dir, ignore_errors=True)
    else:
        info["frames_dir"] = rec.dir
    if rec.error:
        info["error"] = rec.error
    content = [text(info)]
    for t, png in shots:
        content += [text(f"t={t:.2f}s"), image(png)]
    return content


# ---------------------------------------------------------------- input

def press(hold_ms=100):
    post("/button/press")
    try:
        time.sleep(hold_ms / 1000)
    finally:
        post("/button/release")


def turn(detents=None, notches=None):
    want = {"delta": int(detents)} if detents is not None else {"notches": int(notches)}
    return json.loads(post("/wheel/turn", want)).get("position")


# ---------------------------------------------------------------- the process

def log_tail(lines=40, pattern=None):
    try:
        with open(LOG, errors="replace") as f:
            out = f.read().splitlines()
    except OSError:
        return f"(no log at {LOG}: the client was not started by launch)"
    if pattern:
        out = [l for l in out if re.search(pattern, l)]
    return "\n".join(out[-lines:])


def t_launch(a):
    global child
    if up():
        if not a.get("restart"):
            raise ToolError(f"a client already answers at {url}: stop it first, or pass restart")
        t_stop({})
    if a.get("build", True):
        cmd = ["cargo", "build", "--features", "desktop", "--target-dir", TARGET] + (["--release"] if a.get("release", True) else [])
        r = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True, timeout=1800)
        if r.returncode:
            raise ToolError("build failed:\n" + r.stderr[-6000:])
    binary = os.path.join(TARGET, "release" if a.get("release", True) else "debug", "client-ng")
    if not os.path.exists(binary):
        raise ToolError(f"no {binary}: launch with build true")
    os.makedirs(WORK, exist_ok=True)
    u = urllib.parse.urlsplit(url)
    # Stock's own pixels, 480 each way, unless asked otherwise.
    args = ["--settings", os.path.join(WORK, "settings.json")] + list(a.get("args") or [])
    if "--pixels" not in args:
        args += ["--pixels", "480"]
    if a.get("headless", True) and "--headless" not in args:
        args += ["--headless"]
    args += ["--control", f"{u.hostname}:{u.port or 80}"]
    log = open(LOG, "wb")
    child = subprocess.Popen([binary] + args, cwd=REPO, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                             start_new_session=True)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if child.poll() is not None:
            raise ToolError(f"the client exited ({child.returncode}):\n" + log_tail())
        try:
            request("GET", "/screen.png", timeout=2)
            break
        except ToolError:
            time.sleep(0.25)
    else:
        raise ToolError("no frame within 30 s:\n" + log_tail())
    return [text(f"running (pid {child.pid}) with {' '.join(args)}\n" + log_tail(20)), image(grab())]


def t_stop(a):
    global child
    if up():
        post("/quit")
    if child:
        try:
            child.wait(10)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        code, child = child.returncode, None
        return [text(f"stopped (exit {code})")]
    deadline = time.monotonic() + 10
    while up() and time.monotonic() < deadline:
        time.sleep(0.2)
    return [text("stopped" if not up() else "still answering after 10 s")]


def t_logs(a):
    return [text(log_tail(int(a.get("lines", 80)), a.get("grep")))]


# ---------------------------------------------------------------- tools

def t_status(a):
    out = {"url": url, "up": up(), "launched_pid": child.pid if child and child.poll() is None else None}
    if out["up"]:
        out.update(json.loads(request("GET", "/state")))
    out["log_tail"] = log_tail(10)
    return [text(out)]


def t_screenshot(a):
    if a.get("settle_ms"):
        png, settled = settle(a["settle_ms"], a.get("timeout_s", 10))
        notes = [] if settled else ["the screen did not settle before timeout_s"]
    else:
        png, notes = grab(), []
    notes.append("%dx%d" % png_size(png))
    if a.get("save_to"):
        notes.append("saved " + save(png, a["save_to"]))
    return [image(png), text(", ".join(notes))]


def t_capture_frames(a):
    count = min(max(int(a.get("count", 6)), 1), 32)
    interval = max(int(a.get("interval_ms", 500)), 0) / 1000
    out, last, t0 = [], None, time.monotonic()
    for i in range(count):
        if i:
            time.sleep(max(t0 + i * interval - time.monotonic(), 0))
        png = grab()
        t = time.monotonic() - t0
        if a.get("changes_only") and png == last:
            continue
        last = png
        label = f"frame {i} t={t:.2f}s"
        if a.get("save_dir"):
            label += " saved " + save(png, os.path.join(a["save_dir"], f"frame-{i:03d}.png"))
        out += [text(label), image(png)]
    return out


def t_press_button(a):
    press(int(a.get("hold_ms", 100)))
    return [text(f"pressed for {int(a.get('hold_ms', 100))} ms")]


def t_turn_wheel(a):
    if "detents" not in a and "notches" not in a:
        raise ToolError("give detents or notches")
    return [text(f"wheel at {turn(a.get('detents'), a.get('notches'))}")]


def t_input_sequence(a):
    out, log = [], []
    rec_path = record_start(a.get("fps", 30)) if a.get("record") else None
    try:
        for i, st in enumerate(a.get("steps") or []):
            if "press" in st:
                press(int(st["press"]))
                log.append(f"{i}: press {st['press']} ms")
            elif "button" in st:
                post("/button/press" if st["button"] == "down" else "/button/release")
                log.append(f"{i}: button {st['button']}")
            elif "turn" in st:
                log.append(f"{i}: turn {st['turn']} -> {turn(st['turn'])}")
            elif "notches" in st:
                log.append(f"{i}: notches {st['notches']} -> {turn(notches=st['notches'])}")
            elif "wait" in st:
                time.sleep(int(st["wait"]) / 1000)
                log.append(f"{i}: wait {st['wait']} ms")
            elif "screenshot" in st:
                out += [text(f"step {i}"), image(grab())]
                log.append(f"{i}: screenshot")
            else:
                raise ToolError(f"step {i}: unknown step {st}")
            if a.get("step_delay_ms"):
                time.sleep(int(a["step_delay_ms"]) / 1000)
        if a.get("screenshot_after", True):
            time.sleep(int(a.get("after_ms", 300)) / 1000)
            out += [text("after the sequence"), image(grab())]
    finally:
        if rec_path:
            out = record_stop(a.get("keyframes", 4)) + out
    return [text("\n".join(log))] + out


def t_record_start(a):
    path = record_start(int(a.get("fps", 30)), a.get("path"), int(a.get("max_s", 300)))
    return [text(f"recording to {path}: call record_stop to finish (stops on its own after {int(a.get('max_s', 300))} s)")]


def t_record_stop(a):
    return record_stop(int(a.get("keyframes", 4)), a.get("keep_frames", False))


def t_backplate(a):
    want = {k: v for k, v in a.items() if v is not None}
    return [text(json.loads(post("/backplate", want)))]


def t_cloud(a):
    post("/cloud", a["updates"])
    return [text("sent " + ", ".join(a["updates"]))]


def t_open_menu(a):
    want = {"path": a["path"]}
    if a.get("at") is not None:
        want["at"] = a["at"]
    post("/menu/open", want)
    return [text("opened " + " > ".join(a["path"]) + ("" if a.get("at") is None else " at %d" % a["at"]))]


def schema(props, required=()):
    return {"type": "object", "properties": props, "required": list(required)}


INT, NUM, STR, BOOL = ({"type": t} for t in ("integer", "number", "string", "boolean"))


def d(base, desc):
    return dict(base, description=desc)


TOOLS = {
    "launch": (t_launch, "Build the desktop client (cargo build --features desktop) and start it (headless unless asked for the window), its log in .work/desktop/client.log and its settings in .work/desktop/settings.json. Returns the first frame.",
               schema({"args": {"type": "array", "items": {"type": "string"},
                                "description": "client options, e.g. [\"--no-settings\"], [\"--simulate-backplate\"], [\"--display-idle\", \"600\"], [\"--fps\"]"},
                       "build": d(BOOL, "build first (default true)"),
                       "release": d(BOOL, "the release build, which holds 60 fps (default true; a debug build draws under 40)"),
                       "restart": d(BOOL, "stop a client already running first"),
                       "headless": d(BOOL, "no window: nothing shown, no focus taken (default true; false opens the round window)")})),
    "stop": (t_stop, "Stop the client (and with it backplate.py).", schema({})),
    "logs": (t_logs, "The client's log (what it prints: skin errors, HVAC decisions, the backplate link); launch's runs only.",
             schema({"lines": d(INT, "the last this many lines (default 80)"),
                     "grep": d(STR, "only lines matching this Python regex")})),
    "status": (t_status, "Is the client up, the wheel position, which backplate stands in, the log's last lines.", schema({})),
    "screenshot": (t_screenshot, "The screen now, 480x480 upright (the whole square; the window masks it to the round glass).",
                   schema({"settle_ms": d(INT, "first wait until the screen has not changed for this long (an animation ending)"),
                           "timeout_s": d(NUM, "longest to wait for settle_ms (default 10)"),
                           "save_to": d(STR, "also save the PNG to this path")})),
    "capture_frames": (t_capture_frames, "Several screenshots at a fixed interval, returned together: for animations and transitions.",
                       schema({"count": d(INT, "frames, 1..32 (default 6)"),
                               "interval_ms": d(INT, "between frames (default 500)"),
                               "changes_only": d(BOOL, "skip frames identical to the previous one"),
                               "save_dir": d(STR, "also save frame-NNN.png files here")})),
    "press_button": (t_press_button, "Press the button (the display face) and release it after hold_ms.",
                     schema({"hold_ms": d(INT, "how long to hold (default 100; a long press is a few seconds)")})),
    "turn_wheel": (t_turn_wheel, "Turn the ring, clockwise positive: detents as the unit's ring reports them, in one wheel event (two detents a dial step; the menu ring and Settings' carousel move by their pace, about eight a quick entry), "
                   "or notches as the window's mouse wheel turns it (a dial step, a menu entry, a carousel tile each); returns the position in detents.",
                   schema({"detents": INT, "notches": INT})),
    "input_sequence": (t_input_sequence,
                       "Play a list of inputs in order, optionally recorded, with screenshots where asked and one at the end. "
                       "Steps: {\"press\": ms}, {\"button\": \"down\"|\"up\"}, {\"turn\": detents}, {\"notches\": n}, {\"wait\": ms}, {\"screenshot\": true}.",
                       schema({"steps": {"type": "array", "items": {"type": "object"}},
                               "step_delay_ms": d(INT, "pause after every step (default 0)"),
                               "screenshot_after": d(BOOL, "screenshot after the last step (default true)"),
                               "after_ms": d(INT, "wait before that screenshot (default 300)"),
                               "record": d(BOOL, "record a video of the sequence (record_stop's result is included)"),
                               "fps": d(INT, "video frame rate when recording (default 30)"),
                               "keyframes": d(INT, "frames of the recording to return as images (default 4)")},
                              ["steps"])),
    "record_start": (t_record_start, "Start recording the screen to an MP4 (every frame the client shows). Other tools keep working meanwhile; record_stop finishes it.",
                     schema({"fps": d(INT, "output frame rate (default 30)"),
                             "path": d(STR, "output file (default .work/desktop/recordings/<time>.mp4)"),
                             "max_s": d(INT, "stop on its own after this long (default 300)")})),
    "record_stop": (t_record_stop, "Stop the recording and encode it (ffmpeg; without it the PNG frames are kept). Returns the path and a few frames as images.",
                    schema({"keyframes": d(INT, "frames spread over the recording to return as images (default 4)"),
                            "keep_frames": d(BOOL, "keep the PNG frames next to the video")})),
    "cloud": (t_cloud, "Stand in for nestlocal: buckets as it sends them, under shared.SERIAL and device.SERIAL (status gives the serial), "
              "e.g. {\"shared.SERIAL\": {\"target_temperature\": 22.5}}, and the weather, "
              "{\"weather\": {\"temp_c\": 31.1, \"humidity\": 30, \"icon\": \"clear-day\"}}.",
              schema({"updates": {"type": "object", "description": "{key: {field: value}}"}}, ["updates"])),
    "open_menu": (t_open_menu, "Open the menus straight onto a submenu, by its labels from the top menu as config/menus.lua names them, "
                  "e.g. [\"Settings\"] for Settings' tiles or [\"Settings\", \"Nest Sense\"] for that page, on its entry `at` "
                  "(from 0, counting only the entries shown) when given; no transition plays. A press then opens that entry.",
                  schema({"path": {"type": "array", "items": {"type": "string"}},
                          "at": d(INT, "the entry to select, from 0")}, ["path"])),
    "backplate": (t_backplate, "The simulated backplate's (nest-qemu's backplate.py) room and wiring, changed while running; with no arguments, its state "
                  "(also the circuits the client has switched on). The room drifts to ambient over room_tau and heats/cools at the rates while switched.",
                  schema({"temp": d(NUM, "the room now, degrees C"),
                          "rh": d(NUM, "relative humidity, %"),
                          "ambient": d(NUM, "where the room drifts with nothing on, degrees C"),
                          "heat_rate": d(NUM, "degrees C an hour while heating"),
                          "cool_rate": d(NUM, "degrees C an hour while cooling"),
                          "room_tau": d(NUM, "hours for the room to drift most of the way to ambient"),
                          "presence": d(BOOL, "report someone near (motion)"),
                          "mono": d({"type": "object"}, "firmware fields the MCU reports (version, description, user, host, date), as if flashed; it restarts"),
                          "bsl": d({"type": "object"}, "the bootloader's fields, likewise"),
                          "als": d(INT, "the ambient light's visible count"),
                          "wires": {"type": "array", "items": {"type": "string"},
                                    "description": "terminals with a wire, e.g. [\"Rh\", \"W1\", \"Y1\", \"G\", \"C\"]; the client sees the new wiring"}})),
}

INSTRUCTIONS = """\
Drives client-ng's desktop build: the thermostat UI in a window on this
host, with nest-qemu's backplate.py as its backplate. launch builds and
starts it (status says whether one runs); screenshot / capture_frames show
the screen; press_button, turn_wheel and input_sequence are the unit's
inputs; record_start/record_stop make a video; backplate sets the
simulated room and wiring; logs reads what the client printed. Skin and
menu edits in the checkout reload within a second, no relaunch needed. The
display sleeps after 40 s without input (launch with
["--display-idle", "600"] to keep it lit): a black screen means asleep,
and the first press only wakes it."""


# ---------------------------------------------------------------- protocol

out_lock = threading.Lock()


def reply(msg):
    with out_lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()


def call_tool(rid, name, args):
    try:
        if name not in TOOLS:
            raise ToolError(f"no tool {name}")
        result = {"content": TOOLS[name][0](args or {}), "isError": False}
    except ToolError as e:
        result = {"content": [text(str(e))], "isError": True}
    except KeyError as e:
        result = {"content": [text(f"missing argument {e}")], "isError": True}
    except Exception as e:                  # a bug here, not the agent's
        result = {"content": [text(f"{type(e).__name__}: {e}")], "isError": True}
    reply({"jsonrpc": "2.0", "id": rid, "result": result})


def handle(msg):
    method, rid = msg.get("method"), msg.get("id")
    if rid is None:
        return
    if method == "initialize":
        want = msg.get("params", {}).get("protocolVersion")
        reply({"jsonrpc": "2.0", "id": rid, "result": {
            "protocolVersion": want if want in PROTOCOLS else PROTOCOLS[0],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "client-ng-desktop", "version": "1.0"},
            "instructions": INSTRUCTIONS}})
    elif method == "ping":
        reply({"jsonrpc": "2.0", "id": rid, "result": {}})
    elif method == "tools/list":
        reply({"jsonrpc": "2.0", "id": rid, "result": {"tools": [
            {"name": n, "description": desc, "inputSchema": sch} for n, (_, desc, sch) in TOOLS.items()]}})
    elif method == "tools/call":
        p = msg.get("params", {})
        # Its own thread: a long call (a build, a recording) must not hold
        # up pings or the next call.
        threading.Thread(target=call_tool, args=(rid, p.get("name"), p.get("arguments")), daemon=True).start()
    else:
        reply({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": f"no method {method}"}})


def main():
    global url
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--url", default=os.environ.get("CLIENT_NG_URL", url),
                    help="the client's control server (default %(default)s)")
    url = ap.parse_args().url.rstrip("/")
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            reply({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}})
            continue
        handle(msg)
    if recorder:
        record_stop(0)
    if child and child.poll() is None:
        t_stop({})


if __name__ == "__main__":
    main()
