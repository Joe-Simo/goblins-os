#!/usr/bin/env python3
"""Host-side driver for the hardware-gate display-backed-VM capture.

Codifies the validated flow against a running qemu VM (QMP socket):
  1. wait for the verification-only kickstart install marker
  2. require the kickstart %post marker, then wait for first-boot desktop settle
  3. complete first boot through the same core APIs as the visible private /
     offline path, then close the stale first-boot windows
  4. publish the in-session orchestrator for the verification-only user service
  5. screendump each surface to OUTDIR/<shot>.png on its authenticated event
     until ORCH_ALLDONE

Env: GOS_QMP (qmp.sock), GOS_SERIALLOG (serial.log), GOS_CAPTURE_EVENTS,
GOS_CAPTURE_WORK_DIR, GOS_OUTDIR, GOS_PORT, GOS_CAPTURE_ACK_DIR.
Note: per-shot window-focus timing lives in in-session-orchestrator.sh; tune the
settle there if surfaces capture as duplicates (md5-identical). KVM (CI) makes
the VM fast enough that the same timings that work under hvf hold.
"""
import errno
import hashlib
import hmac
import ipaddress
import json
import os
import re
import socket
import socketserver
import stat
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import parse_qsl, urlsplit

from png_validation import MAX_CAPTURE_PIXELS, validate_png

MODE = sys.argv[1] if len(sys.argv) == 2 else ""
if len(sys.argv) > 2 or MODE not in (
    "",
    "--qmp-io-self-test",
    "--capture-channel-self-test",
    "--event-receiver",
):
    raise SystemExit(
        "usage: drive-capture.py "
        "[--qmp-io-self-test|--capture-channel-self-test|--event-receiver]"
    )
QMP_IO_SELF_TEST = MODE == "--qmp-io-self-test"
CAPTURE_CHANNEL_SELF_TEST = MODE == "--capture-channel-self-test"
EVENT_RECEIVER_MODE = MODE == "--event-receiver"
NON_DRIVER_MODE = QMP_IO_SELF_TEST or CAPTURE_CHANNEL_SELF_TEST or EVENT_RECEIVER_MODE

def _bounded_timeout(name, default, maximum):
    raw = os.environ.get(name, str(default))
    try:
        value = float(raw)
    except ValueError as error:
        raise SystemExit(f"{name} must be a number of seconds") from error
    if not 0.05 <= value <= maximum:
        raise SystemExit(f"{name} must be between 0.05 and {maximum} seconds")
    return value

QMP_CONNECT_ATTEMPT_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_QMP_CONNECT_ATTEMPT_TIMEOUT_SECONDS", 3, 30
)
QMP_CONNECT_TOTAL_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_QMP_CONNECT_TOTAL_TIMEOUT_SECONDS", 120, 300
)
QMP_GREETING_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_QMP_GREETING_TIMEOUT_SECONDS", 10, 60
)
QMP_CAPABILITIES_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_QMP_CAPABILITIES_TIMEOUT_SECONDS", 10, 60
)
QMP_COMMAND_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_QMP_COMMAND_TIMEOUT_SECONDS", 30, 120
)
PNG_CONVERSION_TIMEOUT_SECONDS = _bounded_timeout(
    "GOS_PNG_CONVERSION_TIMEOUT_SECONDS", 120, 300
)
QMP_MAX_MESSAGE_BYTES = 1024 * 1024
ACTIVE_CAPTURE_DEADLINE = None

if NON_DRIVER_MODE:
    QMP = SERIALLOG = OUTDIR = CAPTURE_ACK_DIR = EVENTS = WORKDIR = ""
    PORT = "8099"
    DISPLAY_DEVICE = "video0"
    ORCHESTRATOR_SOURCE = ORCHESTRATOR_DEST = None
else:
    QMP = os.environ["GOS_QMP"]
    SERIALLOG = os.environ.get(
        "GOS_SERIALLOG", os.path.join(os.path.dirname(QMP), "serial.log")
    )
    EVENTS = os.environ["GOS_CAPTURE_EVENTS"]
    WORKDIR = os.environ["GOS_CAPTURE_WORK_DIR"]
    OUTDIR = os.environ["GOS_OUTDIR"]
    PORT = os.environ.get("GOS_PORT", "8099")
    DISPLAY_DEVICE = os.environ.get("GOS_QMP_DISPLAY_DEVICE", "video0")
    ORCHESTRATOR_SOURCE = os.environ.get("GOS_ORCHESTRATOR_SOURCE")
    ORCHESTRATOR_DEST = os.environ.get("GOS_ORCHESTRATOR_DEST")
    CAPTURE_ACK_DIR = os.environ["GOS_CAPTURE_ACK_DIR"]
ABS_MAX = 0x7fff
INSTALL_POST_TIMEOUT = int(os.environ.get("GOS_INSTALL_POST_TIMEOUT", "900"))
INSTALL_POST_TIMEOUT_EXIT = int(os.environ.get("GOS_INSTALL_POST_TIMEOUT_EXIT", "70"))
INSTALL_MARKER_EXIT_CODE = int(os.environ.get("GOS_EXIT_AFTER_INSTALL_MARKER", "0") or "0")
SKIP_INSTALL_PHASE = os.environ.get("GOS_SKIP_INSTALL_PHASE") == "1"
REQUIRED_FRAME_SETTLE_SECONDS = int(os.environ.get("GOS_REQUIRED_FRAME_SETTLE_SECONDS", "24"))
CAPTURE_TOTAL_TIMEOUT_SECONDS = int(os.environ.get("GOS_CAPTURE_TOTAL_TIMEOUT_SECONDS", "1200"))
CAPTURE_INACTIVITY_TIMEOUT_SECONDS = int(os.environ.get("GOS_CAPTURE_INACTIVITY_TIMEOUT_SECONDS", "180"))
EXPECTED_READY_SHOTS = (
    "01-installer",
    "02-install-network",
    "03-login",
    "04-desktop",
    "05-first-boot-private-unlock",
    "06-onboarding",
    "07-home",
    "08-shell-home",
    "09-shell-dark",
    "10-settings",
    "11-settings-models",
    "12-settings-dark",
    "13-studio-before",
    "14-studio-running",
    "15-studio-app-detail",
    "16-built-app-open",
    "17-dark-motion",
    "18-light-motion",
    "19-vulkan-vkcube",
    "20-gamemode-active",
    "21-gamescope-session",
    "22-mangohud-overlay",
    "23-controller-detection",
    "24-audio-output",
    "25-install-destination",
    "26-install-storage-summary",
    "27-dual-boot-preserve-existing-os",
    "28-bootloader-efi-summary",
    "29-preview-pdf-open",
    "30-preview-image-open",
    "31-text-shortcuts-candidate-bubble-render",
    "32-text-shortcuts-live-ibus-runtime-render",
)
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
    "audio-output",
    "runtime-build",
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
    "audio-output": "audio-output-proof.json",
    "runtime-build": "runtime-build-proof.json",
}

HTTP_MAX_REQUEST_BYTES = 12 * 1024
HTTP_MAX_HEADER_BYTES = 8 * 1024
HTTP_MAX_HEADERS = 32
HTTP_READ_TIMEOUT_SECONDS = 5
EVENT_MAX_RECORD_BYTES = 16 * 1024
EVENT_MAX_STREAM_BYTES = 8 * 1024 * 1024
EVENT_MAX_COUNT = 4096
EVENT_MAX_QUERY_FIELDS = 192
EVENT_MAX_VALUE_BYTES = 2048
SERIAL_MAX_BYTES = 64 * 1024 * 1024
LOG_READ_CHUNK_BYTES = 64 * 1024
LOG_OVERLAP_BYTES = 4096
LOG_DIAGNOSTIC_TAIL_BYTES = 2048
PPM_MAX_HEADER_BYTES = 4096
PPM_MAX_BYTES = MAX_CAPTURE_PIXELS * 3 + PPM_MAX_HEADER_BYTES
CAPTURE_CANVAS_WIDTH = 5120
CAPTURE_CANVAS_HEIGHT = 2880
CAPTURE_CANVAS_DIMENSIONS = (CAPTURE_CANVAS_WIDTH, CAPTURE_CANVAS_HEIGHT)
SAFE_EVENT_NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,95}$")
SAFE_QUERY_KEY = re.compile(r"^[a-z][a-z0-9_]{0,63}$")
TOKEN_PATTERN = re.compile(r"^[0-9a-f]{64}$")


class CaptureChannelError(RuntimeError):
    pass


