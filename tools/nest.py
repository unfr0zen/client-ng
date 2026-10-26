#!/usr/bin/env python3
"""Develop client-ng on nest-qemu: boot the emulated gen 3, deploy
and run the client in place of nlclient, grab the screen, turn the wheel
and press the button. Standard library only.

    client-ng/tools/nest.py qemu start      # boot nest-qemu on its own NAND copy
    client-ng/tools/nest.py run             # deploy build/client-ng, swap out nlclient
    client-ng/tools/nest.py screenshot      # the face, upright, as PNG
    client-ng/tools/nest.py record DIR      # every frame for a few seconds, for animations
    client-ng/tools/nest.py compare A B     # two screenshots side by side, and where they differ
    client-ng/tools/nest.py wheel 3         # three detents clockwise
    client-ng/tools/nest.py click           # press and release the button
    client-ng/tools/nest.py watch           # push skin/menu edits as they are saved
    client-ng/tools/nest.py stop            # stop the client, nlclient comes back
    client-ng/tools/nest.py qemu stop

nest-qemu (https://github.com/unfr0zen/qemu) is found beside this repository
(../nest-qemu, or ../qemu as it clones) or at $NEST_QEMU.
The emulator boots a copy of nest-qemu's NAND state (.qemu/nand-state.bin,
or --nand) kept under .work/client-qemu/, so its own state is never
written. The guest's root login is root/nest.
"""
import argparse
import hashlib
import io
import json
import os
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tarfile
import time
import urllib.request
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE = ROOT / ".work" / "client-qemu"
SESSION = STATE / "session.json"
UI_INFO = STATE / "ui.json"
STAGE = ROOT / "build" / "client-ng"
REMOTE = "/media/scratch/client-ng"
PASSWORD = "nest"


def nest_qemu():
    beside = next((p for p in (ROOT.parent / "nest-qemu", ROOT.parent / "qemu") if (p / "run.sh").exists()), ROOT.parent / "nest-qemu")
    path = Path(os.environ.get("NEST_QEMU", beside))
    if not (path / "run.sh").exists():
        sys.exit(f"nest-qemu not found at {path}: set NEST_QEMU")
    return path


def session():
    try:
        return json.loads(SESSION.read_text())
    except (OSError, ValueError):
        return None


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def require_session():
    s = session()
    if not s or not alive(s["pid"]):
        sys.exit("nest-qemu is not running: client-ng/tools/nest.py qemu start")
    return s


# --- SSH ----------------------------------------------------------------

def askpass():
    """A helper that answers the guest's password prompt, so no expect or
    sshpass is needed."""
    path = STATE / "askpass"
    if not path.exists():
        STATE.mkdir(parents=True, exist_ok=True)
        path.write_text(f"#!/bin/sh\necho {PASSWORD}\n")
        path.chmod(0o700)
    return path


def ssh_command(port, command):
    return ["ssh", "-p", str(port),
            "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
            "-o", "PubkeyAcceptedAlgorithms=+ssh-rsa", "-o", "HostKeyAlgorithms=+ssh-rsa",
            "-o", "KexAlgorithms=+diffie-hellman-group14-sha1", "-o", "PreferredAuthentications=password",
            "-o", "ConnectTimeout=10", "root@127.0.0.1", command]


def ssh(command, stdin=None, capture=False, check=True):
    s = require_session()
    env = dict(os.environ, SSH_ASKPASS=str(askpass()), SSH_ASKPASS_REQUIRE="force", DISPLAY=os.environ.get("DISPLAY", ":0"))
    r = subprocess.run(ssh_command(s["ssh_port"], command), input=stdin, env=env, capture_output=capture,
                       stdin=None if stdin is not None else subprocess.DEVNULL)
    if check and r.returncode != 0:
        sys.exit(f"guest command failed ({r.returncode}): {command}" + (f"\n{r.stderr.decode(errors='replace')}" if capture else ""))
    return r


# --- nest-qemu ----------------------------------------------------------

