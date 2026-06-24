#!/usr/bin/env python3
"""QMP driver: screendump, sendkey, abs-mouse click, type-string, serial-watch capture loop."""
import json, socket, sys, time, os, subprocess

SOCK = "/tmp/gos-hwgate-aarch64/qmp.sock"
SERIAL = "/tmp/gos-hwgate-aarch64/serial.log"
OUTDIR = "/tmp/gos-hwgate-aarch64/shots"

# char -> (qcode, shift?)
CMAP = {}
for c in "abcdefghijklmnopqrstuvwxyz": CMAP[c] = (c, False)
for c in "0123456789": CMAP[c] = (c, False)
CMAP.update({
    " ": ("spc", False), "-": ("minus", False), ".": ("dot", False),
    "/": ("slash", False), "_": ("minus", True), ":": ("semicolon", True),
    '"': ("apostrophe", True), "|": ("backslash", True), "=": ("equal", False),
    "'": ("apostrophe", False), ";": ("semicolon", False), ">": ("dot", True),
    "<": ("comma", True), ",": ("comma", False),
})

def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    f = s.makefile("rw")
    f.readline()
    f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
    return s, f

def cmd(f, execute, **args):
    msg = {"execute": execute}
    if args: msg["arguments"] = args
    f.write(json.dumps(msg) + "\n"); f.flush()
    while True:
        line = f.readline()
        if not line: return None
        obj = json.loads(line)
        if "event" in obj: continue
        return obj

def sendkey(f, keys):
    cmd(f, "send-key", keys=[{"type": "qcode", "data": k} for k in keys.split("+")])

def type_string(f, s):
    for ch in s:
        if ch not in CMAP:
            continue
        q, shift = CMAP[ch]
        keys = [{"type": "qcode", "data": "shift"}, {"type": "qcode", "data": q}] if shift else [{"type": "qcode", "data": q}]
        cmd(f, "send-key", keys=keys)
        time.sleep(0.03)

def click(f, xf, yf):
    ax = int(xf * 32767); ay = int(yf * 32767)
    cmd(f, "input-send-event", events=[
        {"type": "abs", "data": {"axis": "x", "value": ax}},
        {"type": "abs", "data": {"axis": "y", "value": ay}}])
    cmd(f, "input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": True}}])
    cmd(f, "input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": False}}])

def dump(f, path):
    cmd(f, "screendump", filename=path)

def topng(ppm, png):
    subprocess.run(["sips", "-s", "format", "png", ppm, "--out", png],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

HTTPLOG = "/tmp/gos-hwgate-aarch64/httpd.log"

def watch(f, timeout=360):
    """Tail httpd.log; on GET /ready/<name> screendump to shots/<name>.png."""
    os.makedirs(OUTDIR, exist_ok=True)
    # seek to END so we only act on NEW signals (ignore backlog from prior runs)
    try: pos = os.path.getsize(HTTPLOG)
    except OSError: pos = 0
    start = time.time()
    seen = set()
    while time.time() - start < timeout:
        try:
            with open(HTTPLOG, "r", errors="ignore") as sf:
                sf.seek(pos); chunk = sf.read(); pos = sf.tell()
        except FileNotFoundError:
            time.sleep(0.5); continue
        for line in chunk.splitlines():
            if "/ready/" in line:
                name = line.split("/ready/")[1].split()[0].split("?")[0]
                if name == "ORCH_ALLDONE":
                    print("ORCH_ALLDONE", flush=True); return
                if name and name not in seen and name != "ORCH_START":
                    seen.add(name)
                    ppm = f"{OUTDIR}/{name}.ppm"; png = f"{OUTDIR}/{name}.png"
                    dump(f, ppm); topng(ppm, png)
                    try: os.remove(ppm)
                    except OSError: pass
                    print(f"CAPTURED {name} ({len(seen)})", flush=True)
        time.sleep(0.3)
    print(f"watch timeout; captured {len(seen)}: {sorted(seen)}", flush=True)

def main():
    a = sys.argv[1]
    s, f = connect()
    try:
        if a == "status": print(json.dumps(cmd(f, "query-status")))
        elif a == "dump": dump(f, sys.argv[2]); print("dumped")
        elif a == "key": sendkey(f, sys.argv[2]); print("key")
        elif a == "type": type_string(f, sys.argv[2]); print("typed")
        elif a == "click": click(f, float(sys.argv[2]), float(sys.argv[3])); print("clicked")
        elif a == "watch": watch(f, int(sys.argv[2]) if len(sys.argv) > 2 else 300)
    finally:
        f.close(); s.close()

if __name__ == "__main__":
    main()