def _bounded_regular_file(path, maximum, *, expected_uid=None):
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise CaptureChannelError("capture channel leaf is not a single-link file")
        if expected_uid is not None and metadata.st_uid != expected_uid:
            raise CaptureChannelError("capture channel leaf has the wrong owner")
        if metadata.st_size < 0 or metadata.st_size > maximum:
            raise CaptureChannelError("capture channel leaf exceeds its size limit")
        chunks = []
        remaining = maximum + 1
        while remaining:
            chunk = os.read(descriptor, min(65536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        value = b"".join(chunks)
        if len(value) > maximum or os.read(descriptor, 1):
            raise CaptureChannelError("capture channel leaf grew beyond its size limit")
        return value
    finally:
        os.close(descriptor)


def _create_private_leaf(path, data=b"", mode=0o600):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, mode)
    try:
        view = memoryview(data)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise CaptureChannelError("short write while creating private leaf")
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


class CaptureEventStore:
    def __init__(self, path, maximum_bytes=EVENT_MAX_STREAM_BYTES, maximum_count=EVENT_MAX_COUNT):
        self.path = path
        self.maximum_bytes = maximum_bytes
        self.maximum_count = maximum_count
        self.count = 0
        self.size = 0
        flags = (
            os.O_WRONLY
            | os.O_APPEND
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_NOFOLLOW", 0)
        )
        self.descriptor = os.open(path, flags, 0o600)
        self.lock = threading.Lock()

    def close(self):
        if self.descriptor is not None:
            os.close(self.descriptor)
            self.descriptor = None

    def append(self, event):
        with self.lock:
            if self.count >= self.maximum_count:
                raise CaptureChannelError("capture event count limit reached")
            recorded_event = dict(event)
            recorded_event["sequence"] = self.count + 1
            encoded = (
                json.dumps(
                    recorded_event,
                    separators=(",", ":"),
                    sort_keys=True,
                    ensure_ascii=True,
                )
                + "\n"
            ).encode("ascii")
            if len(encoded) > EVENT_MAX_RECORD_BYTES:
                raise CaptureChannelError("capture event exceeds its record limit")
            if self.size + len(encoded) > self.maximum_bytes:
                raise CaptureChannelError("capture event stream limit reached")
            view = memoryview(encoded)
            while view:
                written = os.write(self.descriptor, view)
                if written <= 0:
                    raise CaptureChannelError("short capture event write")
                view = view[written:]
            os.fsync(self.descriptor)
            self.count += 1
            self.size += len(encoded)


def _strict_query(query):
    if not query:
        return {}
    if re.search(r"%(?![0-9A-Fa-f]{2})", query):
        raise CaptureChannelError("event query contains invalid percent encoding")
    try:
        pairs = parse_qsl(
            query,
            keep_blank_values=True,
            strict_parsing=True,
            encoding="utf-8",
            errors="strict",
            max_num_fields=EVENT_MAX_QUERY_FIELDS,
        )
    except (UnicodeDecodeError, ValueError) as error:
        raise CaptureChannelError("invalid event query") from error
    values = {}
    for key, value in pairs:
        if not SAFE_QUERY_KEY.fullmatch(key):
            raise CaptureChannelError("invalid event query key")
        if key in values:
            raise CaptureChannelError("duplicate event query key")
        if len(value.encode("utf-8")) > EVENT_MAX_VALUE_BYTES:
            raise CaptureChannelError("event query value exceeds its limit")
        if any(ord(character) < 0x20 for character in value):
            raise CaptureChannelError("event query value contains a control character")
        values[key] = value
    return values


class CaptureReceiver:
    def __init__(self, token, event_store, helpers, capture_ack_dir):
        if not TOKEN_PATTERN.fullmatch(token):
            raise CaptureChannelError("capture bearer token must be 64 lowercase hex characters")
        self.token = token
        self.event_store = event_store
        self.helpers = helpers
        self.capture_ack_dir = capture_ack_dir

    def _event_for_target(self, target):
        if len(target.encode("ascii", errors="strict")) > HTTP_MAX_REQUEST_BYTES:
            raise CaptureChannelError("request target exceeds its limit")
        if "%" in target.split("?", 1)[0] or "#" in target:
            raise CaptureChannelError("encoded or fragmented event path is forbidden")
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc or parsed.fragment:
            raise CaptureChannelError("request target must use origin form")
        values = _strict_query(parsed.query)
        path = parsed.path
        if path in self.helpers:
            if values:
                raise CaptureChannelError("helper download does not accept a query")
            return "helper", {"kind": "helper", "name": path[1:]}
        ack_match = re.fullmatch(r"/capture-acks/([a-z0-9][a-z0-9-]{0,95})[.]captured", path)
        if ack_match:
            if values or ack_match.group(1) not in EXPECTED_READY_SHOTS:
                raise CaptureChannelError("invalid capture acknowledgement route")
            return "ack", {"kind": "ack", "name": ack_match.group(1)}
        ready_match = re.fullmatch(r"/ready/([A-Z0-9_-]+|[a-z0-9][a-z0-9-]{0,95})", path)
        if ready_match:
            name = ready_match.group(1)
            allowed = set(EXPECTED_READY_SHOTS) - {"05-first-boot-private-unlock"}
            allowed.update(("FIRSTBOOT_UNLOCK", "ORCH_START", "ORCH_ALLDONE"))
            if name not in allowed:
                raise CaptureChannelError("unknown ready event")
            if name == "FIRSTBOOT_UNLOCK":
                if values != {"status": "pass"}:
                    raise CaptureChannelError("invalid first-boot ready event")
            elif values:
                raise CaptureChannelError("ready event does not accept a query")
            return "event", {"kind": "ready", "name": name, "values": values}
        if path == "/failed/FIRSTBOOT_UNLOCK":
            if set(values) != {"stage", "rc"} or not values["stage"] or not re.fullmatch(
                r"[0-9]{1,3}", values["rc"]
            ):
                raise CaptureChannelError("invalid first-boot failure event")
            return "event", {"kind": "failed", "name": "FIRSTBOOT_UNLOCK", "values": values}
        proof_match = re.fullmatch(r"/proof/([a-z0-9][a-z0-9-]{0,95})", path)
        if proof_match:
            name = proof_match.group(1)
            if name not in REQUIRED_PROOFS or values.get("status") not in ("pass", "fail"):
                raise CaptureChannelError("unknown or invalid proof event")
            return "event", {"kind": "proof", "name": name, "values": values}
        input_match = re.fullmatch(
            r"/input/(click|text|key)/([a-z0-9][a-z0-9-]{0,95})", path
        )
        if input_match:
            input_kind, name = input_match.groups()
            expected_keys = {"click": {"x", "y"}, "text": {"text"}, "key": {"key"}}[
                input_kind
            ]
            if set(values) != expected_keys:
                raise CaptureChannelError("input event query has the wrong fields")
            if input_kind == "click" and any(
                not re.fullmatch(r"(?:0(?:[.][0-9]{1,6})?|1(?:[.]0{1,6})?)", values[key])
                for key in ("x", "y")
            ):
                raise CaptureChannelError("input click coordinate is outside 0 through 1")
            if input_kind == "key" and values["key"] not in (
                "Escape",
                "Return",
                "Space",
                "Tab",
                "Backspace",
            ):
                raise CaptureChannelError("input key is not allowlisted")
            if input_kind == "text" and (
                not values["text"]
                or len(values["text"]) > 256
                or any(
                    character not in "abcdefghijklmnopqrstuvwxyz0123456789 .,-_/:;'"
                    for character in values["text"]
                )
            ):
                raise CaptureChannelError("input text is not representable by QMP")
            return "event", {
                "kind": "input",
                "input_kind": input_kind,
                "name": name,
                "values": values,
            }
        raise FileNotFoundError("unknown capture route")

    def handle(self, request):
        request.settimeout(HTTP_READ_TIMEOUT_SECONDS)
        received = bytearray()
        while b"\r\n\r\n" not in received:
            if len(received) >= HTTP_MAX_REQUEST_BYTES:
                return 414, b"request too large\n", "text/plain; charset=utf-8"
            chunk = request.recv(min(4096, HTTP_MAX_REQUEST_BYTES + 1 - len(received)))
            if not chunk:
                return 400, b"bad request\n", "text/plain; charset=utf-8"
            received.extend(chunk)
        header_block, buffered_body = bytes(received).split(b"\r\n\r\n", 1)
        if buffered_body or len(header_block) > HTTP_MAX_HEADER_BYTES:
            return 400, b"bad request\n", "text/plain; charset=utf-8"
        lines = header_block.split(b"\r\n")
        if not lines or len(lines) - 1 > HTTP_MAX_HEADERS:
            return 400, b"bad request\n", "text/plain; charset=utf-8"
        try:
            request_line = lines[0].decode("ascii", errors="strict")
            method, target, version = request_line.split(" ")
        except (UnicodeDecodeError, ValueError):
            return 400, b"bad request\n", "text/plain; charset=utf-8"
        if version != "HTTP/1.1":
            return 505, b"HTTP version not supported\n", "text/plain; charset=utf-8"
        headers = {}
        for raw_line in lines[1:]:
            if not raw_line or raw_line[:1] in b" \t" or b":" not in raw_line:
                return 400, b"bad request\n", "text/plain; charset=utf-8"
            raw_name, raw_value = raw_line.split(b":", 1)
            try:
                name = raw_name.decode("ascii", errors="strict").lower()
                value = raw_value.strip().decode("ascii", errors="strict")
            except UnicodeDecodeError:
                return 400, b"bad request\n", "text/plain; charset=utf-8"
            if not re.fullmatch(r"[a-z0-9-]+", name) or name in headers:
                return 400, b"bad request\n", "text/plain; charset=utf-8"
            headers[name] = value
        if method != "GET":
            return 405, b"method not allowed\n", "text/plain; charset=utf-8"
        if not headers.get("host") or any(
            character in headers["host"] for character in " \t\r\n"
        ):
            return 400, b"bad request\n", "text/plain; charset=utf-8"
        if "transfer-encoding" in headers or headers.get("content-length", "0") != "0":
            return 400, b"request body forbidden\n", "text/plain; charset=utf-8"
        authorization = headers.get("authorization", "")
        expected = f"Bearer {self.token}"
        if not hmac.compare_digest(authorization, expected):
            return 403, b"forbidden\n", "text/plain; charset=utf-8"
        try:
            route_kind, event = self._event_for_target(target)
            if route_kind == "helper":
                body = _bounded_regular_file(
                    self.helpers[f"/{event['name']}"], 4 * 1024 * 1024, expected_uid=os.getuid()
                )
                self.event_store.append(event)
                return 200, body, "application/octet-stream"
            if route_kind == "ack":
                path = os.path.join(self.capture_ack_dir, f"{event['name']}.captured")
                body = _bounded_regular_file(path, 4096, expected_uid=os.getuid())
                return 200, body, "text/plain; charset=utf-8"
            self.event_store.append(event)
            return 204, b"", "text/plain; charset=utf-8"
        except FileNotFoundError:
            return 404, b"not found\n", "text/plain; charset=utf-8"
        except (CaptureChannelError, OSError, UnicodeEncodeError):
            return 400, b"bad request\n", "text/plain; charset=utf-8"


class _CaptureRequestHandler(socketserver.BaseRequestHandler):
    def handle(self):
        try:
            status_code, body, content_type = self.server.receiver.handle(self.request)
        except (OSError, socket.timeout):
            status_code, body, content_type = 408, b"request timeout\n", "text/plain; charset=utf-8"
        reason = {
            200: "OK",
            204: "No Content",
            400: "Bad Request",
            403: "Forbidden",
            404: "Not Found",
            405: "Method Not Allowed",
            408: "Request Timeout",
            414: "URI Too Long",
            505: "HTTP Version Not Supported",
        }[status_code]
        response = (
            f"HTTP/1.1 {status_code} {reason}\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Content-Type: {content_type}\r\n"
            "Cache-Control: no-store\r\n"
            "Connection: close\r\n\r\n"
        ).encode("ascii") + body
        try:
            self.request.sendall(response)
        except OSError:
            pass


class CaptureHTTPServer(socketserver.TCPServer):
    allow_reuse_address = False
    request_queue_size = 8

    def __init__(self, address, receiver):
        self.receiver = receiver
        super().__init__(address, _CaptureRequestHandler)

    def verify_request(self, request, client_address):
        try:
            return ipaddress.ip_address(client_address[0]).is_loopback
        except ValueError:
            return False


def _read_capture_token(path):
    raw = _bounded_regular_file(path, 65, expected_uid=os.getuid())
    try:
        token = raw.decode("ascii", errors="strict").strip()
    except UnicodeDecodeError as error:
        raise CaptureChannelError("capture token is not ASCII") from error
    if not TOKEN_PATTERN.fullmatch(token):
        raise CaptureChannelError("capture token file has an invalid value")
    return token


def _run_event_receiver():
    port_text = os.environ.get("GOS_PORT", "8099")
    if not re.fullmatch(r"[0-9]{1,5}", port_text) or not 1024 <= int(port_text) <= 65535:
        raise SystemExit("GOS_PORT must be an unprivileged TCP port")
    event_path = os.environ["GOS_CAPTURE_EVENTS"]
    token = _read_capture_token(os.environ["GOS_CAPTURE_TOKEN_FILE"])
    helpers = {
        "/firstboot-unlock.sh": os.environ["GOS_CAPTURE_FIRSTBOOT_HELPER"],
        "/core-proof-operation.sh": os.environ["GOS_CAPTURE_CORE_PROOF_HELPER"],
        "/orchestrator.sh": os.environ["GOS_ORCHESTRATOR_DEST"],
    }
    store = CaptureEventStore(event_path)
    server = None
    try:
        receiver = CaptureReceiver(token, store, helpers, os.environ["GOS_CAPTURE_ACK_DIR"])
        server = CaptureHTTPServer(("127.0.0.1", int(port_text)), receiver)
        _create_private_leaf(os.environ["GOS_CAPTURE_RECEIVER_READY"], b"ready\n")
        print("capture receiver ready on QEMU slirp loopback", flush=True)
        server.serve_forever(poll_interval=0.25)
    finally:
        if server is not None:
            server.server_close()
        store.close()


class IncrementalFileReader:
    def __init__(self, path, maximum_bytes, start_offset=0):
        self.path = path
        self.maximum_bytes = maximum_bytes
        self.offset = start_offset
        self.overlap = b""
        self.tail = b""
        self.descriptor = None

    def close(self):
        if self.descriptor is not None:
            os.close(self.descriptor)
            self.descriptor = None

    def _open(self):
        if self.descriptor is not None:
            return
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        try:
            self.descriptor = os.open(self.path, flags)
        except FileNotFoundError:
            return
        metadata = os.fstat(self.descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_nlink != 1
            or metadata.st_uid != os.getuid()
        ):
            self.close()
            raise CaptureChannelError("incremental log is not a safe regular file")
        if metadata.st_size < self.offset:
            self.close()
            raise CaptureChannelError("incremental log was truncated before its start offset")

    def read_available(self):
        self._open()
        if self.descriptor is None:
            return b""
        metadata = os.fstat(self.descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_nlink != 1
            or metadata.st_uid != os.getuid()
        ):
            raise CaptureChannelError("incremental log changed identity")
        if metadata.st_size < self.offset:
            raise CaptureChannelError("incremental log was truncated")
        if metadata.st_size > self.maximum_bytes:
            raise CaptureChannelError(
                f"incremental log exceeded its {self.maximum_bytes}-byte limit"
            )
        chunks = []
        while self.offset < metadata.st_size:
            amount = min(LOG_READ_CHUNK_BYTES, metadata.st_size - self.offset)
            if hasattr(os, "pread"):
                chunk = os.pread(self.descriptor, amount, self.offset)
            else:
                os.lseek(self.descriptor, self.offset, os.SEEK_SET)
                chunk = os.read(self.descriptor, amount)
            if not chunk:
                raise CaptureChannelError("incremental log returned a short read")
            self.offset += len(chunk)
            chunks.append(chunk)
        new = b"".join(chunks)
        if new:
            self.tail = (self.tail + new)[-LOG_DIAGNOSTIC_TAIL_BYTES:]
        return new

    def contains(self, needle):
        encoded = needle.encode("utf-8")
        new = self.read_available()
        searchable = self.overlap + new
        found = encoded in searchable
        self.overlap = searchable[-max(LOG_OVERLAP_BYTES, len(encoded) - 1) :]
        return found

    def diagnostic_tail(self):
        return self.tail.decode("utf-8", errors="replace")


class IncrementalEventReader:
    def __init__(self, path):
        self.reader = IncrementalFileReader(path, EVENT_MAX_STREAM_BYTES)
        self.pending = b""
        self.expected_sequence = 1

    def close(self):
        self.reader.close()

    @staticmethod
    def _validate(event):
        kind = event.get("kind")
        common = {"kind", "name", "sequence"}
        if kind == "helper":
            if set(event) != common or event.get("name") not in (
                "firstboot-unlock.sh",
                "core-proof-operation.sh",
                "orchestrator.sh",
            ):
                raise CaptureChannelError("capture event stream contains an invalid helper event")
            return
        if kind in ("ready", "failed", "proof"):
            if set(event) != common | {"values"}:
                raise CaptureChannelError("capture event stream contains unknown fields")
        elif kind == "input":
            if set(event) != common | {"values", "input_kind"}:
                raise CaptureChannelError("capture input event contains unknown fields")
            if event.get("input_kind") not in ("click", "text", "key"):
                raise CaptureChannelError("capture input event has an invalid type")
        else:
            raise CaptureChannelError("capture event stream contains an unknown event kind")
        if not isinstance(event.get("name"), str) or not event["name"]:
            raise CaptureChannelError("capture event stream contains an invalid event name")
        values = event.get("values")
        if not isinstance(values, dict) or any(
            not isinstance(key, str) or not isinstance(value, str)
            for key, value in values.items()
        ):
            raise CaptureChannelError("capture event stream contains invalid event values")

    def poll(self):
        self.pending += self.reader.read_available()
        if len(self.pending) > EVENT_MAX_RECORD_BYTES and b"\n" not in self.pending:
            raise CaptureChannelError("capture event record exceeded its limit")
        events = []
        while b"\n" in self.pending:
            raw, self.pending = self.pending.split(b"\n", 1)
            if not raw or len(raw) + 1 > EVENT_MAX_RECORD_BYTES:
                raise CaptureChannelError("capture event stream contains an invalid record")
            try:
                event = json.loads(
                    raw.decode("ascii", errors="strict"),
                    object_pairs_hook=_reject_duplicate_json_keys,
                    parse_constant=_reject_json_constant,
                )
            except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
                raise CaptureChannelError("capture event stream contains invalid JSON") from error
            if not isinstance(event, dict) or type(event.get("sequence")) is not int:
                raise CaptureChannelError("capture event stream contains an invalid event")
            if event["sequence"] != self.expected_sequence:
                raise CaptureChannelError("capture event sequence is missing or reordered")
            self._validate(event)
            self.expected_sequence += 1
            events.append(event)
        return events

    def diagnostic_tail(self):
        return self.reader.diagnostic_tail()

class QmpProtocolError(RuntimeError):
    pass

class QmpTimeoutError(RuntimeError):
    pass

class QmpCommandError(RuntimeError):
    pass

def _capture_bounded_timeout(default_seconds, label):
    if ACTIVE_CAPTURE_DEADLINE is None:
        return default_seconds
    remaining = ACTIVE_CAPTURE_DEADLINE - time.monotonic()
    if remaining <= 0:
        raise SystemExit(f"capture deadline expired before {label}")
    return min(default_seconds, remaining)

def _reject_duplicate_json_keys(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON key {key!r}")
        value[key] = item
    return value

def _reject_json_constant(value):
    raise ValueError(f"non-finite JSON number {value!r}")

class QmpClient:
    def __init__(self, connection):
        self._connection = connection
        self._buffer = bytearray()
        self._next_id = 1

    def close(self):
        try:
            self._connection.close()
        except OSError:
            pass

    @staticmethod
    def _remaining(deadline, label):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise QmpTimeoutError(f"{label} timed out")
        return remaining

    def _send(self, message, deadline, label):
        encoded = json.dumps(
            message, separators=(",", ":"), ensure_ascii=True, allow_nan=False
        ).encode("ascii") + b"\n"
        try:
            self._connection.settimeout(self._remaining(deadline, label))
            self._connection.sendall(encoded)
        except socket.timeout as error:
            raise QmpTimeoutError(f"{label} timed out while sending") from error
        except OSError as error:
            raise QmpProtocolError(f"{label} failed while sending: {error}") from error

    def _read(self, deadline, label):
        while True:
            newline = self._buffer.find(b"\n")
            if newline >= 0:
                if newline > QMP_MAX_MESSAGE_BYTES:
                    raise QmpProtocolError(
                        f"{label} exceeded the {QMP_MAX_MESSAGE_BYTES}-byte limit"
                    )
                raw = bytes(self._buffer[:newline])
                del self._buffer[: newline + 1]
                if not raw:
                    raise QmpProtocolError(f"{label} returned an empty frame")
                try:
                    decoded = raw.decode("utf-8", errors="strict")
                    message = json.loads(
                        decoded,
                        object_pairs_hook=_reject_duplicate_json_keys,
                        parse_constant=_reject_json_constant,
                    )
                except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
                    raise QmpProtocolError(f"{label} returned malformed JSON") from error
                if not isinstance(message, dict):
                    raise QmpProtocolError(f"{label} did not return a JSON object")
                return message
            if len(self._buffer) > QMP_MAX_MESSAGE_BYTES:
                raise QmpProtocolError(
                    f"{label} exceeded the {QMP_MAX_MESSAGE_BYTES}-byte limit"
                )
            try:
                self._connection.settimeout(self._remaining(deadline, label))
                chunk = self._connection.recv(
                    min(65536, QMP_MAX_MESSAGE_BYTES + 1 - len(self._buffer))
                )
            except socket.timeout as error:
                raise QmpTimeoutError(f"{label} timed out while receiving") from error
            except OSError as error:
                raise QmpProtocolError(f"{label} failed while receiving: {error}") from error
            if not chunk:
                raise QmpProtocolError(f"{label} reached EOF before a complete reply")
            self._buffer.extend(chunk)

    def read_greeting(self, timeout_seconds):
        deadline = time.monotonic() + timeout_seconds
        greeting = self._read(deadline, "QMP greeting")
        if set(greeting) != {"QMP"} or not isinstance(greeting["QMP"], dict):
            raise QmpProtocolError("QMP greeting did not contain the required QMP object")
        return greeting

    def execute(self, command, arguments=None, timeout_seconds=QMP_COMMAND_TIMEOUT_SECONDS):
        if not isinstance(command, str) or not command:
            raise ValueError("QMP command must be a nonempty string")
        if arguments is not None and not isinstance(arguments, dict):
            raise ValueError("QMP arguments must be an object")
        request_id = self._next_id
        self._next_id += 1
        request = {"execute": command, "id": request_id}
        if arguments:
            request["arguments"] = arguments
        label = f"QMP command {command!r}"
        deadline = time.monotonic() + timeout_seconds
        self._send(request, deadline, label)
        while True:
            reply = self._read(deadline, label)
            if "event" in reply:
                if (
                    not isinstance(reply.get("event"), str)
                    or not reply["event"]
                    or not set(reply).issubset({"event", "data", "timestamp"})
                    or ("data" in reply and not isinstance(reply["data"], dict))
                    or ("timestamp" in reply and not isinstance(reply["timestamp"], dict))
                ):
                    raise QmpProtocolError(f"{label} received a malformed event")
                continue
            if not set(reply).issubset({"return", "error", "id"}):
                raise QmpProtocolError(f"{label} received unknown response fields")
            if type(reply.get("id")) is not int or reply["id"] != request_id:
                raise QmpProtocolError(f"{label} received a mismatched response id")
            if ("return" in reply) == ("error" in reply):
                raise QmpProtocolError(
                    f"{label} must contain exactly one of return or error"
                )
            if "error" in reply:
                error = reply["error"]
                if (
                    not isinstance(error, dict)
                    or not isinstance(error.get("class"), str)
                    or not error["class"]
                    or not isinstance(error.get("desc"), str)
                    or not error["desc"]
                ):
                    raise QmpProtocolError(f"{label} received a malformed error")
                raise QmpCommandError(
                    f"{error['class']}: {error['desc']}"
                )
            return reply

def _run_image_conversion(command, timeout_seconds=PNG_CONVERSION_TIMEOUT_SECONDS):
    return subprocess.run(
        command,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=timeout_seconds,
    )

def _expect_qmp_failure(expected, operation):
    try:
        operation()
    except expected:
        return
    raise AssertionError(f"expected {expected.__name__}")

def _qmp_io_self_test():
    client_socket, peer_socket = socket.socketpair()
    try:
        peer_socket.sendall(
            b'{"QMP":{"version":{},"capabilities":[]}}\n'
            b'{"event":"RESET","data":{}}\n'
            b'{"return":{},"id":1}\n'
            b'{"return":{"status":"running"},"id":2}\n'
        )
        client = QmpClient(client_socket)
        client.read_greeting(0.25)
        client.execute("qmp_capabilities", timeout_seconds=0.25)
        reply = client.execute("query-status", timeout_seconds=0.25)
        assert reply == {"return": {"status": "running"}, "id": 2}
    finally:
        client_socket.close()
        peer_socket.close()

    for payload in (
        b"",
        b"not-json\n",
        b'{"not-QMP":{}}\n',
        b'{"QMP":{},"QMP":{}}\n',
    ):
        client_socket, peer_socket = socket.socketpair()
        try:
            if payload:
                peer_socket.sendall(payload)
            else:
                peer_socket.close()
            client = QmpClient(client_socket)
            _expect_qmp_failure(
                QmpProtocolError, lambda: client.read_greeting(0.25)
            )
        finally:
            client_socket.close()
            peer_socket.close()

    client_socket, peer_socket = socket.socketpair()
    try:
        client = QmpClient(client_socket)
        started = time.monotonic()
        _expect_qmp_failure(QmpTimeoutError, lambda: client.read_greeting(0.05))
        assert time.monotonic() - started < 0.5
    finally:
        client_socket.close()
        peer_socket.close()

    for payload in (
        b"not-json\n",
        b'{"return":{},"id":99}\n',
        b'{"return":{},"return":{},"id":1}\n',
    ):
        client_socket, peer_socket = socket.socketpair()
        try:
            peer_socket.sendall(payload)
            client = QmpClient(client_socket)
            _expect_qmp_failure(
                QmpProtocolError,
                lambda: client.execute("query-status", timeout_seconds=0.25),
            )
        finally:
            client_socket.close()
            peer_socket.close()

    client_socket, peer_socket = socket.socketpair()
    try:
        peer_socket.close()
        client = QmpClient(client_socket)
        _expect_qmp_failure(
            QmpProtocolError,
            lambda: client.execute("query-status", timeout_seconds=0.25),
        )
    finally:
        client_socket.close()
        peer_socket.close()

    client_socket, peer_socket = socket.socketpair()
    try:
        client = QmpClient(client_socket)
        started = time.monotonic()
        _expect_qmp_failure(
            QmpTimeoutError,
            lambda: client.execute("query-status", timeout_seconds=0.05),
        )
        assert time.monotonic() - started < 0.5
    finally:
        client_socket.close()
        peer_socket.close()

    started = time.monotonic()
    _expect_qmp_failure(
        subprocess.TimeoutExpired,
        lambda: _run_image_conversion(
            [sys.executable, "-c", "import time; time.sleep(2)"], 0.05
        ),
    )
    assert time.monotonic() - started < 0.75

def _conn():
    deadline = time.monotonic() + QMP_CONNECT_TOTAL_TIMEOUT_SECONDS
    last_error = "socket missing"
    while True:
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client = None
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            connection.settimeout(min(QMP_CONNECT_ATTEMPT_TIMEOUT_SECONDS, remaining))
            connection.connect(QMP)
            client = QmpClient(connection)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise QmpTimeoutError("QMP connect deadline expired before greeting")
            client.read_greeting(min(QMP_GREETING_TIMEOUT_SECONDS, remaining))
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise QmpTimeoutError("QMP connect deadline expired before capabilities")
            client.execute(
                "qmp_capabilities",
                timeout_seconds=min(QMP_CAPABILITIES_TIMEOUT_SECONDS, remaining),
            )
            return client
        except (socket.timeout, QmpTimeoutError, OSError) as error:
            last_error = repr(error)
            (client.close() if client is not None else connection.close())
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            time.sleep(min(1, remaining))
        except (QmpProtocolError, QmpCommandError) as error:
            (client.close() if client is not None else connection.close())
            raise SystemExit(f"QMP handshake failed: {error}") from error
    raise SystemExit(
        f"QMP never came up at {QMP} within "
        f"{QMP_CONNECT_TOTAL_TIMEOUT_SECONDS}s; last connection error: {last_error}"
    )

if QMP_IO_SELF_TEST:
    _qmp_io_self_test()
    started = time.monotonic()
    QMP_CONNECT_ATTEMPT_TIMEOUT_SECONDS = 0.05
    QMP_CONNECT_TOTAL_TIMEOUT_SECONDS = 0.05
    with tempfile.TemporaryDirectory(prefix="goblins-qmp-self-test.") as scratch:
        QMP = os.path.join(scratch, "missing.sock")
        _expect_qmp_failure(SystemExit, _conn)
    assert time.monotonic() - started < 0.5
    print("qmp_io_self_test=ok", flush=True)
    raise SystemExit(0)
if EVENT_RECEIVER_MODE:
    _run_event_receiver()
    raise SystemExit(0)

QMP_CLIENT = None if CAPTURE_CHANNEL_SELF_TEST else _conn()
def cmd(ex, **a):
    try:
        return QMP_CLIENT.execute(
            ex,
            a or None,
            timeout_seconds=_capture_bounded_timeout(
                QMP_COMMAND_TIMEOUT_SECONDS, f"QMP command {ex!r}"
            ),
        )
    except (QmpProtocolError, QmpTimeoutError, QmpCommandError) as error:
        QMP_CLIENT.close()
        raise SystemExit(f"QMP command {ex!r} failed closed: {error}") from error
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

def _parse_ppm_header(header):
    tokens = []
    index = 0
    token_end = None
    while index < len(header) and len(tokens) < 4:
        while index < len(header) and header[index] in b" \t\r\n":
            index += 1
        if index >= len(header):
            break
        if header[index] == ord("#"):
            newline = header.find(b"\n", index + 1)
            if newline < 0:
                break
            index = newline + 1
            continue
        start = index
        while index < len(header) and header[index] not in b" \t\r\n#":
            index += 1
        if start == index:
            raise ValueError("PPM header contains an invalid token")
        tokens.append(header[start:index])
        token_end = index
    if len(tokens) != 4 or token_end is None or token_end >= len(header):
        raise ValueError("PPM header ended before its dimensions")
    if header[token_end] not in b" \t\r\n":
        raise ValueError("PPM header is not terminated by whitespace")
    payload_offset = token_end + 1
    if header[token_end : token_end + 2] == b"\r\n":
        payload_offset += 1
    if tokens[0] != b"P6" or tokens[3] != b"255":
        raise ValueError("QMP screendump is not an 8-bit binary PPM")
    try:
        width = int(tokens[1])
        height = int(tokens[2])
    except ValueError as error:
        raise ValueError("QMP screendump has nonnumeric dimensions") from error
    if width <= 0 or height <= 0:
        raise ValueError("QMP screendump has invalid dimensions")
    if width * height > MAX_CAPTURE_PIXELS:
        raise ValueError("QMP screendump exceeds the fixed capture pixel limit")
    if width > CAPTURE_CANVAS_WIDTH or height > CAPTURE_CANVAS_HEIGHT:
        raise ValueError("QMP screendump exceeds the fixed evidence canvas")
    return width, height, payload_offset


def inspect_ppm(path, hash_name="sha256"):
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_nlink != 1
            or metadata.st_uid != os.getuid()
            or metadata.st_size <= 0
            or metadata.st_size > PPM_MAX_BYTES
        ):
            raise ValueError("QMP screendump is not a safe bounded regular file")
        header = os.read(descriptor, min(PPM_MAX_HEADER_BYTES, metadata.st_size))
        width, height, payload_offset = _parse_ppm_header(header)
        expected_size = payload_offset + width * height * 3
        if metadata.st_size != expected_size:
            raise ValueError("QMP screendump size does not match its dimensions")
        digest = hashlib.new(hash_name)
        os.lseek(descriptor, 0, os.SEEK_SET)
        total = 0
        while total < metadata.st_size:
            chunk = os.read(descriptor, min(65536, metadata.st_size - total))
            if not chunk:
                raise ValueError("QMP screendump ended during bounded hash read")
            digest.update(chunk)
            total += len(chunk)
        if os.read(descriptor, 1):
            raise ValueError("QMP screendump grew during bounded hash read")
        return (width, height), metadata.st_size, digest.hexdigest()
    finally:
        os.close(descriptor)


def reserve_ppm(label):
    if not WORKDIR:
        raise CaptureChannelError("private capture work directory is not configured")
    descriptor, path = tempfile.mkstemp(
        prefix=f"frame-{slug(label)[:32]}-", suffix=".ppm", dir=WORKDIR
    )
    try:
        os.fchmod(descriptor, 0o600)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return path

def fsync_directory(path):
    flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    directory_fd = os.open(path, flags)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)

