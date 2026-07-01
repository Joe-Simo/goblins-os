#!/usr/bin/env python3
"""Host-side driver for the hardware-gate display-backed-VM capture.

Codifies the validated flow against a running qemu VM (QMP socket):
  1. wait for the verification-only kickstart install marker
  2. require the kickstart %post marker, then wait for first-boot desktop settle
  3. complete first boot through the same core APIs as the visible private /
     offline path, then close the stale first-boot windows
  4. publish the in-session orchestrator for the verification-only user service
  5. screendump each surface to OUTDIR/<shot>.png on its HTTP /ready/<shot> signal
     until ORCH_ALLDONE

Env: GOS_QMP (qmp.sock), GOS_SERIALLOG (serial.log), GOS_HTTPLOG
(httpd.log), GOS_OUTDIR, GOS_PORT.
Note: per-shot window-focus timing lives in in-session-orchestrator.sh; tune the
settle there if surfaces capture as duplicates (md5-identical). KVM (CI) makes
the VM fast enough that the same timings that work under hvf hold.
"""
import hashlib, json, os, shutil, socket, subprocess, time
from urllib.parse import parse_qs, urlparse

QMP = os.environ["GOS_QMP"]; HTTPLOG = os.environ["GOS_HTTPLOG"]
SERIALLOG = os.environ.get("GOS_SERIALLOG", os.path.join(os.path.dirname(QMP), "serial.log"))
OUTDIR = os.environ["GOS_OUTDIR"]; PORT = os.environ.get("GOS_PORT", "8099")
DISPLAY_DEVICE = os.environ.get("GOS_QMP_DISPLAY_DEVICE", "video0")
ORCHESTRATOR_SOURCE = os.environ.get("GOS_ORCHESTRATOR_SOURCE")
ORCHESTRATOR_DEST = os.environ.get("GOS_ORCHESTRATOR_DEST")
ABS_MAX = 0x7fff
INSTALL_POST_TIMEOUT = int(os.environ.get("GOS_INSTALL_POST_TIMEOUT", "900"))
INSTALL_POST_TIMEOUT_EXIT = int(os.environ.get("GOS_INSTALL_POST_TIMEOUT_EXIT", "70"))
REQUIRED_FRAME_SETTLE_SECONDS = int(os.environ.get("GOS_REQUIRED_FRAME_SETTLE_SECONDS", "24"))
REQUIRED_PROOFS = (
    "firewall-live-toggle",
    "text-shortcuts-session-enable",
    "text-shortcuts-candidate-metadata",
    "text-shortcuts-overlay-intent",
    "text-shortcuts-candidate-bubble-frame",
    "text-shortcuts-candidate-bubble-layout",
    "text-shortcuts-candidate-bubble-render-intent",
    "text-shortcuts-candidate-bubble-render",
    "text-shortcuts-live-ibus-runtime-render",
    "keyboard-shortcuts-roundtrip",
    "input-sources-roundtrip",
    "multi-display-apply",
    "focus-arm-roundtrip",
    "app-privacy-revoke",
    "preview-open-render",
)
PROOF_FILENAMES = {
    "firewall-live-toggle": "firewall-live-toggle-proof.json",
    "text-shortcuts-session-enable": "text-shortcuts-session-enable-proof.json",
    "text-shortcuts-candidate-metadata": "text-shortcuts-candidate-metadata-proof.json",
    "text-shortcuts-overlay-intent": "text-shortcuts-overlay-intent-proof.json",
    "text-shortcuts-candidate-bubble-frame": "text-shortcuts-candidate-bubble-frame-proof.json",
    "text-shortcuts-candidate-bubble-layout": "text-shortcuts-candidate-bubble-layout-proof.json",
    "text-shortcuts-candidate-bubble-render-intent": "text-shortcuts-candidate-bubble-render-intent-proof.json",
    "text-shortcuts-candidate-bubble-render": "text-shortcuts-candidate-bubble-render-proof.json",
    "text-shortcuts-live-ibus-runtime-render": "text-shortcuts-live-ibus-runtime-render-proof.json",
    "keyboard-shortcuts-roundtrip": "keyboard-shortcuts-roundtrip-proof.json",
    "input-sources-roundtrip": "input-sources-roundtrip-proof.json",
    "multi-display-apply": "multi-display-apply-proof.json",
    "focus-arm-roundtrip": "focus-arm-roundtrip-proof.json",
    "app-privacy-revoke": "app-privacy-revoke-proof.json",
    "preview-open-render": "preview-open-render-proof.json",
}

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
        if "event" not in o:
            if "error" in o:
                raise SystemExit(f"QMP command {ex!r} failed: {o['error']}")
            return o
