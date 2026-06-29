#!/usr/bin/env python3
"""Host-side driver for the hardware-gate display-backed-VM capture.

Codifies the validated flow against a running qemu VM (QMP socket):
  1. wait for Anaconda to settle, drive Installation Destination -> Begin
  2. require the kickstart %post marker, then wait for first-boot desktop settle
  3. dismiss the onboarding (Escape, then "Private - keep this computer offline")
  4. launch the in-session orchestrator via GNOME Alt+F2 (curl -o + bash; no sshd)
  5. screendump each surface to OUTDIR/<shot>.png on its HTTP /ready/<shot> signal
     until ORCH_ALLDONE

Env: GOS_QMP (qmp.sock), GOS_SERIALLOG (serial.log), GOS_HTTPLOG
(httpd.log), GOS_OUTDIR, GOS_PORT.
Note: per-shot window-focus timing lives in in-session-orchestrator.sh; tune the
settle there if surfaces capture as duplicates (md5-identical). KVM (CI) makes
the VM fast enough that the same timings that work under hvf hold.
"""
import hashlib, json, os, socket, subprocess, time
from urllib.parse import parse_qs, urlparse

QMP = os.environ["GOS_QMP"]; HTTPLOG = os.environ["GOS_HTTPLOG"]
SERIALLOG = os.environ.get("GOS_SERIALLOG", os.path.join(os.path.dirname(QMP), "serial.log"))
OUTDIR = os.environ["GOS_OUTDIR"]; PORT = os.environ.get("GOS_PORT", "8099")
REQUIRED_PROOFS = (
    "firewall-live-toggle",
    "text-shortcuts-session-enable",
    "text-shortcuts-live-keystroke",
    "text-shortcuts-candidate-metadata",
    "text-shortcuts-overlay-intent",
    "text-shortcuts-candidate-bubble-frame",
    "text-shortcuts-candidate-bubble-layout",
    "text-shortcuts-candidate-bubble-render-intent",
    "text-shortcuts-candidate-bubble-render",
    "text-shortcuts-live-ibus-runtime-render",
    "keyboard-shortcuts-roundtrip",
    "input-sources-roundtrip",
    "focus-arm-roundtrip",
    "app-privacy-revoke",
    "preview-open-render",
)
PROOF_FILENAMES = {
    "firewall-live-toggle": "firewall-live-toggle-proof.json",
    "text-shortcuts-session-enable": "text-shortcuts-session-enable-proof.json",
    "text-shortcuts-live-keystroke": "text-shortcuts-live-keystroke-proof.json",
    "text-shortcuts-candidate-metadata": "text-shortcuts-candidate-metadata-proof.json",
    "text-shortcuts-overlay-intent": "text-shortcuts-overlay-intent-proof.json",
    "text-shortcuts-candidate-bubble-frame": "text-shortcuts-candidate-bubble-frame-proof.json",
    "text-shortcuts-candidate-bubble-layout": "text-shortcuts-candidate-bubble-layout-proof.json",
    "text-shortcuts-candidate-bubble-render-intent": "text-shortcuts-candidate-bubble-render-intent-proof.json",
    "text-shortcuts-candidate-bubble-render": "text-shortcuts-candidate-bubble-render-proof.json",
    "text-shortcuts-live-ibus-runtime-render": "text-shortcuts-live-ibus-runtime-render-proof.json",
    "keyboard-shortcuts-roundtrip": "keyboard-shortcuts-roundtrip-proof.json",
    "input-sources-roundtrip": "input-sources-roundtrip-proof.json",
    "focus-arm-roundtrip": "focus-arm-roundtrip-proof.json",
    "app-privacy-revoke": "app-privacy-revoke-proof.json",
    "preview-open-render": "preview-open-render-proof.json",
}

CMAP = {c: (c, False) for c in "abcdefghijklmnopqrstuvwxyz0123456789"}
CMAP.update({" ": ("spc", False), "-": ("minus", False), ".": ("dot", False),
             "/": ("slash", False), ":": ("semicolon", True)})