def png(ppm, out):
    source_dimensions, _, _ = inspect_ppm(ppm)
    out_dir = os.path.dirname(out) or "."
    os.makedirs(out_dir, exist_ok=True)
    temporary = tempfile.NamedTemporaryFile(
        prefix=f".{os.path.basename(out)}.",
        suffix=".tmp.png",
        dir=out_dir,
        delete=False,
    )
    temporary_path = temporary.name
    temporary.close()
    try:
        if os.uname().sysname == "Darwin":
            _run_image_conversion(
                ["sips", "-s", "format", "png", ppm, "--out", temporary_path],
                _capture_bounded_timeout(
                    PNG_CONVERSION_TIMEOUT_SECONDS, "framebuffer PNG conversion"
                ),
            )
            _run_image_conversion(
                [
                    "sips",
                    "--padToHeightWidth",
                    str(CAPTURE_CANVAS_HEIGHT),
                    str(CAPTURE_CANVAS_WIDTH),
                    "--padColor",
                    "101216",
                    temporary_path,
                ],
                _capture_bounded_timeout(
                    PNG_CONVERSION_TIMEOUT_SECONDS, "framebuffer evidence padding"
                ),
            )
        else:
            _run_image_conversion(
                [
                    "convert",
                    ppm,
                    "-background",
                    "#101216",
                    "-gravity",
                    "center",
                    "-extent",
                    f"{CAPTURE_CANVAS_WIDTH}x{CAPTURE_CANVAS_HEIGHT}",
                    f"png:{temporary_path}",
                ],
                _capture_bounded_timeout(
                    PNG_CONVERSION_TIMEOUT_SECONDS, "framebuffer PNG conversion"
                ),
            )
        temporary_metadata = os.lstat(temporary_path)
        if (
            not stat.S_ISREG(temporary_metadata.st_mode)
            or temporary_metadata.st_nlink != 1
            or temporary_metadata.st_uid != os.getuid()
        ):
            raise ValueError("converted PNG is not a safe regular file")
        png_sha256, width, height = validate_png(
            temporary_path, CAPTURE_CANVAS_DIMENSIONS
        )
        descriptor = os.open(
            temporary_path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        )
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        os.replace(temporary_path, out)
        fsync_directory(out_dir)
        print(
            f"framebuffer evidence padded without resampling from "
            f"{source_dimensions[0]}x{source_dimensions[1]} to "
            f"{CAPTURE_CANVAS_WIDTH}x{CAPTURE_CANVAS_HEIGHT}",
            flush=True,
        )
        return png_sha256, width, height
    except (
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
        ValueError,
    ) as error:
        raise SystemExit(f"could not atomically create a validated PNG at {out}: {error}") from error
    finally:
        try:
            os.remove(temporary_path)
        except FileNotFoundError:
            pass