def try_cmd(ex, **a):
    try:
        return cmd(ex, **a)
    except SystemExit as err:
        print(f"diagnostic QMP command {ex!r} failed: {err}", flush=True)
        return None
def key(k): cmd("send-key", keys=[{"type": "qcode", "data": x} for x in k.split("+")])
def qcode_for_char(ch):
    if "a" <= ch <= "z" or "0" <= ch <= "9":
        return ch
    qcodes = {
        " ": "spc",
        ".": "dot",
        ",": "comma",
        "-": "minus",
        "_": "shift+minus",
        "/": "slash",
        ":": "shift+semicolon",
        ";": "semicolon",
        "'": "apostrophe",
    }
    if ch in qcodes:
        return qcodes[ch]
    raise SystemExit(f"unsupported QMP text input character: {ch!r}")
def qmp_type_text(text):
    for ch in text:
        key(qcode_for_char(ch))
        time.sleep(0.04)
def qmp_press_key(name):
    qcodes = {
        "Escape": "esc",
        "Return": "ret",
        "Space": "spc",
        "Tab": "tab",
        "Backspace": "backspace",
    }
    if name not in qcodes:
        raise SystemExit(f"unsupported QMP key input: {name!r}")
    key(qcodes[name])
def abs_axis(value):
    return int(max(0.0, min(1.0, value)) * ABS_MAX)
def click(xf, yf):
    route = {"device": DISPLAY_DEVICE} if DISPLAY_DEVICE else {}
    cmd("input-send-event", **route, events=[{"type": "abs", "data": {"axis": "x", "value": abs_axis(xf)}},
                                             {"type": "abs", "data": {"axis": "y", "value": abs_axis(yf)}}])
    cmd("input-send-event", **route, events=[{"type": "btn", "data": {"button": "left", "down": True}}])
    time.sleep(0.05)
    cmd("input-send-event", **route, events=[{"type": "btn", "data": {"button": "left", "down": False}}])