def _conn():
    last_error = "socket missing"
    for _ in range(120):
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect(QMP)
            f = s.makefile("rw"); f.readline()
            f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); f.flush(); f.readline()
            return s, f
        except OSError as err:
            last_error = repr(err)
            time.sleep(1)
    raise SystemExit(f"QMP never came up at {QMP}; last connection error: {last_error}")

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
def serial_text():
    try:
        with open(SERIALLOG, errors="ignore") as fh:
            return fh.read()
    except OSError:
        return ""

def wait_serial_contains(label, needle, timeout):
    t = time.time()
    last_tail = ""
    while time.time() - t < timeout:
        data = serial_text()
        if needle in data:
            print(f"{label}: observed serial marker {needle!r}", flush=True)
            return True
        last_tail = data[-500:]
        time.sleep(1)
    raise SystemExit(
        f"{label} did not appear in serial log within {timeout}s; "
        f"expected {needle!r}; serial_tail={last_tail!r}"
    )

def observe_serial_contains(label, needle, timeout):
    t = time.time()
    while time.time() - t < timeout:
        if needle in serial_text():
            print(f"{label}: observed serial marker {needle!r}", flush=True)
            return True
        time.sleep(1)
    print(
        f"{label}: serial marker {needle!r} not observed within {timeout}s; "
        "continuing to framebuffer stages",
        flush=True,
    )
    return False

def slug(label):
    value = "".join(ch.lower() if ch.isalnum() else "-" for ch in label).strip("-")
    return value or "stage"

def frame_sample(label, save_debug=False):
    p = f"/tmp/_fb-{slug(label)}.ppm"
    dump(p)
    try:
        with open(p, "rb") as fh:
            data = fh.read()
        sample = {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()[:16]}
        if save_debug:
            os.makedirs(OUTDIR, exist_ok=True)
            out = f"{OUTDIR}/_debug-{slug(label)}.png"
            png(p, out)
            print(f"{label}: debug framebuffer saved to {out}", flush=True)
        return sample
    except OSError as err:
        return {"size": 0, "sha256": f"error:{err}"}
    finally:
        try:
            os.remove(p)
        except OSError:
            pass

def wait_stage(label, seconds, sample_every=30):
    """Wait a bounded stage interval while recording diagnostic-only frames.

    QEMU PPM byte size is resolution-driven on CI, not a reliable UI state
    detector. These samples are intentionally diagnostic-only; real progress is
    proven by serial markers and in-session HTTP proof signals.
    """
    deadline = time.time() + seconds
    samples = []
    while True:
        now = time.time()
        save_debug = now >= deadline
        samples.append(frame_sample(label, save_debug=save_debug))
        if save_debug:
            break
        time.sleep(max(1, min(sample_every, int(deadline - now))))
    compact = [f"{sample['size']}:{sample['sha256']}" for sample in samples[-16:]]
    print(f"{label}: diagnostic framebuffer samples after {seconds}s: {compact}", flush=True)

def http_get_path(line):
    marker = '"GET '
    if marker not in line:
        return None
    return line.split(marker, 1)[1].split(" ", 1)[0]

def write_proof(path, proofs):
    parsed = urlparse(path)
    name = parsed.path.rsplit("/", 1)[-1]
    values = {k: v[-1] for k, v in parse_qs(parsed.query, keep_blank_values=True).items()}
    values.update({
        "name": name,
        "captured_via": "display-backed VM HTTP proof signal",
    })
    proofs[name] = values
    filename = PROOF_FILENAMES.get(name, f"{name}-proof.json")
    with open(f"{OUTDIR}/{filename}", "w", encoding="utf-8") as fh:
        json.dump(values, fh, indent=2, sort_keys=True)
        fh.write("\n")