def fail(message, exit_code=1):
    print(message, flush=True)
    raise SystemExit(exit_code)


def safe_file_size(path, maximum):
    try:
        metadata = os.lstat(path)
    except FileNotFoundError:
        return 0
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_nlink != 1
        or metadata.st_uid != os.getuid()
        or metadata.st_size > maximum
    ):
        raise CaptureChannelError("capture log is not a safe bounded regular file")
    return metadata.st_size


def wait_serial_contains(
    label, needle, timeout, debug_label=None, debug_every=0, exit_code=1
):
    reader = IncrementalFileReader(SERIALLOG, SERIAL_MAX_BYTES)
    started = time.monotonic()
    last_debug = started
    try:
        while time.monotonic() - started < timeout:
            if reader.contains(needle):
                print(f"{label}: observed serial marker {needle!r}", flush=True)
                return True
            if (
                debug_label
                and debug_every
                and time.monotonic() - last_debug >= debug_every
            ):
                frame_sample(debug_label, save_debug=True)
                last_debug = time.monotonic()
            time.sleep(1)
        fail(
            f"{label} did not appear in serial log within {timeout}s; "
            f"expected {needle!r}; serial_tail={reader.diagnostic_tail()!r}",
            exit_code=exit_code,
        )
    finally:
        reader.close()