def dump(p): cmd("screendump", filename=p)
def png(ppm, out): subprocess.run(["sips", "-s", "format", "png", ppm, "--out", out] if os.uname().sysname == "Darwin"
                                  else ["convert", ppm, out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
def serial_text():
    try:
        with open(SERIALLOG, errors="ignore") as fh:
            return fh.read()
    except OSError:
        return ""

def fail(message, exit_code=1):
    print(message, flush=True)
    raise SystemExit(exit_code)

def http_log_text():
    try:
        with open(HTTPLOG, errors="ignore") as fh:
            return fh.read()
    except OSError:
        return ""

def wait_serial_contains(label, needle, timeout, debug_label=None, debug_every=0, exit_code=1):
    t = time.time()
    last_tail = ""
    last_debug = 0.0
    while time.time() - t < timeout:
        data = serial_text()
        if needle in data:
            print(f"{label}: observed serial marker {needle!r}", flush=True)
            return True
        if debug_label and debug_every and time.time() - last_debug >= debug_every:
            frame_sample(debug_label, save_debug=True)
            last_debug = time.time()
        last_tail = data[-500:]
        time.sleep(1)
    fail(
        f"{label} did not appear in serial log within {timeout}s; "
        f"expected {needle!r}; serial_tail={last_tail!r}",
        exit_code=exit_code,
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

def wait_http_contains(label, needle, timeout):
    t = time.time()
    last_tail = ""
    while time.time() - t < timeout:
        data = http_log_text()
        if needle in data:
            print(f"{label}: observed HTTP marker {needle!r}", flush=True)
            return True
        last_tail = data[-500:]
        time.sleep(1)
    raise SystemExit(
        f"{label} did not appear in HTTP log within {timeout}s; "
        f"expected {needle!r}; http_tail={last_tail!r}"
    )

def wait_http_contains_after(label, needle, start_pos, timeout):
    t = time.time()
    last_tail = ""
    while time.time() - t < timeout:
        try:
            with open(HTTPLOG, errors="ignore") as fh:
                fh.seek(start_pos)
                data = fh.read()
        except OSError:
            data = ""
        if needle in data:
            print(f"{label}: observed HTTP marker {needle!r}", flush=True)
            return True
        last_tail = data[-500:]
        time.sleep(1)
    raise SystemExit(
        f"{label} did not appear in HTTP log within {timeout}s; "
        f"expected {needle!r}; http_tail={last_tail!r}"
    )


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

def probe_graphical_vts():
    """Capture likely graphical VTs for failure diagnostics only.

    Failed gates have proven the installed deployment reaches a graphical
    Goblins session, but VT ownership is not stable across the text-install
    verification path. This probe is intentionally outside the happy path:
    switching VTs before first-boot/orchestrator work can leave the capture
    driver on the GDM surface instead of the live session. The proof path stays
    on the currently active graphical session and fails closed unless the
    verification-only user service produces the in-session HTTP callbacks.
    """
    print("first boot VT probe: checking likely graphical virtual terminals", flush=True)
    for debug_label, combo in (
        ("first boot vt f2", "ctrl+alt+f2"),
        ("first boot vt f7", "ctrl+alt+f7"),
        ("first boot vt f1", "ctrl+alt+f1"),
        ("first boot vt f7 final", "ctrl+alt+f7"),
    ):
        key(combo)
        time.sleep(3)
        frame_sample(debug_label, save_debug=True)

def complete_first_boot_setup():
    """Wait for the verification-only user service to complete first boot."""
    print("first boot setup: completing private offline path through session core APIs", flush=True)
    frame_sample("first boot before private unlock", save_debug=True)
    try:
        wait_http_contains("first boot helper download", "/firstboot-unlock.sh", 180)
        wait_http_contains("first boot private unlock callback", "/ready/FIRSTBOOT_UNLOCK", 180)
    except SystemExit:
        print("first boot setup failed before helper callback; collecting VT diagnostics", flush=True)
        probe_graphical_vts()
        raise
    frame_sample("post first boot private unlock", save_debug=True)

def publish_orchestrator():
    if not ORCHESTRATOR_SOURCE or not ORCHESTRATOR_DEST:
        raise SystemExit("missing GOS_ORCHESTRATOR_SOURCE/GOS_ORCHESTRATOR_DEST for verification service orchestration")
    shutil.copyfile(ORCHESTRATOR_SOURCE, ORCHESTRATOR_DEST)
    os.chmod(ORCHESTRATOR_DEST, 0o644)
    print(f"in-session orchestrator published for verification user service: {ORCHESTRATOR_DEST}", flush=True)

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

def handle_input(path):
    parsed = urlparse(path)
    values = {k: v[-1] for k, v in parse_qs(parsed.query, keep_blank_values=True).items()}
    name = parsed.path.rsplit("/", 1)[-1]
    if parsed.path.startswith("/input/click/"):
        try:
            x = float(values.get("x", "0.5"))
            y = float(values.get("y", "0.5"))
        except ValueError as err:
            raise SystemExit(f"invalid click coordinate for input route {name}: {err}") from err
        print(f"input click {name}: x={x:.3f} y={y:.3f}", flush=True)
        click(x, y)
        return
    if parsed.path.startswith("/input/text/"):
        text = values.get("text", "")
        print(f"input text {name}: {text!r}", flush=True)
        qmp_type_text(text)
        return
    if parsed.path.startswith("/input/key/"):
        key_name = values.get("key", "")
        print(f"input key {name}: {key_name!r}", flush=True)
        qmp_press_key(key_name)
        return
    raise SystemExit(f"unsupported input path: {path}")

def capture_ready_frame(name, frame_hashes):
    ppm = f"{OUTDIR}/{name}.ppm"
    out = f"{OUTDIR}/{name}.png"
    deadline = time.time() + REQUIRED_FRAME_SETTLE_SECONDS
    last_hash = None
    attempts = 0
    while True:
        attempts += 1
        dump(ppm)
        try:
            with open(ppm, "rb") as fh:
                data = fh.read()
            last_hash = hashlib.md5(data).hexdigest()
        except OSError:
            last_hash = None
        if not last_hash or last_hash not in frame_hashes:
            break
        if time.time() >= deadline:
            print(
                f"{name}: framebuffer stayed duplicate for {REQUIRED_FRAME_SETTLE_SECONDS}s; "
                "saving it so the signoff guard can fail closed",
                flush=True,
            )
            break
        time.sleep(1)
    if last_hash:
        frame_hashes.add(last_hash)
    png(ppm, out)
    try:
        os.remove(ppm)
    except OSError:
        pass
    print(f"captured {name} after {attempts} framebuffer sample(s)", flush=True)

def require_proofs(proofs):
    bad = [
        f"{name}={proofs.get(name, {}).get('status', 'missing')}"
        for name in REQUIRED_PROOFS
        if proofs.get(name, {}).get("status") != "pass"
    ]
    if bad:
        raise SystemExit("missing or failing required proof signals: " + ", ".join(bad))

# 0. Boot the highlighted installer entry instead of burning the GRUB timeout.
print(f"QMP display input route: {DISPLAY_DEVICE or 'default'}", flush=True)
print(f"QMP query-mice: {try_cmd('query-mice')}", flush=True)
wait_serial_contains("ISO boot menu", "Install Goblins OS 44", 180)
if "Booting `Install Goblins OS 44'" not in serial_text():
    key("ret")
observe_serial_contains("ISO boot handoff", "Booting `Install Goblins OS 44'", 30)
# 1. The verification-only embedded kickstart pins the scratch VM disk and
# should auto-start without interactive Anaconda clicks. Progress is proven only
# by the serial %post marker, with periodic framebuffer diagnostics on timeout
# paths.
wait_serial_contains(
    "kickstart install post",
    "GOBLINS_VERIFY_INSTALL_DONE",
    INSTALL_POST_TIMEOUT,
    debug_label="Anaconda automated kickstart progress",
    debug_every=120,
    exit_code=INSTALL_POST_TIMEOUT_EXIT,
)
# 2. Wait for first boot before treating install progress as real.
observe_serial_contains("first boot hardware diagnostics", "GOBLINS_HWGATE_DIAG_DONE", 180)
wait_stage("first boot desktop", 420)
observe_serial_contains(
    "session orchestrator starter",
    "GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED",
    5,
)
# 3. complete first boot through the real offline/private core contracts.
complete_first_boot_setup()
# 4. publish orchestrator only after the host is ready to tail its signals.
os.makedirs(OUTDIR, exist_ok=True)
pos = os.path.getsize(HTTPLOG) if os.path.exists(HTTPLOG) else 0
publish_orchestrator()
wait_http_contains_after("in-session orchestrator download", '"GET /orchestrator.sh HTTP/1.1" 200', pos, 180)
# 5. capture on signals
seen = set(); frame_hashes = set(); proofs = {}; t = time.time()
while time.time() - t < 600:
    with open(HTTPLOG, errors="ignore") as fh:
        fh.seek(pos); chunk = fh.read(); pos = fh.tell()
    for line in chunk.splitlines():
        path = http_get_path(line)
        if not path:
            continue
        if path.startswith("/input/"):
            handle_input(path)
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
            if name and name not in seen and name not in ("ORCH_START", "FIRSTBOOT_UNLOCK"):
                seen.add(name)
                capture_ready_frame(name, frame_hashes)
                print(f"captured {name} ({len(seen)})", flush=True)
    time.sleep(0.3)
require_proofs(proofs)
print(f"timeout; captured {len(seen)}", flush=True)
raise SystemExit(1)