def qemu_start(args):
    s = session()
    if s and alive(s["pid"]):
        print(f"already running: {s['ui_url']}  (ssh port {s['ssh_port']})")
        return
    qemu = nest_qemu()
    nand = Path(args.nand) if args.nand else qemu / ".qemu" / "nand-state.bin"
    state = STATE / "nand-state.bin"
    if not state.exists() and not nand.exists():
        sys.exit(f"no NAND to seed {state} from: {nand} is missing (--nand FILE)")
    STATE.mkdir(parents=True, exist_ok=True)
    UI_INFO.unlink(missing_ok=True)
    log = open(STATE / "qemu.log", "wb")
    env = dict(os.environ, QEMU_UI_INFO=str(UI_INFO), NAND=str(nand), NAND_STATE=str(state), UI_PORT=str(args.ui_port))
    if args.fresh:
        env["NAND_FRESH"] = "1"
    extra = (["--mqtt", args.mqtt] if args.mqtt else []) + ([] if args.offline else ["--cloud"])
    aps = ["-global", f"driver=bcm43341,property=aps,value={args.aps}"] if args.aps else []
    proc = subprocess.Popen(["./run.sh", "--ui", "--ssh", str(args.ssh_port), *extra, *aps], cwd=qemu, env=env,
                            stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    deadline = time.monotonic() + args.timeout
    while not UI_INFO.exists():
        if proc.poll() is not None or time.monotonic() > deadline:
            sys.exit(f"nest-qemu did not start; see {STATE / 'qemu.log'}")
        time.sleep(1)
    info = json.loads(UI_INFO.read_text())
    SESSION.write_text(json.dumps({"pid": proc.pid, "ssh_port": args.ssh_port, "ui_url": info["ui_url"]}, indent=2) + "\n")
    print(f"booting: {info['ui_url']}")
    while b"login:" not in (STATE / "qemu.log").read_bytes():
        if proc.poll() is not None or time.monotonic() > deadline:
            sys.exit(f"nest-qemu stopped while booting; see {STATE / 'qemu.log'}")
        time.sleep(2)
    while ssh("true", check=False).returncode != 0:
        if time.monotonic() > deadline:
            sys.exit("the guest's sshd did not answer")
        time.sleep(2)
    print(f"ready: {info['ui_url']}  (ssh port {args.ssh_port})")


def qemu_stop(_):
    s = session()
    if not s or not alive(s["pid"]):
        print("not running")
        return
    # Started by `qemu start`, the launcher leads its own process group;
    # QEMU itself is also found by the NAND copy it runs on.
    nand = str(STATE / "nand-state.bin")
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(s["pid"], sig)
        except OSError:
            subprocess.run(["pkill", "-" + sig.name[3:], "-f", nand], check=False)
            if alive(s["pid"]):
                os.kill(s["pid"], sig)
        for _ in range(30):
            if not alive(s["pid"]):
                break
            time.sleep(0.5)
        else:
            continue
        break
    SESSION.unlink(missing_ok=True)
    print("stopped")


def qemu_status(_):
    s = session()
    if s and alive(s["pid"]):
        print(f"running: {s['ui_url']}  (ssh port {s['ssh_port']}, pid {s['pid']})")
    else:
        print("not running")


# --- the client in the guest -------------------------------------------

def push(paths):
    """Stage files before replacing the guest's client paths."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for path in paths:
            tar.add(STAGE / path, arcname=path)
    names = " ".join(paths)
    ssh(f"""set -e
mkdir -p {REMOTE}
rm -rf {REMOTE}/.incoming
mkdir {REMOTE}/.incoming
tar -C {REMOTE}/.incoming -xf -
for name in {names}; do
    rm -rf {REMOTE}/.old-$name
    if [ -e {REMOTE}/$name ]; then mv {REMOTE}/$name {REMOTE}/.old-$name; fi
    mv {REMOTE}/.incoming/$name {REMOTE}/$name
    rm -rf {REMOTE}/.old-$name
done
rmdir {REMOTE}/.incoming
""", stdin=buffer.getvalue())


def deploy(_=None):
    if not (STAGE / "client-ng").exists():
        sys.exit("nothing to deploy: client-ng/build.sh")
    want = hashlib.md5((STAGE / "client-ng").read_bytes()).hexdigest()
    for _attempt in range(3):
        push(["client-ng", "skins", "config"])
        got = ssh(f"md5sum {REMOTE}/client-ng", capture=True, check=False).stdout.decode().split()[:1]
        if got == [want]:
            print(f"deployed {STAGE} to {REMOTE}")
            return
    sys.exit(f"deploy failed: {REMOTE}/client-ng does not match build/client-ng")


def run(args):
    if not args.no_deploy:
        deploy()
    client_args = args.client_args[1:] if args.client_args[:1] == ["--"] else args.client_args
    extra = " ".join(client_args)
    ssh(f"""
monit unmonitor nlclient >/dev/null 2>&1
for p in /proc/[0-9]*; do case "$(readlink $p/exe 2>/dev/null)" in */client-ng*) kill ${{p#/proc/}};; esac; done
sleep 1
for i in 1 2 3 4 5 6 7 8 9 10; do
    pid=$(pidof nlclient) || break
    [ $i -lt 6 ] && kill $pid || kill -9 $pid
    sleep 0.5
done
cd {REMOTE} && setsid ./client-ng {extra} > client.log 2>&1 < /dev/null &
sleep 1
pidof client-ng >/dev/null || {{ cat {REMOTE}/client.log; exit 1; }}
""")
    print("running in place of nlclient; logs: client-ng/tools/nest.py logs")


def stop(_):
    ssh('''for p in /proc/[0-9]*; do case "$(readlink $p/exe 2>/dev/null)" in */client-ng*) kill ${p#/proc/};; esac; done; monit monitor nlclient >/dev/null 2>&1; true''')
    print("stopped; monit brings nlclient back within a few seconds")


def logs(args):
    ssh(f"tail -n {args.lines} {'-f ' if args.follow else ''}{REMOTE}/client.log")


def watch(args):
    """Pushes skin and menu changes as they are saved; the client reloads them."""
    sources = [ROOT / "client-ng" / "skins", ROOT / "client-ng" / "config"]

    def snapshot():
        return {p: p.stat().st_mtime for src in sources for p in src.rglob("*") if p.is_file()}

    seen = snapshot()
    print("watching client-ng/skins and client-ng/config (Ctrl-C to stop)")
    while True:
        time.sleep(args.interval)
        now = snapshot()
        if now != seen:
            changed = sorted(str(p.relative_to(ROOT / "client-ng")) for p in now if seen.get(p) != now[p])
            seen = now
            subprocess.run(["cp", "-R", str(ROOT / "client-ng" / "skins"), str(STAGE)], check=True)
            subprocess.run(["cp", str(ROOT / "client-ng" / "config" / "menus.lua"), str(STAGE / "config")], check=True)
            subprocess.run(["cp", "-R", str(ROOT / "client-ng" / "config" / "plugins"), str(STAGE / "config")], check=True)
            subprocess.run(["cp", "-R", str(ROOT / "client-ng" / "config" / "locales"), str(STAGE / "config")], check=True)
            push(["skins", "config"])
            print("pushed " + ", ".join(changed))


# --- screen and input ---------------------------------------------------

def hmp(command):
    info = json.loads(UI_INFO.read_text())
    with socket.create_connection((info["hmp_host"], info["hmp_port"]), timeout=10) as s:
        s.settimeout(0.3)
        try:
            s.recv(65536)
        except socket.timeout:
            pass
        s.sendall(command.encode() + b"\n")
        time.sleep(0.5)


def read_ppm(data):
    fields, i = [], 0
    while len(fields) < 4:
        while data[i:i + 1].isspace():
            i += 1
        if data[i:i + 1] == b"#":
            i = data.index(b"\n", i)
            continue
        start = i
        while not data[i:i + 1].isspace():
            i += 1
        fields.append(data[start:i])
    if fields[0] != b"P6" or int(fields[3]) != 255:
        raise ValueError("not an 8-bit P6 PPM")
    width, height = int(fields[1]), int(fields[2])
    return width, height, data[i + 1:i + 1 + width * height * 3]


def face(width, height, pixels, raw):
    """The face upright: as client-ng places it (client-ng/src/display.rs)."""
    if raw:
        return width, height, pixels
    if (width, height) == (480, 640):
        size, ox, oy, turns = 480, 0, 0, 1
    else:
        size = min(width, height)
        ox, oy, turns = (width - size) // 2, (height - size) // 2, 0
    rows = [pixels[((oy + y) * width + ox) * 3:((oy + y) * width + ox + size) * 3] for y in range(size)]
    for _ in range(turns):
        # One quarter turn counterclockwise: the panel's top becomes the right.
        rows = [b"".join(rows[y][x * 3:x * 3 + 3] for y in range(size)) for x in range(size - 1, -1, -1)]
    return size, size, b"".join(rows)


def png(width, height, pixels):
    def chunk(kind, payload):
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))
    raw = b"".join(b"\0" + pixels[y * width * 3:(y + 1) * width * 3] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def read_png(data):
    """8-bit RGB or RGBA PNG, not interlaced (as QEMU's and these tools'
    encoders write): width, height and RGB bytes."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    i, idat, width = 8, b"", None
    while i < len(data):
        length, kind = struct.unpack(">I4s", data[i:i + 8])
        payload = data[i + 8:i + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", payload)
            if depth != 8 or color not in (2, 6) or interlace:
                raise ValueError("only 8-bit RGB or RGBA PNGs, not interlaced")
            bpp = 3 if color == 2 else 4
        elif kind == b"IDAT":
            idat += payload
        i += 12 + length
    raw, stride = zlib.decompress(idat), width * bpp
    out, prev, pos = bytearray(), bytearray(stride), 0
    for _ in range(height):
        kind, line = raw[pos], bytearray(raw[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b, c = prev[x], prev[x - bpp] if x >= bpp else 0
            if kind == 1:
                line[x] = (line[x] + a) & 255
            elif kind == 2:
                line[x] = (line[x] + b) & 255
            elif kind == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif kind == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        out += line if bpp == 3 else b"".join(line[x:x + 3] for x in range(0, stride, 4))
        prev = line
    return width, height, bytes(out)


def record(args):
    """Saves each new frame nest-qemu's page receives, upright, for
    `seconds`: QEMU sends one whenever the panel changes, every 20 to 30 ms
    while the UI animates."""
    s = require_session()
    out = Path(args.dir)
    out.mkdir(parents=True, exist_ok=True)
    started, n = time.monotonic(), 0
    with urllib.request.urlopen(s["ui_url"] + "/screen", timeout=args.seconds + 5) as stream:
        while time.monotonic() - started < args.seconds:
            line = stream.readline()
            if not line.startswith(b"data: "):
                continue
            try:
                frame = read_png(__import__("base64").b64decode(line[6:].strip()))
            except (ValueError, zlib.error, struct.error):
                continue
            ms = int((time.monotonic() - started) * 1000)
            (out / f"{n:03d}-{ms:05d}ms.png").write_bytes(png(*face(*frame, args.raw)))
            n += 1
    print(f"{n} frames in {out}")


def compare(args):
    """Puts two screenshots side by side with a third panel marking the
    pixels that differ, and prints their share."""
    (wa, ha, a), (wb, hb, b) = (read_png(Path(f).read_bytes()) for f in (args.a, args.b))
    if (wa, ha) != (wb, hb):
        sys.exit(f"sizes differ: {wa}x{ha} and {wb}x{hb}")
    diff, count = bytearray(), 0
    for i in range(0, len(a), 3):
        far = max(abs(a[i + k] - b[i + k]) for k in range(3)) > args.threshold
        count += far
        diff += b"\xff\0\0" if far else b"\0\0\0"
    rows = [a[y * wa * 3:(y + 1) * wa * 3] + b[y * wa * 3:(y + 1) * wa * 3] + bytes(diff[y * wa * 3:(y + 1) * wa * 3])
            for y in range(ha)]
    out = Path(args.output) if args.output else STATE / "compare.png"
    out.write_bytes(png(3 * wa, ha, b"".join(rows)))
    print(f"{100 * count / (wa * ha):.1f}% of pixels differ by more than {args.threshold}: {out}")


def screenshot(args):
    require_session()
    info = json.loads(UI_INFO.read_text())
    shot = Path(info["screendump"])
    shot.unlink(missing_ok=True)
    hmp(f"screendump {shot}")
    for _ in range(20):
        if shot.exists() and shot.stat().st_size > 0:
            break
        time.sleep(0.1)
    image = png(*face(*read_ppm(shot.read_bytes()), args.raw))
    out = Path(args.output) if args.output else STATE / "shots" / time.strftime("%Y%m%d-%H%M%S.png")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(image)
    print(out)


def post(path, body=None):
    s = require_session()
    data = json.dumps(body).encode() if body is not None else b""
    request = urllib.request.Request(s["ui_url"] + path, data=data, method="POST",
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=10) as r:
        return r.read()


# The unit's shared files the desktop build reads in place of
# /nestlabs/share: its typefaces, pictures and strings.
UNIT_SHARE = ("fonts", "images", "client/locales")


def unit_files(args):
    """Copies the guest's /nestlabs/share parts the client uses into
    .work/unit/share in this repository, for the desktop build (README.md,
    "On the desktop"). Over the console, as nest-qemu's MCP server moves
    files (md5-checked), so the guest's login need not be known. `--ui`
    takes them from a unit nest-qemu runs on its own (run.sh --ui)."""
    sys.path.insert(0, str(nest_qemu() / "ui"))
    import mcp_server
    mcp_server.base_url = args.ui or require_session()["ui_url"]
    archive = "/tmp/unit-share.tgz"
    status, out, _ = mcp_server.execute(f"cd /nestlabs/share && tar czf {archive} {' '.join(UNIT_SHARE)}", 120)
    if status != 0:
        sys.exit(f"tar in the guest failed: {out.strip()}")
    data = mcp_server.pull(archive, 600)
    mcp_server.execute(f"rm -f {archive}")
    dest = Path(__file__).resolve().parents[1] / ".work" / "unit" / "share"
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        tar.extractall(dest, filter="data")
    print(f"{len(data) // 1024} KiB of the unit's {', '.join(UNIT_SHARE)} in {dest}")


def wheel(args):
    for _ in range(abs(args.detents)):
        post("/wheel/turn", {"delta": 1 if args.detents > 0 else -1})
        time.sleep(args.pace)


def click(args):
    post("/button/press")
    time.sleep(args.hold / 1000)
    post("/button/release")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="command", required=True)

    q = sub.add_parser("qemu", help="boot, stop or show the emulated unit")
    qs = q.add_subparsers(dest="action", required=True)
    start = qs.add_parser("start")
    start.add_argument("--nand", help="seed the client's NAND copy from FILE (first boot, or with --fresh)")
    start.add_argument("--fresh", action="store_true", help="reseed the NAND copy")
    start.add_argument("--ui-port", type=int, default=8090)
    start.add_argument("--ssh-port", type=int, default=2290)
    start.add_argument("--timeout", type=int, default=420)
    start.add_argument("--mqtt", metavar="HOST[:PORT]", help="relay the guest's 10.0.2.100:1883 to this broker, for nestlocal")
    start.add_argument("--offline", action="store_true",
                       help="no internet (nest-qemu's default); online the unit's stock software may sign in to Nest")
    start.add_argument("--aps", metavar="SSID:CH:RSSI:PASS;...",
                       help="more access points beside QEMU's open one (bcm43341's aps; '!' before an SSID hides it)")
    start.set_defaults(func=qemu_start)
    qs.add_parser("stop").set_defaults(func=qemu_stop)
    qs.add_parser("status").set_defaults(func=qemu_status)

    sub.add_parser("deploy", help="copy build/client-ng into the guest").set_defaults(func=deploy)
    r = sub.add_parser("run", help="deploy and run the client in place of nlclient")
    r.add_argument("--no-deploy", action="store_true")
    r.add_argument("client_args", nargs=argparse.REMAINDER, help="passed to client-ng")
    r.set_defaults(func=run)
    sub.add_parser("stop", help="stop the client and give the screen back to nlclient").set_defaults(func=stop)
    lg = sub.add_parser("logs", help="the client's log")
    lg.add_argument("-f", "--follow", action="store_true")
    lg.add_argument("-n", "--lines", type=int, default=50)
    lg.set_defaults(func=logs)
    w = sub.add_parser("watch", help="push skin and menu edits as they are saved")
    w.add_argument("--interval", type=float, default=0.5)
    w.set_defaults(func=watch)
    sc = sub.add_parser("screenshot", help="save the face as PNG, upright")
    sc.add_argument("-o", "--output")
    sc.add_argument("--raw", action="store_true", help="the whole framebuffer, as the panel scans it")
    sc.set_defaults(func=screenshot)
    rc = sub.add_parser("record", help="save every frame for a while, upright")
    rc.add_argument("dir")
    rc.add_argument("--seconds", type=float, default=3)
    rc.add_argument("--raw", action="store_true", help="the whole framebuffer, as the panel scans it")
    rc.set_defaults(func=record)
    cp = sub.add_parser("compare", help="two screenshots side by side, and where they differ")
    cp.add_argument("a")
    cp.add_argument("b")
    cp.add_argument("-o", "--output")
    cp.add_argument("--threshold", type=int, default=48, help="largest channel difference counted as equal")
    cp.set_defaults(func=compare)
    wh = sub.add_parser("wheel", help="turn the wheel")
    wh.add_argument("detents", type=int, help="positive is clockwise")
    wh.add_argument("--pace", type=float, default=0.05,
                    help="seconds between detents (stock nlclient needs quick turns: 6 at 0.02 is one menu step)")
    wh.set_defaults(func=wheel)
    for name, hold in (("click", 120), ("hold", 2000)):
        c = sub.add_parser(name, help=f"press the button for {hold} ms")
        c.add_argument("--hold", type=int, default=hold, help="milliseconds")
        c.set_defaults(func=click)
    sub.add_parser("press").set_defaults(func=lambda _: post("/button/press"))
    sub.add_parser("release").set_defaults(func=lambda _: post("/button/release"))
    uf = sub.add_parser("unit-files", help="copy the guest's fonts, images and strings for the desktop build")
    uf.add_argument("--ui", metavar="URL", help="a unit's page other than this tool's (run.sh --ui's, say)")
    uf.set_defaults(func=unit_files)
    sh = sub.add_parser("ssh", help="run a command in the guest")
    sh.add_argument("cmd", nargs=argparse.REMAINDER)
    sh.set_defaults(func=lambda a: sys.exit(ssh(" ".join(a.cmd) or "sh", check=False).returncode))

    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