def observe_serial_contains(label, needle, timeout):
    reader = IncrementalFileReader(SERIALLOG, SERIAL_MAX_BYTES)
    started = time.monotonic()
    try:
        while time.monotonic() - started < timeout:
            if reader.contains(needle):
                print(f"{label}: observed serial marker {needle!r}", flush=True)
                return True
            time.sleep(1)
        print(
            f"{label}: serial marker {needle!r} not observed within {timeout}s; "
            f"serial_tail={reader.diagnostic_tail()!r}; continuing to framebuffer stages",
            flush=True,
        )
        return False
    finally:
        reader.close()


def serial_contains_now(needle):
    reader = IncrementalFileReader(SERIALLOG, SERIAL_MAX_BYTES)
    try:
        return reader.contains(needle)
    finally:
        reader.close()


def wait_helper_event(event_reader, helper_name, timeout):
    started = time.monotonic()
    while time.monotonic() - started < timeout:
        for event in event_reader.poll():
            if event.get("kind") == "helper" and event.get("name") == helper_name:
                print(f"authenticated helper download observed: {helper_name}", flush=True)
                return
        time.sleep(0.25)
    fail(
        f"authenticated helper download {helper_name!r} did not appear within "
        f"{timeout}s; event_tail={event_reader.diagnostic_tail()!r}"
    )