def require_proofs(proofs):
    bad = [
        f"{name}={proofs.get(name, {}).get('status', 'missing')}"
        for name in REQUIRED_PROOFS
        if proofs.get(name, {}).get("status") != "pass"
    ]
    if bad:
        raise SystemExit("missing or failing required proof signals: " + ", ".join(bad))

# 0. Boot the highlighted installer entry instead of burning the GRUB timeout.
wait_serial_contains("ISO boot menu", "Install Goblins OS 44", 180)
if "Booting `Install Goblins OS 44'" not in serial_text():
    key("ret")
observe_serial_contains("ISO boot handoff", "Booting `Install Goblins OS 44'", 30)
# 1. Let Anaconda reach the storage confirmation, then drive the validated clicks.
wait_stage("Anaconda storage confirmation", 360)
click(0.55, 0.455); time.sleep(5)
frame_sample("Anaconda destination screen", save_debug=True)
click(0.34, 0.32); time.sleep(2)
frame_sample("Anaconda destination disk selected", save_debug=True)
click(0.039, 0.06); time.sleep(6)
frame_sample("Anaconda summary after destination", save_debug=True)
click(0.937, 0.895); time.sleep(5)
frame_sample("Anaconda begin submitted", save_debug=True)
# 2. Wait for the kickstart post marker before treating install progress as real.
wait_serial_contains("kickstart install post", "GOBLINS_VERIFY_INSTALL_DONE", 1800)
wait_stage("first boot desktop", 420)
# 3. dismiss onboarding
key("esc"); time.sleep(2); click(0.5, 0.627); time.sleep(3)
# 4. launch orchestrator via Alt+F2 (pipe-free)
key("alt+f2"); time.sleep(2); typ(f"curl -o /tmp/o 10.0.2.2:{PORT}/orchestrator.sh"); time.sleep(1); key("ret"); time.sleep(3)
key("alt+f2"); time.sleep(2); typ("bash /tmp/o"); time.sleep(1); key("ret")
# 5. capture on signals
os.makedirs(OUTDIR, exist_ok=True)
pos = os.path.getsize(HTTPLOG) if os.path.exists(HTTPLOG) else 0
seen = set(); proofs = {}; t = time.time()
while time.time() - t < 600:
    with open(HTTPLOG, errors="ignore") as fh:
        fh.seek(pos); chunk = fh.read(); pos = fh.tell()
    for line in chunk.splitlines():
        path = http_get_path(line)
        if not path:
            continue
        if path.startswith("/proof/"):
            write_proof(path, proofs)
            print(f"proof {path.split('?', 1)[0].rsplit('/', 1)[-1]}={proofs[path.split('?', 1)[0].rsplit('/', 1)[-1]].get('status', 'unknown')}", flush=True)
            continue
        if path.startswith("/ready/"):
            name = path.split("/ready/")[1].split("?")[0]
            if name == "ORCH_ALLDONE":
                require_proofs(proofs)
                print("ORCH_ALLDONE", flush=True); raise SystemExit(0)
            if name and name not in seen and name not in ("ORCH_START",):
                seen.add(name); ppm = f"{OUTDIR}/{name}.ppm"
                # Re-dump until the frame differs from the previous shot: a
                # launched window can render slower than the orchestrator's fixed
                # delay, so one dump may catch the prior/desktop frame.
                import hashlib
                last = globals().get("_last_md5")
                for _try in range(5):
                    dump(ppm)
                    try: h = hashlib.md5(open(ppm, "rb").read()).hexdigest()
                    except OSError: h = None
                    if h != last or _try == 4:
                        globals()["_last_md5"] = h; break
                    time.sleep(1.3)
                png(ppm, f"{OUTDIR}/{name}.png")
                try: os.remove(ppm)
                except OSError: pass
                print(f"captured {name} ({len(seen)})", flush=True)
    time.sleep(0.3)
require_proofs(proofs)
print(f"timeout; captured {len(seen)}", flush=True)
raise SystemExit(1)
