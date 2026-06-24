#!/usr/bin/env python3
"""Host-side driver for the hardware-gate display-backed-VM capture.

Codifies the validated flow against a running qemu VM (QMP socket):
  1. wait for the Anaconda summary, drive Installation Destination -> Begin
  2. wait for the bootc install + first-boot GDM-autologin desktop
  3. dismiss the onboarding (Escape, then "Private - keep this computer offline")
  4. launch the in-session orchestrator via GNOME Alt+F2 (curl -o + bash; no sshd)
  5. screendump each surface to OUTDIR/<shot>.png on its HTTP /ready/<shot> signal
     until ORCH_ALLDONE

Env: GOS_QMP (qmp.sock), GOS_HTTPLOG (httpd.log), GOS_OUTDIR, GOS_PORT.
Note: per-shot window-focus timing lives in in-session-orchestrator.sh; tune the
settle there if surfaces capture as duplicates (md5-identical). KVM (CI) makes
the VM fast enough that the same timings that work under hvf hold.
"""
import json, os, socket, subprocess, time

QMP = os.environ["GOS_QMP"]; HTTPLOG = os.environ["GOS_HTTPLOG"]
OUTDIR = os.environ["GOS_OUTDIR"]; PORT = os.environ.get("GOS_PORT", "8099")

CMAP = {c: (c, False) for c in "abcdefghijklmnopqrstuvwxyz0123456789"}
CMAP.update({" ": ("spc", False), "-": ("minus", False), ".": ("dot", False),
             "/": ("slash", False), ":": ("semicolon", True)})

def _conn():
    for _ in range(120):
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect(QMP)
            f = s.makefile("rw"); f.readline()
            f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
            return s, f
        except (FileNotFoundError, ConnectionRefusedError):
            time.sleep(1)
    raise SystemExit("QMP never came up")

S, F = _conn()
def cmd(ex, **a):
    m = {"execute": ex}
    if a: m["arguments"] = a
    F.write(json.dumps(m) + "\n"); F.flush()
    while True:
        o = json.loads(F.readline())
        if "event" not in o: return o
def key(k): cmd("send-key", keys=[{"type": "qcode", "data": x} for x in k.split("+")])
def typ(s):
    for ch in s:
        if ch in CMAP:
            q, sh = CMAP[ch]
            cmd("send-key", keys=([{"type": "qcode", "data": "shift"}] if sh else []) + [{"type": "qcode", "data": q}])
            time.sleep(0.03)
def click(xf, yf):
    cmd("input-send-event", events=[{"type": "abs", "data": {"axis": "x", "value": int(xf*32767)}},
                                     {"type": "abs", "data": {"axis": "y", "value": int(yf*32767)}}])
    cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": True}}])
    cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": False}}])
def dump(p): cmd("screendump", filename=p)
def png(ppm, out): subprocess.run(["sips", "-s", "format", "png", ppm, "--out", out] if os.uname().sysname == "Darwin"
                                  else ["convert", ppm, out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
def fb_size():
    p = "/tmp/_fb.ppm"; dump(p)
    try: return os.path.getsize(p)
    except OSError: return 0

def wait_frame(lo, hi, timeout):
    t = time.time()
    while time.time() - t < timeout:
        sz = fb_size()
        if lo <= sz <= hi: return True
        time.sleep(10)
    return False

# 1. Anaconda summary -> destination -> begin
wait_frame(78000, 95000, 300)
click(0.55, 0.455); time.sleep(3); click(0.039, 0.06); time.sleep(3); click(0.937, 0.935)
# 2. wait for first-boot desktop (large frame)
wait_frame(150000, 10**9, 700)
# 3. dismiss onboarding
key("esc"); time.sleep(2); click(0.5, 0.627); time.sleep(3)
# 4. launch orchestrator via Alt+F2 (pipe-free)
key("alt+f2"); time.sleep(2); typ(f"curl -o /tmp/o 10.0.2.2:{PORT}/orchestrator.sh"); time.sleep(1); key("ret"); time.sleep(3)
key("alt+f2"); time.sleep(2); typ("bash /tmp/o"); time.sleep(1); key("ret")
# 5. capture on signals
os.makedirs(OUTDIR, exist_ok=True)
pos = os.path.getsize(HTTPLOG) if os.path.exists(HTTPLOG) else 0
seen = set(); t = time.time()
while time.time() - t < 600:
    with open(HTTPLOG, errors="ignore") as fh:
        fh.seek(pos); chunk = fh.read(); pos = fh.tell()
    for line in chunk.splitlines():
        if "/ready/" in line:
            name = line.split("/ready/")[1].split()[0].split("?")[0]
            if name == "ORCH_ALLDONE":
                print("ORCH_ALLDONE", flush=True); raise SystemExit(0)
            if name and name not in seen and name not in ("ORCH_START",):
                seen.add(name); ppm = f"{OUTDIR}/{name}.ppm"
                dump(ppm); png(ppm, f"{OUTDIR}/{name}.png")
                try: os.remove(ppm)
                except OSError: pass
                print(f"captured {name} ({len(seen)})", flush=True)
    time.sleep(0.3)
print(f"timeout; captured {len(seen)}", flush=True)