def wait_firstboot_unlock_result(timeout, event_reader, serial_start_pos):
    failure_serial_marker = "GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_FAILED"
    success_serial_marker = "GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE"
    serial_reader = IncrementalFileReader(
        SERIALLOG, SERIAL_MAX_BYTES, start_offset=serial_start_pos
    )
    started = time.monotonic()
    success_event_seen = False
    success_serial_seen = False
    try:
        while time.monotonic() - started < timeout:
            for event in event_reader.poll():
                if (
                    event.get("kind") == "failed"
                    and event.get("name") == "FIRSTBOOT_UNLOCK"
                ):
                    values = event.get("values", {})
                    fail(
                        "first boot release-proof unlock failed in the guest: "
                        f"stage={values.get('stage', 'missing')} "
                        f"rc={values.get('rc', 'missing')}"
                    )
                if (
                    event.get("kind") == "ready"
                    and event.get("name") == "FIRSTBOOT_UNLOCK"
                ):
                    success_event_seen = event.get("values") == {"status": "pass"}
            serial_chunk = serial_reader.read_available()
            serial_text = (serial_reader.overlap + serial_chunk).decode(
                "utf-8", errors="replace"
            )
            serial_reader.overlap = (serial_reader.overlap + serial_chunk)[
                -LOG_OVERLAP_BYTES:
            ]
            if failure_serial_marker in serial_text:
                fail(
                    "first boot release-proof unlock failed in the guest: "
                    f"{failure_serial_marker}"
                )
            if success_serial_marker in serial_text:
                success_serial_seen = True
            if success_event_seen and success_serial_seen:
                print(
                    "first boot release-proof unlock callback: observed authenticated "
                    "event and guest completion marker",
                    flush=True,
                )
                return True
            time.sleep(0.25)
        fail(
            "first boot release-proof unlock callback did not appear in authenticated "
            f"events and serial within {timeout}s; "
            f"event_tail={event_reader.diagnostic_tail()!r}; "
            f"serial_tail={serial_reader.diagnostic_tail()!r}"
        )
    finally:
        serial_reader.close()


def slug(label):
    value = "".join(ch.lower() if ch.isalnum() else "-" for ch in label).strip("-")
    return value or "stage"

def frame_sample(label, save_debug=False):
    path = reserve_ppm(label)
    try:
        dump(path)
        _, size, digest = inspect_ppm(path)
        sample = {"size": size, "sha256": digest[:16]}
        if save_debug:
            os.makedirs(OUTDIR, exist_ok=True)
            out = f"{OUTDIR}/_debug-{slug(label)}.png"
            png(path, out)
            print(f"{label}: debug framebuffer saved to {out}", flush=True)
        return sample
    except (OSError, ValueError) as err:
        return {"size": 0, "sha256": f"error:{err}"}
    finally:
        try:
            os.remove(path)
        except OSError:
            pass

def wait_stage(label, seconds, sample_every=30):
    """Wait a bounded stage interval while recording diagnostic-only frames.

    QEMU PPM byte size is resolution-driven on CI, not a reliable UI state
    detector. These samples are intentionally diagnostic-only; real progress is
    proven by serial markers and authenticated in-session events.
    """
    deadline = time.monotonic() + seconds
    samples = []
    while True:
        now = time.monotonic()
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

def complete_first_boot_setup(event_reader, serial_start_pos):
    """Wait for the verification-only user service to complete first boot."""
    print("first boot setup: completing offline path through the root release-proof capability", flush=True)
    frame_sample("first boot before release-proof unlock", save_debug=True)
    try:
        wait_helper_event(event_reader, "firstboot-unlock.sh", 180)
        wait_firstboot_unlock_result(180, event_reader, serial_start_pos)
    except SystemExit:
        print("first boot setup failed before helper callback; collecting VT diagnostics", flush=True)
        probe_graphical_vts()
        raise
    frame_sample("post first boot release-proof unlock", save_debug=True)

def publish_orchestrator():
    if not ORCHESTRATOR_SOURCE or not ORCHESTRATOR_DEST:
        raise SystemExit("missing GOS_ORCHESTRATOR_SOURCE/GOS_ORCHESTRATOR_DEST for verification service orchestration")
    source = _bounded_regular_file(
        ORCHESTRATOR_SOURCE, 4 * 1024 * 1024, expected_uid=os.getuid()
    )
    _create_private_leaf(ORCHESTRATOR_DEST, source, mode=0o600)
    print(f"in-session orchestrator published for verification user service: {ORCHESTRATOR_DEST}", flush=True)

def write_proof(event, proofs):
    name = event["name"]
    values = dict(event["values"])
    values.update({
        "name": name,
        "captured_via": "display-backed VM HTTP proof signal",
    })
    proofs[name] = values
    filename = PROOF_FILENAMES.get(name, f"{name}-proof.json")
    with open(f"{OUTDIR}/{filename}", "w", encoding="utf-8") as fh:
        json.dump(values, fh, indent=2, sort_keys=True)
        fh.write("\n")

def handle_input(event):
    values = event["values"]
    name = event["name"]
    if event["input_kind"] == "click":
        try:
            x = float(values.get("x", "0.5"))
            y = float(values.get("y", "0.5"))
        except ValueError as err:
            raise SystemExit(f"invalid click coordinate for input route {name}: {err}") from err
        print(f"input click {name}: x={x:.3f} y={y:.3f}", flush=True)
        click(x, y)
        return
    if event["input_kind"] == "text":
        text = values.get("text", "")
        print(f"input text {name}: {text!r}", flush=True)
        qmp_type_text(text)
        return
    if event["input_kind"] == "key":
        key_name = values.get("key", "")
        print(f"input key {name}: {key_name!r}", flush=True)
        qmp_press_key(key_name)
        return
    raise SystemExit(f"unsupported input event: {event!r}")

def capture_ready_frame(name, frame_hashes):
    if name not in EXPECTED_READY_SHOTS:
        raise SystemExit(f"refusing capture acknowledgement for unknown shot {name!r}")
    ppm = reserve_ppm(name)
    out = f"{OUTDIR}/{name}.png"
    deadline = time.monotonic() + REQUIRED_FRAME_SETTLE_SECONDS
    last_hash = None
    attempts = 0
    try:
        while True:
            attempts += 1
            dump(ppm)
            try:
                _, _, last_hash = inspect_ppm(ppm)
            except (OSError, ValueError):
                last_hash = None
            if not last_hash or last_hash not in frame_hashes:
                break
            if time.monotonic() >= deadline:
                print(
                    f"{name}: framebuffer stayed duplicate for "
                    f"{REQUIRED_FRAME_SETTLE_SECONDS}s; saving it so the signoff "
                    "guard can fail closed",
                    flush=True,
                )
                break
            time.sleep(1)
        if last_hash:
            frame_hashes.add(last_hash)
        png_sha256, png_width, png_height = png(ppm, out)
    finally:
        try:
            os.remove(ppm)
        except OSError:
            pass
    ack_directory_metadata = os.lstat(CAPTURE_ACK_DIR)
    if (
        not stat.S_ISDIR(ack_directory_metadata.st_mode)
        or ack_directory_metadata.st_uid != os.getuid()
        or stat.S_IMODE(ack_directory_metadata.st_mode) != 0o700
    ):
        raise CaptureChannelError("capture acknowledgement directory is not private")
    ack = os.path.join(CAPTURE_ACK_DIR, f"{name}.captured")
    temporary_ack = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            prefix=f".{name}.",
            suffix=".tmp",
            dir=CAPTURE_ACK_DIR,
            delete=False,
        ) as handle:
            temporary_ack = handle.name
            handle.write(f"frame-sha256={last_hash or 'missing'}\n")
            handle.write(f"png-sha256={png_sha256}\n")
            handle.write(f"png-width={png_width}\n")
            handle.write(f"png-height={png_height}\n")
            handle.flush()
            os.fsync(handle.fileno())
        try:
            existing = os.lstat(ack)
        except FileNotFoundError:
            existing = None
        if existing is not None and (
            not stat.S_ISREG(existing.st_mode)
            or existing.st_uid != os.getuid()
            or existing.st_nlink != 1
        ):
            raise CaptureChannelError("capture acknowledgement leaf is unsafe")
        os.replace(temporary_ack, ack)
        temporary_ack = None
        fsync_directory(CAPTURE_ACK_DIR)
    finally:
        if temporary_ack is not None:
            try:
                os.remove(temporary_ack)
            except FileNotFoundError:
                pass
    print(
        f"captured {name} after {attempts} framebuffer sample(s); "
        f"png_sha256={png_sha256} dimensions={png_width}x{png_height}",
        flush=True,
    )

def require_proofs(proofs):
    bad = [
        f"{name}={proofs.get(name, {}).get('status', 'missing')}"
        for name in REQUIRED_PROOFS
        if proofs.get(name, {}).get("status") != "pass"
    ]
    if bad:
        raise SystemExit("missing or failing required proof signals: " + ", ".join(bad))


def _raw_http_request(port, request):
    connection = socket.create_connection(("127.0.0.1", port), timeout=1)
    try:
        connection.sendall(request)
        try:
            connection.shutdown(socket.SHUT_WR)
        except OSError as error:
            # The bounded server may send its final error response and close
            # before this self-test client half-closes an oversized request.
            # Keep draining that response; every caller still validates the
            # exact HTTP status, so a missing or malformed reply fails closed.
            if error.errno not in (errno.ENOTCONN, errno.EPIPE):
                raise
        response = bytearray()
        while True:
            chunk = connection.recv(4096)
            if not chunk:
                break
            response.extend(chunk)
        return bytes(response)
    finally:
        connection.close()


def _response_status(response):
    return int(response.split(b" ", 2)[1])


def _capture_channel_self_test():
    token = "a" * 64
    with tempfile.TemporaryDirectory(prefix="goblins-capture-channel-test.") as scratch:
        os.chmod(scratch, 0o700)
        helper = os.path.join(scratch, "firstboot-unlock.sh")
        core_helper = os.path.join(scratch, "core-proof-operation.sh")
        orchestrator = os.path.join(scratch, "orchestrator.sh")
        ack_dir = os.path.join(scratch, "capture-acks")
        os.mkdir(ack_dir, 0o700)
        _create_private_leaf(
            os.path.join(ack_dir, "01-installer.captured"),
            b"png-sha256=" + b"0" * 64 + b"\npng-width=5120\npng-height=2880\n",
        )
        _create_private_leaf(helper, b"firstboot\n")
        _create_private_leaf(core_helper, b"core\n")
        _create_private_leaf(orchestrator, b"orchestrator\n")
        event_path = os.path.join(scratch, "events.jsonl")
        store = CaptureEventStore(event_path)
        receiver = CaptureReceiver(
            token,
            store,
            {
                "/firstboot-unlock.sh": helper,
                "/core-proof-operation.sh": core_helper,
                "/orchestrator.sh": orchestrator,
            },
            ack_dir,
        )
        server = CaptureHTTPServer(("127.0.0.1", 0), receiver)
        assert server.verify_request(None, ("127.0.0.1", 1))
        assert not server.verify_request(None, ("192.0.2.1", 1))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        port = server.server_address[1]
        authorization = f"Authorization: Bearer {token}\r\n".encode("ascii")
        try:
            valid_helper = (
                b"GET /firstboot-unlock.sh HTTP/1.1\r\nHost: 127.0.0.1\r\n"
                + authorization
                + b"\r\n"
            )
            response = _raw_http_request(port, valid_helper)
            assert _response_status(response) == 200 and response.endswith(b"firstboot\n")
            valid_ack = (
                b"GET /capture-acks/01-installer.captured HTTP/1.1\r\n"
                b"Host: 127.0.0.1\r\n"
                + authorization
                + b"\r\n"
            )
            response = _raw_http_request(port, valid_ack)
            assert _response_status(response) == 200 and b"png-width=5120" in response
            valid_event = (
                b"GET /ready/ORCH_START HTTP/1.1\r\nHost: 127.0.0.1\r\n"
                + authorization
                + b"\r\n"
            )
            assert _response_status(_raw_http_request(port, valid_event)) == 204
            bad_requests = (
                (
                    b"POST /ready/ORCH_START HTTP/1.1\r\nHost: localhost\r\n"
                    + authorization
                    + b"\r\n",
                    405,
                ),
                (
                    b"GET /ready/ORCH_START HTTP/1.1\r\nHost: localhost\r\n"
                    b"Authorization: Bearer " + b"b" * 64 + b"\r\n\r\n",
                    403,
                ),
                (
                    b"GET /ready/ORCH_START HTTP/1.1\r\nHost: localhost\r\n"
                    + authorization
                    + authorization
                    + b"\r\n",
                    400,
                ),
                (
                    b"GET /input/key/a?key=Escape&key=Return HTTP/1.1\r\n"
                    b"Host: localhost\r\n" + authorization + b"\r\n",
                    400,
                ),
                (
                    b"GET /input/key/a?key=%ZZ HTTP/1.1\r\nHost: localhost\r\n"
                    + authorization
                    + b"\r\n",
                    400,
                ),
                (
                    b"GET /unknown HTTP/1.1\r\nHost: localhost\r\n"
                    + authorization
                    + b"\r\n",
                    404,
                ),
                (
                    b"GET /ready/ORCH_START HTTP/1.1\r\nHost: localhost\r\n"
                    + authorization
                    + b"Content-Length: 1\r\n\r\nx",
                    400,
                ),
            )
            for request, expected_status in bad_requests:
                assert _response_status(_raw_http_request(port, request)) == expected_status
            oversized = b"GET /" + b"x" * HTTP_MAX_REQUEST_BYTES
            assert _response_status(_raw_http_request(port, oversized)) == 414
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
            store.close()
        event_reader = IncrementalEventReader(event_path)
        try:
            events = event_reader.poll()
            assert [(event["kind"], event["name"]) for event in events] == [
                ("helper", "firstboot-unlock.sh"),
                ("ready", "ORCH_START"),
            ]
            assert event_reader.poll() == []
        finally:
            event_reader.close()

        bounded_store_path = os.path.join(scratch, "bounded-events.jsonl")
        bounded_store = CaptureEventStore(bounded_store_path, maximum_bytes=32)
        try:
            try:
                bounded_store.append({"kind": "ready", "name": "ORCH_START", "values": {}})
            except CaptureChannelError:
                pass
            else:
                raise AssertionError("bounded event stream accepted an oversized record")
        finally:
            bounded_store.close()

        incremental_path = os.path.join(scratch, "incremental.log")
        _create_private_leaf(incremental_path, b"prefix-abc")
        incremental = IncrementalFileReader(incremental_path, 64)
        try:
            assert not incremental.contains("abcdef")
            with open(incremental_path, "ab", buffering=0) as handle:
                handle.write(b"def-suffix")
            assert incremental.contains("abcdef")
            with open(incremental_path, "ab", buffering=0) as handle:
                handle.write(b"x" * 64)
            try:
                incremental.read_available()
            except CaptureChannelError:
                pass
            else:
                raise AssertionError("incremental reader accepted excessive growth")
        finally:
            incremental.close()

        ppm_path = os.path.join(scratch, "valid.ppm")
        _create_private_leaf(ppm_path, b"P6\n2 1\n255\n" + b"\x00\x01\x02\x03\x04\x05")
        dimensions, size, digest = inspect_ppm(ppm_path)
        assert dimensions == (2, 1) and size == 17 and len(digest) == 64
        malformed_ppm = os.path.join(scratch, "malformed.ppm")
        _create_private_leaf(malformed_ppm, b"P6\n2 1\n255\n" + b"\x00")
        try:
            inspect_ppm(malformed_ppm)
        except ValueError:
            pass
        else:
            raise AssertionError("PPM bounds accepted a truncated framebuffer")
        excessive_header_ppm = os.path.join(scratch, "excessive-header.ppm")
        _create_private_leaf(
            excessive_header_ppm, b"P6\n" + b"9" * (PPM_MAX_HEADER_BYTES + 1)
        )
        try:
            inspect_ppm(excessive_header_ppm)
        except ValueError:
            pass
        else:
            raise AssertionError("PPM bounds accepted an excessive header")
        symlink_ppm = os.path.join(scratch, "symlink.ppm")
        os.symlink(ppm_path, symlink_ppm)
        try:
            inspect_ppm(symlink_ppm)
        except OSError as error:
            assert error.errno in (errno.ELOOP, errno.EMLINK)
        else:
            raise AssertionError("PPM reader followed a symlink")
    print("capture_channel_self_test=ok", flush=True)


if CAPTURE_CHANNEL_SELF_TEST:
    _capture_channel_self_test()
    raise SystemExit(0)


# 0. Boot the highlighted installer entry instead of burning the GRUB timeout.
event_reader = IncrementalEventReader(EVENTS)
firstboot_serial_start_pos = safe_file_size(SERIALLOG, SERIAL_MAX_BYTES)
print(f"QMP display input route: {DISPLAY_DEVICE or 'default'}", flush=True)
print(f"QMP query-mice: {try_cmd('query-mice')}", flush=True)
if not SKIP_INSTALL_PHASE:
    wait_serial_contains("ISO boot menu", "Install Goblins OS 44", 180)
    if not serial_contains_now("Booting `Install Goblins OS 44'"):
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
    if INSTALL_MARKER_EXIT_CODE:
        print("kickstart install post: returning to host for disk-only first boot", flush=True)
        raise SystemExit(INSTALL_MARKER_EXIT_CODE)
else:
    print("install phase skipped by host; waiting for installed first boot", flush=True)
# 2. Wait for first boot before treating install progress as real.
observe_serial_contains("first boot hardware diagnostics", "GOBLINS_HWGATE_DIAG_DONE", 180)
wait_stage("first boot desktop", 420)
observe_serial_contains(
    "session orchestrator starter",
    "GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED",
    5,
)
# 3. complete first boot through the real offline/private core contracts.
complete_first_boot_setup(event_reader, firstboot_serial_start_pos)
# Capture the genuine post-contract framebuffer directly from the host before
# the session orchestrator can launch, close, or replace any visible surface.
time.sleep(3)
frame_hashes = set()
capture_ready_frame("05-first-boot-private-unlock", frame_hashes)
# 4. publish orchestrator only after the host is ready to consume its events.
os.makedirs(OUTDIR, exist_ok=True)
publish_orchestrator()
wait_helper_event(event_reader, "orchestrator.sh", 180)
# 5. capture on signals. The VM can legitimately spend more than ten minutes
# moving through proof windows, but only while it is still producing fresh proof,
# input, or ready events. Fail closed on inactivity so a stuck guest cannot spin
# forever.
seen = {"05-first-boot-private-unlock"}
proofs = {}
capture_started = time.monotonic()
capture_deadline = capture_started + CAPTURE_TOTAL_TIMEOUT_SECONDS
last_progress = capture_started
timeout_reason = "total"
print(
    "capture signal timeouts: "
    f"total={CAPTURE_TOTAL_TIMEOUT_SECONDS}s "
    f"inactivity={CAPTURE_INACTIVITY_TIMEOUT_SECONDS}s",
    flush=True,
)
while True:
    now = time.monotonic()
    if now >= capture_deadline:
        timeout_reason = "total"
        break
    if now - last_progress >= CAPTURE_INACTIVITY_TIMEOUT_SECONDS:
        timeout_reason = "inactivity"
        break
    ACTIVE_CAPTURE_DEADLINE = min(
        capture_deadline,
        last_progress + CAPTURE_INACTIVITY_TIMEOUT_SECONDS,
    )
    for event in event_reader.poll():
        kind = event.get("kind")
        if kind == "input":
            last_progress = time.monotonic()
            ACTIVE_CAPTURE_DEADLINE = min(
                capture_deadline,
                last_progress + CAPTURE_INACTIVITY_TIMEOUT_SECONDS,
            )
            handle_input(event)
            last_progress = time.monotonic()
            continue
        if kind == "proof":
            write_proof(event, proofs)
            name = event["name"]
            print(f"proof {name}={proofs[name].get('status', 'unknown')}", flush=True)
            last_progress = time.monotonic()
            continue
        if kind == "failed":
            raise SystemExit(f"unexpected authenticated failure event: {event!r}")
        if kind == "ready":
            name = event["name"]
            if name == "ORCH_ALLDONE":
                require_proofs(proofs)
                print("ORCH_ALLDONE", flush=True)
                raise SystemExit(0)
            if name and name not in seen and name not in ("ORCH_START", "FIRSTBOOT_UNLOCK"):
                seen.add(name)
                last_progress = time.monotonic()
                ACTIVE_CAPTURE_DEADLINE = min(
                    capture_deadline,
                    last_progress + CAPTURE_INACTIVITY_TIMEOUT_SECONDS,
                )
                capture_ready_frame(name, frame_hashes)
                print(f"captured {name} ({len(seen)})", flush=True)
                last_progress = time.monotonic()
    time.sleep(0.3)
missing = [f"{shot}.png" for shot in EXPECTED_READY_SHOTS if shot not in seen]
print(
    f"timeout reason={timeout_reason}; captured {len(seen)}; "
    f"missing={','.join(missing) if missing else 'none'}; "
    f"seconds_since_progress={int(time.monotonic() - last_progress)}",
    flush=True,
)
require_proofs(proofs)
raise SystemExit(1)
