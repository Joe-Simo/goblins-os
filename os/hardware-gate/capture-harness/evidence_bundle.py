#!/usr/bin/env python3
"""Create and verify canonical hardware-gate evidence bundles.

The bundle is deliberately a manifest rather than an archive.  Capture output
stays reviewable in its normal run directory, while this file binds every
required screenshot and proof to exact bytes and one uniform framebuffer.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import datetime as dt
import hashlib
import json
import os
import platform
import re
import secrets
import ssl
import stat
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path, PurePath
from typing import Final

from png_validation import MAX_CAPTURE_PNG_BYTES, validate_png_bytes


SCHEMA: Final = "goblins-os-hardware-evidence-bundle-v5"
ATTESTATION_SCHEMA: Final = "goblins-os-aarch64-local-display-authority-v2"
SEAL_NAME: Final = "evidence-bundle.json"
ATTESTATION_NAME: Final = "aarch64-local-display-attestation.json"
ATTESTATION_SIGNATURE_NAME: Final = "aarch64-local-display-attestation.json.cms"
AUTHORITY_PURPOSE: Final = "goblins-os-display-proof-authority-v2"
AUTHORITY_GENERATION: Final = 2
AUTHORITY_COMMON_NAME: Final = "Goblins OS Display Proof Authority 2"
AUTHORITY_CERTIFICATE_NAME: Final = "display-proof-authority2.pem"
AUTHORITY_FINGERPRINT_NAME: Final = "display-proof-authority2.sha256"
AUTHORITY_CA_CERTIFICATE_NAME: Final = "display-proof-authority2-ca.pem"
AUTHORITY_CA_FINGERPRINT_NAME: Final = "display-proof-authority2-ca.sha256"
MAX_JSON_BYTES: Final = 16 * 1024 * 1024
MAX_SEAL_BYTES: Final = 256 * 1024
MAX_ATTESTATION_INPUT_BYTES: Final = 512 * 1024
MAX_SIGNATURE_BYTES: Final = 256 * 1024
MIN_FRAMEBUFFER_WIDTH: Final = 1024
MAX_FRAMEBUFFER_WIDTH: Final = 7680
MIN_FRAMEBUFFER_HEIGHT: Final = 720
MAX_FRAMEBUFFER_HEIGHT: Final = 4320
COMMIT_RE: Final = re.compile(r"[0-9a-f]{40}\Z")
SHA256_RE: Final = re.compile(r"[0-9a-f]{64}\Z")
IMAGE_RE: Final = re.compile(
    r"ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}\Z"
)
QEMU_VERSION_RE: Final = re.compile(r"QEMU emulator version [^\x00-\x1f\x7f]{1,192}\Z")
CAPTURE_ENVIRONMENT_FIXED_VALUES: Final[dict[str, str]] = {
    "host_os": "Darwin",
    "host_architecture": "arm64",
    "accelerator": "hvf",
    "qemu_binary": "qemu-system-aarch64",
    "qemu_machine": "virt,accel=hvf,gic-version=max",
    "qemu_cpu": "host",
}

REQUIRED_PNGS: Final = (
    "01-installer.png",
    "02-install-network.png",
    "03-login.png",
    "04-desktop.png",
    "05-first-boot-private-unlock.png",
    "06-onboarding.png",
    "07-home.png",
    "08-shell-home.png",
    "09-shell-dark.png",
    "10-settings.png",
    "11-settings-models.png",
    "12-settings-dark.png",
    "13-studio-before.png",
    "14-studio-running.png",
    "15-studio-app-detail.png",
    "16-built-app-open.png",
    "17-dark-motion.png",
    "18-light-motion.png",
    "19-vulkan-vkcube.png",
    "20-gamemode-active.png",
    "21-gamescope-session.png",
    "22-mangohud-overlay.png",
    "23-controller-detection.png",
    "24-audio-output.png",
    "25-install-destination.png",
    "26-install-storage-summary.png",
    "27-dual-boot-preserve-existing-os.png",
    "28-bootloader-efi-summary.png",
    "29-preview-pdf-open.png",
    "30-preview-image-open.png",
    "31-text-shortcuts-candidate-bubble-render.png",
    "32-text-shortcuts-live-ibus-runtime-render.png",
    "33-accessibility-text-scaling.png",
    "34-accessibility-high-contrast.png",
    "35-accessibility-reduced-transparency.png",
    "36-accessibility-reduced-motion.png",
    "37-accessibility-localization-expansion.png",
    "38-accessibility-orca-atspi.png",
    "39-accessibility-keyboard-focus.png",
    "40-accessibility-window-resize.png",
    "41-hosted-context-review.png",
    "42-hosted-context-review-dark.png",
)

REQUIRED_PROOFS: Final = (
    "firewall-live-toggle-proof.json",
    "text-shortcuts-session-enable-proof.json",
    "text-shortcuts-candidate-metadata-proof.json",
    "text-shortcuts-overlay-intent-proof.json",
    "text-shortcuts-candidate-bubble-frame-proof.json",
    "text-shortcuts-candidate-bubble-layout-proof.json",
    "text-shortcuts-candidate-bubble-render-intent-proof.json",
    "text-shortcuts-candidate-bubble-render-proof.json",
    "text-shortcuts-live-ibus-runtime-render-proof.json",
    "keyboard-shortcuts-roundtrip-proof.json",
    "input-sources-roundtrip-proof.json",
    "multi-display-apply-proof.json",
    "focus-arm-roundtrip-proof.json",
    "app-privacy-revoke-proof.json",
    "preview-open-render-proof.json",
    "audio-output-proof.json",
    "runtime-build-proof.json",
    "accessibility-adaptivity-proof.json",
)

COPIED_VERIFICATION_FILES: Final = (
    "proof-manifest.json",
    "verification-iso-manifest.json",
    "verification-bib-manifest.json",
    "verification-release-evidence-manifest.json",
)


class EvidenceError(ValueError):
    pass


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise EvidenceError("JSON contains a duplicate object key")
        result[key] = value
    return result


def reject_constant(_: str) -> None:
    raise EvidenceError("JSON contains a non-finite number")


def parse_json_bytes(data: bytes, *, maximum: int, label: str) -> object:
    if not data or len(data) > maximum:
        raise EvidenceError(f"{label} is empty or exceeds its fixed byte limit")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{label} is not UTF-8 JSON") from error
    try:
        return json.loads(
            text,
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=reject_constant,
        )
    except (json.JSONDecodeError, RecursionError) as error:
        raise EvidenceError(f"{label} is not valid bounded JSON") from error


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(
            value,
            ensure_ascii=True,
            allow_nan=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8")


def require_exact_keys(value: object, expected: set[str], label: str) -> dict[str, object]:
    if type(value) is not dict or set(value) != expected:
        raise EvidenceError(f"{label} does not have the exact key set")
    return value


def validate_date(value: str) -> str:
    if type(value) is not str:
        raise EvidenceError("run date must be a string")
    try:
        parsed = dt.date.fromisoformat(value)
    except ValueError as error:
        raise EvidenceError("run date must be a real YYYY-MM-DD date") from error
    if parsed.isoformat() != value:
        raise EvidenceError("run date must use canonical YYYY-MM-DD form")
    return value


def validate_capture_environment(value: object) -> dict[str, object]:
    expected_keys = set(CAPTURE_ENVIRONMENT_FIXED_VALUES) | {
        "qemu_binary_sha256",
        "qemu_version",
    }
    environment = require_exact_keys(value, expected_keys, "capture environment")
    if not all(
        type(key) is str and type(item) is str
        for key, item in environment.items()
    ):
        raise EvidenceError("capture environment fields must be strings")
    for key, expected in CAPTURE_ENVIRONMENT_FIXED_VALUES.items():
        if environment[key] != expected:
            raise EvidenceError(f"unexpected capture environment value for {key}")
    if not SHA256_RE.fullmatch(environment["qemu_binary_sha256"]):
        raise EvidenceError("capture QEMU executable SHA256 is invalid")
    if not QEMU_VERSION_RE.fullmatch(environment["qemu_version"]):
        raise EvidenceError("capture QEMU version is invalid")
    return environment


def validate_metadata(
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    capture_workflow_run: str,
    capture_workflow_run_attempt: int,
    capture_environment: object,
) -> str:
    if architecture != "aarch64":
        raise EvidenceError("architecture must be canonical aarch64")
    if type(candidate_commit) is not str or not COMMIT_RE.fullmatch(candidate_commit):
        raise EvidenceError("candidate commit must be exactly 40 lowercase hexadecimal characters")
    if type(image_ref) is not str or not IMAGE_RE.fullmatch(image_ref):
        raise EvidenceError("image reference must be the exact digest-pinned Goblins OS package")
    validate_date(run_date)
    if type(capture_workflow_run_attempt) is not int:
        raise EvidenceError("capture workflow attempt must be an integer")
    if capture_workflow_run != "" or capture_workflow_run_attempt != 0:
        raise EvidenceError("local aarch64/HVF evidence must not claim a GitHub capture run")
    validate_capture_environment(capture_environment)
    return "local-aarch64-hvf"


def expected_names(architecture: str) -> tuple[str, ...]:
    if architecture != "aarch64":
        raise EvidenceError("architecture must be canonical aarch64")
    return REQUIRED_PNGS + REQUIRED_PROOFS + COPIED_VERIFICATION_FILES + (
        "native-packaging-gate.json",
    )


def expected_relative_run_dir(architecture: str, run_date: str) -> str:
    return f"os/screenshots/hardware-gate/{architecture}/{run_date}"


def secure_run_directory(
    repository: str, run_dir: str, architecture: str, run_date: str
) -> tuple[str, int]:
    if ".." in PurePath(run_dir).parts:
        raise EvidenceError("run directory may not contain parent traversal")
    repository_abs = os.path.abspath(repository)
    expected = os.path.join(
        repository_abs, "os", "screenshots", "hardware-gate", architecture, run_date
    )
    run_abs = os.path.abspath(os.path.join(repository_abs, run_dir)) if not os.path.isabs(run_dir) else os.path.abspath(run_dir)
    if run_abs != expected or os.path.commonpath((repository_abs, run_abs)) != repository_abs:
        raise EvidenceError("run directory is not the exact architecture/date directory")
    current = repository_abs
    for segment in ("os", "screenshots", "hardware-gate", architecture, run_date):
        current = os.path.join(current, segment)
        try:
            metadata = os.lstat(current)
        except OSError as error:
            raise EvidenceError("run directory is missing") from error
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
            raise EvidenceError("run directory path contains a symlink or non-directory")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        directory_fd = os.open(run_abs, flags)
    except OSError as error:
        raise EvidenceError("run directory could not be opened safely") from error
    opened = os.fstat(directory_fd)
    if (
        not stat.S_ISDIR(opened.st_mode)
        or (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino)
    ):
        os.close(directory_fd)
        raise EvidenceError("run directory changed during safe open")
    return run_abs, directory_fd


def read_regular_at(directory_fd: int, name: str, maximum: int) -> bytes:
    if PurePath(name).name != name or name in {"", ".", ".."}:
        raise EvidenceError("evidence entry path is not a direct safe filename")
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(name, flags, dir_fd=directory_fd)
    except OSError as error:
        raise EvidenceError(f"required evidence file is missing or unsafe: {name}") from error
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise EvidenceError(f"evidence file is not a single-link regular file: {name}")
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, maximum + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if total > maximum:
                raise EvidenceError(f"evidence file exceeds its fixed byte limit: {name}")
        after = os.fstat(descriptor)
        stable = (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_nlink,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) == (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if not stable or total != after.st_size:
            raise EvidenceError(f"evidence file changed while it was being sealed: {name}")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def atomic_write_at(directory_fd: int, destination: str, data: bytes) -> None:
    if PurePath(destination).name != destination:
        raise EvidenceError("seal output must be a direct filename in the run directory")
    try:
        existing = os.stat(destination, dir_fd=directory_fd, follow_symlinks=False)
    except FileNotFoundError:
        existing = None
    if existing is not None and (
        not stat.S_ISREG(existing.st_mode)
        or stat.S_ISLNK(existing.st_mode)
        or existing.st_nlink != 1
    ):
        raise EvidenceError("existing seal output is not a single-link regular file")
    temporary = f".{destination}.tmp-{secrets.token_hex(12)}"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(temporary, flags, 0o600, dir_fd=directory_fd)
    try:
        view = memoryview(data)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise EvidenceError("short write while creating evidence output")
            view = view[written:]
        os.fsync(descriptor)
    except BaseException:
        try:
            os.unlink(temporary, dir_fd=directory_fd)
        except OSError:
            pass
        raise
    finally:
        os.close(descriptor)
    try:
        os.replace(
            temporary,
            destination,
            src_dir_fd=directory_fd,
            dst_dir_fd=directory_fd,
        )
        os.fsync(directory_fd)
    except BaseException:
        try:
            os.unlink(temporary, dir_fd=directory_fd)
        except OSError:
            pass
        raise


def manifest_metadata(data: bytes) -> dict[str, object]:
    value = parse_json_bytes(data, maximum=MAX_JSON_BYTES, label="proof manifest")
    if type(value) is not dict:
        raise EvidenceError("proof manifest must be a JSON object")
    return value


def require_manifest_binding(
    manifest: dict[str, object],
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    capture_workflow_run: str,
    capture_workflow_run_attempt: int,
) -> tuple[dict[str, object], str]:
    expected = {
        "architecture": architecture,
        "candidate_commit": candidate_commit,
        "image_ref": image_ref,
        "captured_at": f"{run_date}T00:00:00Z",
        "screenshot_run_dir": expected_relative_run_dir(architecture, run_date),
        "capture_workflow_run": capture_workflow_run,
        "capture_workflow_run_attempt": capture_workflow_run_attempt,
    }
    if any(manifest.get(key) != value for key, value in expected.items()):
        raise EvidenceError("proof manifest does not match the requested evidence identity")
    iso_sha256 = manifest.get("iso_sha256")
    if type(iso_sha256) is not str or not SHA256_RE.fullmatch(iso_sha256):
        raise EvidenceError("proof manifest does not bind a canonical verification ISO SHA256")
    return validate_capture_environment(manifest.get("capture_environment")), iso_sha256


def entry_for_data(
    name: str, data: bytes, expected_dimensions: tuple[int, int] | None
) -> tuple[dict[str, object], tuple[int, int] | None]:
    if name.endswith(".png"):
        try:
            digest, width, height = validate_png_bytes(data, expected_dimensions)
        except (ValueError, struct.error) as error:
            raise EvidenceError(f"required screenshot is not a valid uniform PNG: {name}") from error
        if not (
            MIN_FRAMEBUFFER_WIDTH <= width <= MAX_FRAMEBUFFER_WIDTH
            and MIN_FRAMEBUFFER_HEIGHT <= height <= MAX_FRAMEBUFFER_HEIGHT
        ):
            raise EvidenceError("required screenshots do not have realistic framebuffer dimensions")
        return (
            {
                "height": height,
                "kind": "png",
                "path": name,
                "sha256": digest,
                "size": len(data),
                "width": width,
            },
            (width, height),
        )
    value = parse_json_bytes(data, maximum=MAX_JSON_BYTES, label=name)
    if type(value) is not dict:
        raise EvidenceError(f"required JSON evidence must be an object: {name}")
    return (
        {
            "kind": "json",
            "path": name,
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": len(data),
        },
        expected_dimensions,
    )


def entry_for_file(
    directory_fd: int, name: str, expected_dimensions: tuple[int, int] | None
) -> tuple[dict[str, object], tuple[int, int] | None]:
    maximum = MAX_CAPTURE_PNG_BYTES if name.endswith(".png") else MAX_JSON_BYTES
    return entry_for_data(
        name, read_regular_at(directory_fd, name, maximum), expected_dimensions
    )


def build_seal(
    directory_fd: int,
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    capture_workflow_run: str,
    capture_workflow_run_attempt: int,
) -> dict[str, object]:
    proof_manifest_data = read_regular_at(
        directory_fd, "proof-manifest.json", MAX_JSON_BYTES
    )
    capture_environment, verification_iso_sha256 = require_manifest_binding(
        manifest_metadata(proof_manifest_data),
        architecture,
        candidate_commit,
        image_ref,
        run_date,
        capture_workflow_run,
        capture_workflow_run_attempt,
    )
    capture_route = validate_metadata(
        architecture,
        candidate_commit,
        image_ref,
        run_date,
        capture_workflow_run,
        capture_workflow_run_attempt,
        capture_environment,
    )
    entries: list[dict[str, object]] = []
    dimensions: tuple[int, int] | None = None
    for name in expected_names(architecture):
        if name == "proof-manifest.json":
            entry, observed_dimensions = entry_for_data(
                name, proof_manifest_data, dimensions
            )
        else:
            entry, observed_dimensions = entry_for_file(directory_fd, name, dimensions)
        entries.append(entry)
        if dimensions is None and observed_dimensions is not None:
            dimensions = observed_dimensions
    if dimensions is None or len(REQUIRED_PNGS) != 42:
        raise EvidenceError("the v5 evidence contract must contain exactly 42 screenshots")
    width, height = dimensions
    screenshot_entries = [entry for entry in entries if entry["kind"] == "png"]
    screenshot_manifest_sha256 = hashlib.sha256(
        canonical_json(screenshot_entries)
    ).hexdigest()
    return {
        "architecture": architecture,
        "candidate_commit": candidate_commit,
        "capture_environment": capture_environment,
        "capture_route": capture_route,
        "capture_workflow_run": capture_workflow_run,
        "capture_workflow_run_attempt": capture_workflow_run_attempt,
        "entries": entries,
        "entry_count": len(entries),
        "framebuffer": {
            "height": height,
            "required_png_count": len(REQUIRED_PNGS),
            "width": width,
        },
        "image_ref": image_ref,
        "run_date": run_date,
        "schema": SCHEMA,
        "screenshot_manifest_sha256": screenshot_manifest_sha256,
        "verification_iso_sha256": verification_iso_sha256,
    }


def validate_seal_structure(
    data: bytes,
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
) -> dict[str, object]:
    value = parse_json_bytes(data, maximum=MAX_SEAL_BYTES, label="evidence bundle")
    seal = require_exact_keys(
        value,
        {
            "architecture",
            "candidate_commit",
            "capture_environment",
            "capture_route",
            "capture_workflow_run",
            "capture_workflow_run_attempt",
            "entries",
            "entry_count",
            "framebuffer",
            "image_ref",
            "run_date",
            "schema",
            "screenshot_manifest_sha256",
            "verification_iso_sha256",
        },
        "evidence bundle",
    )
    if canonical_json(seal) != data:
        raise EvidenceError("evidence bundle is not encoded in canonical JSON form")
    if seal["schema"] != SCHEMA:
        raise EvidenceError("evidence bundle schema is not supported")
    if type(seal["verification_iso_sha256"]) is not str or not SHA256_RE.fullmatch(
        seal["verification_iso_sha256"]
    ):
        raise EvidenceError("evidence bundle verification ISO SHA256 is invalid")
    if type(seal["screenshot_manifest_sha256"]) is not str or not SHA256_RE.fullmatch(
        seal["screenshot_manifest_sha256"]
    ):
        raise EvidenceError("evidence bundle screenshot manifest SHA256 is invalid")
    if any(
        seal[key] != expected
        for key, expected in {
            "architecture": architecture,
            "candidate_commit": candidate_commit,
            "image_ref": image_ref,
            "run_date": run_date,
        }.items()
    ):
        raise EvidenceError("evidence bundle identity does not match expected values")
    expected_route = validate_metadata(
        architecture,
        candidate_commit,
        image_ref,
        run_date,
        seal["capture_workflow_run"],
        seal["capture_workflow_run_attempt"],
        seal["capture_environment"],
    )
    if seal["capture_route"] != expected_route:
        raise EvidenceError("evidence bundle capture route is inconsistent")
    framebuffer = require_exact_keys(
        seal["framebuffer"], {"height", "required_png_count", "width"}, "framebuffer"
    )
    width = framebuffer["width"]
    height = framebuffer["height"]
    if (
        type(width) is not int
        or type(height) is not int
        or type(framebuffer["required_png_count"]) is not int
        or framebuffer["required_png_count"] != 42
        or not (MIN_FRAMEBUFFER_WIDTH <= width <= MAX_FRAMEBUFFER_WIDTH)
        or not (MIN_FRAMEBUFFER_HEIGHT <= height <= MAX_FRAMEBUFFER_HEIGHT)
    ):
        raise EvidenceError("evidence bundle framebuffer contract is invalid")
    entries = seal["entries"]
    expected = expected_names(architecture)
    if type(entries) is not list or type(seal["entry_count"]) is not int:
        raise EvidenceError("evidence bundle entries must be a list with an integer count")
    if len(entries) != len(expected) or seal["entry_count"] != len(expected):
        raise EvidenceError("evidence bundle does not enumerate the exact required file set")
    paths: list[str] = []
    for index, (entry_value, expected_path) in enumerate(zip(entries, expected, strict=True)):
        expected_keys = {"kind", "path", "sha256", "size"}
        if expected_path.endswith(".png"):
            expected_keys |= {"height", "width"}
        entry = require_exact_keys(entry_value, expected_keys, f"evidence entry {index}")
        if entry["path"] != expected_path or PurePath(expected_path).name != expected_path:
            raise EvidenceError("evidence entries are not in the exact required order")
        paths.append(expected_path)
        expected_kind = "png" if expected_path.endswith(".png") else "json"
        if entry["kind"] != expected_kind:
            raise EvidenceError("evidence entry has the wrong kind")
        if type(entry["sha256"]) is not str or not SHA256_RE.fullmatch(entry["sha256"]):
            raise EvidenceError("evidence entry has an invalid SHA256")
        if type(entry["size"]) is not int or entry["size"] <= 0:
            raise EvidenceError("evidence entry has an invalid byte size")
        maximum_size = MAX_CAPTURE_PNG_BYTES if expected_kind == "png" else MAX_JSON_BYTES
        if entry["size"] > maximum_size:
            raise EvidenceError("evidence entry claims an excessive byte size")
        if expected_kind == "png" and (entry["width"], entry["height"]) != (width, height):
            raise EvidenceError("evidence screenshots do not share one framebuffer size")
    if len(paths) != len(set(paths)):
        raise EvidenceError("evidence bundle contains duplicate entry paths")
    screenshot_entries = [entry for entry in entries if entry["kind"] == "png"]
    if len(screenshot_entries) != len(REQUIRED_PNGS):
        raise EvidenceError("evidence bundle does not contain the complete screenshot manifest")
    if hashlib.sha256(canonical_json(screenshot_entries)).hexdigest() != seal[
        "screenshot_manifest_sha256"
    ]:
        raise EvidenceError("evidence bundle screenshot manifest digest is inconsistent")
    return seal


def create_bundle(args: argparse.Namespace) -> int:
    run_abs, directory_fd = secure_run_directory(
        args.repository, args.run_dir, args.architecture, args.run_date
    )
    try:
        expected_output = os.path.join(run_abs, SEAL_NAME)
        if os.path.abspath(args.output or expected_output) != expected_output:
            raise EvidenceError(f"evidence bundle output must be {expected_output}")
        seal = build_seal(
            directory_fd,
            args.architecture,
            args.candidate_commit,
            args.image_ref,
            args.run_date,
            args.capture_workflow_run,
            args.capture_workflow_run_attempt,
        )
        data = canonical_json(seal)
        validate_seal_structure(
            data,
            args.architecture,
            args.candidate_commit,
            args.image_ref,
            args.run_date,
        )
        atomic_write_at(directory_fd, SEAL_NAME, data)
    finally:
        os.close(directory_fd)
    print(hashlib.sha256(data).hexdigest())
    return 0


def verify_bundle(args: argparse.Namespace) -> int:
    _, directory_fd = secure_run_directory(
        args.repository, args.run_dir, args.architecture, args.run_date
    )
    try:
        recorded = read_regular_at(directory_fd, SEAL_NAME, MAX_SEAL_BYTES)
        seal = validate_seal_structure(
            recorded,
            args.architecture,
            args.candidate_commit,
            args.image_ref,
            args.run_date,
        )
        expected = build_seal(
            directory_fd,
            args.architecture,
            args.candidate_commit,
            args.image_ref,
            args.run_date,
            seal["capture_workflow_run"],
            seal["capture_workflow_run_attempt"],
        )
        if canonical_json(expected) != recorded:
            raise EvidenceError("evidence bundle no longer matches the required files")
    finally:
        os.close(directory_fd)
    print(hashlib.sha256(recorded).hexdigest())
    return 0


def read_direct_regular(path: str, maximum: int, label: str) -> bytes:
    try:
        metadata = os.lstat(path)
    except OSError as error:
        raise EvidenceError(f"{label} is missing") from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise EvidenceError(f"{label} is not a single-link regular file")
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags)
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_nlink != 1
            or (before.st_dev, before.st_ino) != (metadata.st_dev, metadata.st_ino)
        ):
            raise EvidenceError(f"{label} changed before it could be read safely")
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, maximum + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if total > maximum:
                raise EvidenceError(f"{label} exceeds its fixed byte limit")
        after = os.fstat(descriptor)
        stable = (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_nlink,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) == (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if not stable or after.st_size != total:
            raise EvidenceError(f"{label} changed while it was read")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def inspect_bundle(args: argparse.Namespace) -> int:
    data = read_direct_regular(args.seal, MAX_SEAL_BYTES, "evidence bundle")
    validate_seal_structure(
        data, args.architecture, args.candidate_commit, args.image_ref, args.run_date
    )
    print(hashlib.sha256(data).hexdigest())
    return 0


def verify_authority_certificate(args: argparse.Namespace) -> int:
    _, fingerprint, _, ca_fingerprint = read_authority_certificates(
        args.certificate,
        args.certificate_sha256,
        args.ca_certificate,
        args.ca_certificate_sha256,
    )
    print(f"{fingerprint} {ca_fingerprint}")
    return 0


def write_direct_atomic(path: str, data: bytes) -> None:
    parent = os.path.abspath(os.path.dirname(path) or ".")
    os.makedirs(parent, mode=0o700, exist_ok=True)
    metadata = os.lstat(parent)
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise EvidenceError("attestation output directory is unsafe")
    directory_fd = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        atomic_write_at(directory_fd, os.path.basename(path), data)
    finally:
        os.close(directory_fd)


def read_authority_certificate(
    certificate_path: str, fingerprint_path: str, *, label: str = "display-proof authority"
) -> tuple[bytes, str]:
    certificate_data = read_direct_regular(
        certificate_path, 64 * 1024, f"{label} certificate"
    )
    try:
        certificate_text = certificate_data.decode("ascii")
        der = ssl.PEM_cert_to_DER_cert(certificate_text)
    except (UnicodeDecodeError, ValueError) as error:
        raise EvidenceError(
            f"{label} certificate is not one PEM certificate"
        ) from error
    if certificate_text.count("-----BEGIN CERTIFICATE-----") != 1 or certificate_text.count(
        "-----END CERTIFICATE-----"
    ) != 1:
        raise EvidenceError(
            f"{label} certificate must contain exactly one certificate"
        )
    if certificate_text != ssl.DER_cert_to_PEM_cert(der):
        raise EvidenceError(
            f"{label} certificate is not canonical certificate-only PEM"
        )
    actual_fingerprint = hashlib.sha256(der).hexdigest()
    fingerprint_data = read_direct_regular(
        fingerprint_path, 256, f"{label} certificate fingerprint"
    )
    try:
        expected_fingerprint = fingerprint_data.decode("ascii").strip()
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{label} fingerprint is not ASCII") from error
    if not SHA256_RE.fullmatch(expected_fingerprint):
        raise EvidenceError(f"{label} fingerprint is not canonical SHA256")
    if expected_fingerprint != actual_fingerprint:
        raise EvidenceError(
            f"{label} certificate does not match its pinned fingerprint"
        )
    return certificate_data, actual_fingerprint


def run_openssl(arguments: list[str], *, label: str) -> bytes:
    try:
        completed = subprocess.run(
            ["openssl", *arguments],
            check=False,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise EvidenceError(f"OpenSSL could not inspect {label}") from error
    if (
        completed.returncode != 0
        or len(completed.stdout) > 1024 * 1024
        or len(completed.stderr) > 1024 * 1024
    ):
        raise EvidenceError(f"OpenSSL rejected {label}")
    return completed.stdout


def certificate_name(certificate: Path, selector: str, label: str) -> str:
    output = run_openssl(
        [
            "x509",
            "-in",
            str(certificate),
            "-noout",
            f"-{selector}",
            "-nameopt",
            "RFC2253,utf8",
        ],
        label=label,
    )
    try:
        text = output.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{label} {selector} is not UTF-8") from error
    prefix = f"{selector}="
    if not text.startswith(prefix) or not text[len(prefix) :] or any(
        ord(character) < 32 for character in text
    ):
        raise EvidenceError(f"{label} has an invalid {selector}")
    return text[len(prefix) :]


def certificate_common_names(certificate: Path, label: str) -> list[str]:
    output = run_openssl(
        [
            "x509",
            "-in",
            str(certificate),
            "-noout",
            "-subject",
            "-nameopt",
            "sep_multiline,utf8,space_eq,sname",
        ],
        label=label,
    )
    try:
        text = output.decode("utf-8")
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{label} subject is not UTF-8") from error
    if any(ord(character) < 32 and character not in "\r\n\t" for character in text):
        raise EvidenceError(f"{label} subject contains control characters")
    return re.findall(r"^[ \t]*CN = ([^\r\n]+)$", text, flags=re.MULTILINE)


def certificate_extension_values(
    certificate: Path, extension: str, label: str
) -> set[str]:
    output = run_openssl(
        ["x509", "-in", str(certificate), "-noout", "-ext", extension],
        label=label,
    )
    try:
        lines = output.decode("ascii").splitlines()
    except UnicodeDecodeError as error:
        raise EvidenceError(f"{label} {extension} extension is not ASCII") from error
    if len(lines) < 2 or not lines[0].startswith("X509v3 "):
        raise EvidenceError(f"{label} has no canonical {extension} extension")
    values = {
        item.strip()
        for item in " ".join(line.strip() for line in lines[1:]).split(",")
        if item.strip()
    }
    if not values:
        raise EvidenceError(f"{label} has an empty {extension} extension")
    return values


def validate_authority_certificate_pair(
    certificate_data: bytes, ca_certificate_data: bytes
) -> None:
    with tempfile.TemporaryDirectory(prefix="goblins-authority2-certificates-") as temporary:
        root = Path(temporary)
        certificate = root / "authority2.pem"
        ca_certificate = root / "authority2-ca.pem"
        certificate.write_bytes(certificate_data)
        ca_certificate.write_bytes(ca_certificate_data)

        if certificate_common_names(certificate, "Authority 2 leaf") != [
            AUTHORITY_COMMON_NAME
        ]:
            raise EvidenceError(
                "display-proof leaf CN is not exactly the fixed Authority 2 identity"
            )
        subject = certificate_name(certificate, "subject", "Authority 2 leaf")
        issuer = certificate_name(certificate, "issuer", "Authority 2 leaf")
        ca_subject = certificate_name(ca_certificate, "subject", "Authority 2 CA")
        if subject == issuer:
            raise EvidenceError("display-proof Authority 2 leaf must not be self-signed")
        if issuer != ca_subject:
            raise EvidenceError("display-proof Authority 2 leaf issuer is not the pinned CA")
        if certificate_extension_values(
            certificate, "basicConstraints", "Authority 2 leaf"
        ) != {"CA:FALSE"}:
            raise EvidenceError("display-proof Authority 2 leaf must be CA:false")
        if certificate_extension_values(
            certificate, "keyUsage", "Authority 2 leaf"
        ) != {"Digital Signature"}:
            raise EvidenceError(
                "display-proof Authority 2 leaf key usage must be digitalSignature only"
            )
        if certificate_extension_values(
            certificate, "extendedKeyUsage", "Authority 2 leaf"
        ) != {"E-mail Protection"}:
            raise EvidenceError(
                "display-proof Authority 2 leaf EKU must be emailProtection only"
            )
        if "CA:TRUE" not in certificate_extension_values(
            ca_certificate, "basicConstraints", "Authority 2 CA"
        ):
            raise EvidenceError("display-proof Authority 2 CA must be CA:true")
        ca_key_usage = certificate_extension_values(
            ca_certificate, "keyUsage", "Authority 2 CA"
        )
        if "Certificate Sign" not in ca_key_usage:
            raise EvidenceError("display-proof Authority 2 CA lacks keyCertSign")
        if ca_key_usage - {"Certificate Sign", "CRL Sign"}:
            raise EvidenceError("display-proof Authority 2 CA has unsafe key usage")
        if certificate_data == ca_certificate_data:
            raise EvidenceError("display-proof leaf and CA certificates are identical")

        checkend = str(30 * 24 * 60 * 60)
        run_openssl(
            ["x509", "-in", str(certificate), "-checkend", checkend, "-noout"],
            label="Authority 2 leaf validity",
        )
        run_openssl(
            ["x509", "-in", str(ca_certificate), "-checkend", checkend, "-noout"],
            label="Authority 2 CA validity",
        )
        run_openssl(
            [
                "verify",
                "-no-CApath",
                "-purpose",
                "smimesign",
                "-CAfile",
                str(ca_certificate),
                str(certificate),
            ],
            label="Authority 2 leaf chain and S/MIME signing purpose",
        )


def read_authority_certificates(
    certificate_path: str,
    fingerprint_path: str,
    ca_certificate_path: str,
    ca_fingerprint_path: str,
) -> tuple[bytes, str, bytes, str]:
    certificate_data, fingerprint = read_authority_certificate(
        certificate_path, fingerprint_path, label="display-proof Authority 2 leaf"
    )
    ca_certificate_data, ca_fingerprint = read_authority_certificate(
        ca_certificate_path,
        ca_fingerprint_path,
        label="display-proof Authority 2 offline CA",
    )
    if fingerprint == ca_fingerprint:
        raise EvidenceError("display-proof Authority 2 leaf and CA fingerprints match")
    validate_authority_certificate_pair(certificate_data, ca_certificate_data)
    return certificate_data, fingerprint, ca_certificate_data, ca_fingerprint


def build_attestation_record(
    seal_data: bytes,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    certificate_fingerprint: str,
    ca_certificate_fingerprint: str,
) -> dict[str, object]:
    seal = validate_seal_structure(
        seal_data, "aarch64", candidate_commit, image_ref, run_date
    )
    if seal["capture_route"] != "local-aarch64-hvf":
        raise EvidenceError("only local aarch64/HVF evidence can use display authority")
    return {
        "architecture": "aarch64",
        "authority_ca_certificate_sha256": ca_certificate_fingerprint,
        "authority_certificate_sha256": certificate_fingerprint,
        "authority_generation": AUTHORITY_GENERATION,
        "candidate_commit": candidate_commit,
        "capture_environment": seal["capture_environment"],
        "evidence_bundle_sha256": hashlib.sha256(seal_data).hexdigest(),
        "evidence_bundle_size": len(seal_data),
        "image_ref": image_ref,
        "run_date": run_date,
        "schema": ATTESTATION_SCHEMA,
        "screenshot_count": len(REQUIRED_PNGS),
        "screenshot_manifest_sha256": seal["screenshot_manifest_sha256"],
        "signature_file": ATTESTATION_SIGNATURE_NAME,
        "signature_purpose": AUTHORITY_PURPOSE,
        "verification_iso_sha256": seal["verification_iso_sha256"],
    }


def create_attestation_record(args: argparse.Namespace) -> int:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise EvidenceError(
            "display-authority records may only be created on the approved Darwin/arm64 capture host"
        )
    seal_data = read_direct_regular(args.seal, MAX_SEAL_BYTES, "evidence bundle")
    _, certificate_fingerprint, _, ca_certificate_fingerprint = read_authority_certificates(
        args.certificate,
        args.certificate_sha256,
        args.ca_certificate,
        args.ca_certificate_sha256,
    )
    record = build_attestation_record(
        seal_data,
        args.candidate_commit,
        args.image_ref,
        args.run_date,
        certificate_fingerprint,
        ca_certificate_fingerprint,
    )
    data = canonical_json(record)
    write_direct_atomic(args.output, data)
    print(certificate_fingerprint)
    return 0


def validate_attestation_record(
    record_data: bytes,
    seal_data: bytes,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    certificate_fingerprint: str,
    ca_certificate_fingerprint: str,
) -> dict[str, object]:
    seal = validate_seal_structure(
        seal_data, "aarch64", candidate_commit, image_ref, run_date
    )
    if seal["capture_route"] != "local-aarch64-hvf":
        raise EvidenceError("aarch64 attestation references the wrong capture route")
    value = parse_json_bytes(
        record_data, maximum=MAX_SEAL_BYTES, label="aarch64 local-display attestation"
    )
    record = require_exact_keys(
        value,
        {
            "architecture",
            "authority_ca_certificate_sha256",
            "authority_certificate_sha256",
            "authority_generation",
            "candidate_commit",
            "capture_environment",
            "evidence_bundle_sha256",
            "evidence_bundle_size",
            "image_ref",
            "run_date",
            "schema",
            "screenshot_count",
            "screenshot_manifest_sha256",
            "signature_file",
            "signature_purpose",
            "verification_iso_sha256",
        },
        "aarch64 local-display attestation",
    )
    if canonical_json(record) != record_data:
        raise EvidenceError("aarch64 attestation is not canonical JSON")
    expected = {
        "architecture": "aarch64",
        "authority_ca_certificate_sha256": ca_certificate_fingerprint,
        "authority_certificate_sha256": certificate_fingerprint,
        "authority_generation": AUTHORITY_GENERATION,
        "candidate_commit": candidate_commit,
        "capture_environment": seal["capture_environment"],
        "evidence_bundle_sha256": hashlib.sha256(seal_data).hexdigest(),
        "evidence_bundle_size": len(seal_data),
        "image_ref": image_ref,
        "run_date": run_date,
        "schema": ATTESTATION_SCHEMA,
        "screenshot_count": len(REQUIRED_PNGS),
        "screenshot_manifest_sha256": seal["screenshot_manifest_sha256"],
        "signature_file": ATTESTATION_SIGNATURE_NAME,
        "signature_purpose": AUTHORITY_PURPOSE,
        "verification_iso_sha256": seal["verification_iso_sha256"],
    }
    if any(record.get(key) != expected_value for key, expected_value in expected.items()):
        raise EvidenceError("aarch64 attestation does not bind the exact local evidence")
    return record


def verify_cms_signature(
    record_data: bytes,
    signature_data: bytes,
    certificate_data: bytes,
    ca_certificate_data: bytes,
) -> None:
    if not signature_data or len(signature_data) > MAX_SIGNATURE_BYTES:
        raise EvidenceError("display-authority CMS signature is empty or excessive")
    with tempfile.TemporaryDirectory(
        prefix="goblins-display-authority-verify-"
    ) as temporary:
        root = Path(temporary)
        record_path = root / ATTESTATION_NAME
        signature_path = root / ATTESTATION_SIGNATURE_NAME
        certificate_path = root / "authority.pem"
        ca_certificate_path = root / "authority-ca.pem"
        record_path.write_bytes(record_data)
        signature_path.write_bytes(signature_data)
        certificate_path.write_bytes(certificate_data)
        ca_certificate_path.write_bytes(ca_certificate_data)
        validate_authority_certificate_pair(certificate_data, ca_certificate_data)
        try:
            completed = subprocess.run(
                [
                    "openssl",
                    "cms",
                    "-verify",
                    "-binary",
                    "-inform",
                    "DER",
                    "-in",
                    str(signature_path),
                    "-content",
                    str(record_path),
                    "-certfile",
                    str(certificate_path),
                    "-nointern",
                    "-CAfile",
                    str(ca_certificate_path),
                    "-no-CApath",
                    "-purpose",
                    "smimesign",
                    "-out",
                    os.devnull,
                ],
                check=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=30,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise EvidenceError("OpenSSL could not verify display authority") from error
    if completed.returncode != 0:
        raise EvidenceError(
            "display-authority CMS signature is invalid for the pinned certificate"
        )


def verify_attestation_record(args: argparse.Namespace) -> int:
    seal_data = read_direct_regular(args.seal, MAX_SEAL_BYTES, "evidence bundle")
    record_data = read_direct_regular(
        args.record, MAX_SEAL_BYTES, "aarch64 local-display attestation"
    )
    signature_data = read_direct_regular(
        args.signature,
        MAX_SIGNATURE_BYTES,
        "aarch64 local-display authority signature",
    )
    (
        certificate_data,
        certificate_fingerprint,
        ca_certificate_data,
        ca_certificate_fingerprint,
    ) = read_authority_certificates(
        args.certificate,
        args.certificate_sha256,
        args.ca_certificate,
        args.ca_certificate_sha256,
    )
    record = validate_attestation_record(
        record_data,
        seal_data,
        args.candidate_commit,
        args.image_ref,
        args.run_date,
        certificate_fingerprint,
        ca_certificate_fingerprint,
    )
    verify_cms_signature(
        record_data, signature_data, certificate_data, ca_certificate_data
    )
    print(
        f"{record['authority_certificate_sha256']} "
        f"{record['authority_ca_certificate_sha256']} "
        f"{record['verification_iso_sha256']} "
        f"{record['screenshot_manifest_sha256']}"
    )
    return 0


def decode_base64(value: str, maximum: int, label: str) -> bytes:
    try:
        encoded = value.encode("ascii")
    except UnicodeEncodeError as error:
        raise EvidenceError(f"{label} is not ASCII base64") from error
    if not encoded or len(encoded) > maximum * 2:
        raise EvidenceError(f"{label} exceeds its fixed encoded byte limit")
    try:
        data = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise EvidenceError(f"{label} is not canonical base64") from error
    if base64.b64encode(data) != encoded:
        raise EvidenceError(f"{label} is not canonical unwrapped base64")
    if not data or len(data) > maximum:
        raise EvidenceError(f"{label} exceeds its decoded byte limit")
    return data


def decode_attestation_input(args: argparse.Namespace) -> int:
    seal_data = decode_base64(
        args.seal_base64, MAX_ATTESTATION_INPUT_BYTES, "evidence bundle input"
    )
    record_data = decode_base64(
        args.record_base64, MAX_ATTESTATION_INPUT_BYTES, "authority record input"
    )
    signature_data = decode_base64(
        args.signature_base64, MAX_SIGNATURE_BYTES, "authority signature input"
    )
    if hashlib.sha256(seal_data).hexdigest() != args.evidence_bundle_sha256:
        raise EvidenceError("attestation input digest does not match the selected bundle")
    (
        certificate_data,
        certificate_fingerprint,
        ca_certificate_data,
        ca_certificate_fingerprint,
    ) = read_authority_certificates(
        args.certificate,
        args.certificate_sha256,
        args.ca_certificate,
        args.ca_certificate_sha256,
    )
    validate_attestation_record(
        record_data,
        seal_data,
        args.candidate_commit,
        args.image_ref,
        args.run_date,
        certificate_fingerprint,
        ca_certificate_fingerprint,
    )
    verify_cms_signature(
        record_data, signature_data, certificate_data, ca_certificate_data
    )
    output_directory = os.path.abspath(args.output_directory)
    os.makedirs(output_directory, mode=0o700, exist_ok=True)
    write_direct_atomic(os.path.join(output_directory, SEAL_NAME), seal_data)
    write_direct_atomic(os.path.join(output_directory, ATTESTATION_NAME), record_data)
    write_direct_atomic(
        os.path.join(output_directory, ATTESTATION_SIGNATURE_NAME), signature_data
    )
    write_direct_atomic(
        os.path.join(output_directory, AUTHORITY_CERTIFICATE_NAME), certificate_data
    )
    write_direct_atomic(
        os.path.join(output_directory, AUTHORITY_FINGERPRINT_NAME),
        (certificate_fingerprint + "\n").encode("ascii"),
    )
    write_direct_atomic(
        os.path.join(output_directory, AUTHORITY_CA_CERTIFICATE_NAME),
        ca_certificate_data,
    )
    write_direct_atomic(
        os.path.join(output_directory, AUTHORITY_CA_FINGERPRINT_NAME),
        (ca_certificate_fingerprint + "\n").encode("ascii"),
    )
    return 0


def make_test_png(width: int, height: int) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    pixels = b"".join(b"\0" + b"\0" * (width * 4) for _ in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(pixels, 9)) + chunk(b"IEND", b"")


def self_test(_: argparse.Namespace) -> int:
    commit = "a" * 40
    image_ref = "ghcr.io/joe-simo/goblins-os@sha256:" + "b" * 64
    run_date = "2026-07-21"
    capture_environment = {
        **CAPTURE_ENVIRONMENT_FIXED_VALUES,
        "qemu_binary_sha256": "c" * 64,
        "qemu_version": "QEMU emulator version 10.0.3",
    }
    for unsupported in ("x86_64", "amd64", "arm64", "riscv64"):
        try:
            validate_metadata(
                unsupported,
                commit,
                image_ref,
                run_date,
                "",
                0,
                capture_environment,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError(
                f"self-test accepted non-canonical architecture {unsupported!r}"
            )
    with tempfile.TemporaryDirectory(prefix="goblins-evidence-self-test-") as temporary:
        repository = Path(temporary)
        run_dir = repository / expected_relative_run_dir("aarch64", run_date)
        run_dir.mkdir(parents=True)
        png = make_test_png(1280, 800)
        for name in REQUIRED_PNGS:
            (run_dir / name).write_bytes(png)
        for name in REQUIRED_PROOFS:
            (run_dir / name).write_text('{"status":"pass"}\n', encoding="utf-8")
        manifest = {
            "architecture": "aarch64",
            "candidate_commit": commit,
            "capture_environment": capture_environment,
            "image_ref": image_ref,
            "iso_sha256": "d" * 64,
            "captured_at": run_date + "T00:00:00Z",
            "screenshot_run_dir": expected_relative_run_dir("aarch64", run_date),
            "capture_workflow_run": "",
            "capture_workflow_run_attempt": 0,
        }
        (run_dir / "proof-manifest.json").write_bytes(canonical_json(manifest))
        for name in COPIED_VERIFICATION_FILES[1:] + ("native-packaging-gate.json",):
            (run_dir / name).write_text('{"schema":"self-test"}\n', encoding="utf-8")
        namespace = argparse.Namespace(
            repository=str(repository),
            run_dir=str(run_dir),
            architecture="aarch64",
            candidate_commit=commit,
            image_ref=image_ref,
            run_date=run_date,
            capture_workflow_run="",
            capture_workflow_run_attempt=0,
            output=str(run_dir / SEAL_NAME),
        )
        create_bundle(namespace)
        verify_bundle(namespace)

        seal_data = (run_dir / SEAL_NAME).read_bytes()
        seal_value = parse_json_bytes(
            seal_data, maximum=MAX_SEAL_BYTES, label="self-test seal"
        )
        for field, invalid in (
            ("host_os", "Linux"),
            ("host_architecture", "aarch64"),
            ("accelerator", "kvm"),
            ("qemu_binary", "qemu-system-x86_64"),
            ("qemu_binary_sha256", "not-a-sha256"),
            ("qemu_version", "QEMU 10.0.3"),
            ("qemu_machine", "virt,accel=kvm,gic-version=max"),
            ("qemu_cpu", "max"),
        ):
            invalid_seal = json.loads(json.dumps(seal_value))
            invalid_seal["capture_environment"][field] = invalid
            try:
                validate_seal_structure(
                    canonical_json(invalid_seal),
                    "aarch64",
                    commit,
                    image_ref,
                    run_date,
                )
            except EvidenceError:
                pass
            else:
                raise EvidenceError(
                    f"self-test accepted {field}={invalid!r} as local aarch64/HVF"
                )

        invalid_manifest = json.loads(json.dumps(manifest))
        invalid_manifest["capture_environment"]["host_os"] = "Linux"
        invalid_manifest["capture_environment"]["accelerator"] = "kvm"
        (run_dir / "proof-manifest.json").write_bytes(canonical_json(invalid_manifest))
        invalid_manifest_fd = os.open(run_dir, os.O_RDONLY)
        try:
            build_seal(
                invalid_manifest_fd,
                "aarch64",
                commit,
                image_ref,
                run_date,
                "",
                0,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted a Linux/KVM proof manifest")
        finally:
            os.close(invalid_manifest_fd)
        (run_dir / "proof-manifest.json").write_bytes(canonical_json(manifest))

        private_key_path = repository / "test-authority2-key.pem"
        certificate_path = repository / "test-authority2.pem"
        fingerprint_path = repository / "test-authority2.sha256"
        ca_private_key_path = repository / "test-authority2-ca-key.pem"
        ca_certificate_path = repository / "test-authority2-ca.pem"
        ca_fingerprint_path = repository / "test-authority2-ca.sha256"
        csr_path = repository / "test-authority2.csr"
        leaf_extensions_path = repository / "test-authority2-leaf.cnf"
        attestation_path = run_dir / ATTESTATION_NAME
        signature_path = run_dir / ATTESTATION_SIGNATURE_NAME

        def self_test_openssl(arguments: list[str], failure: str) -> None:
            try:
                subprocess.run(
                    ["openssl", *arguments],
                    check=True,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=30,
                )
            except (
                OSError,
                subprocess.CalledProcessError,
                subprocess.TimeoutExpired,
            ) as error:
                raise EvidenceError(failure) from error

        def pin_test_certificate(certificate: Path, fingerprint: Path) -> str:
            certificate_text = certificate.read_text(encoding="ascii")
            digest = hashlib.sha256(
                ssl.PEM_cert_to_DER_cert(certificate_text)
            ).hexdigest()
            fingerprint.write_text(digest + "\n", encoding="ascii")
            return digest

        def issue_test_leaf(
            prefix: str,
            common_name: str,
            *,
            basic_constraints: str = "critical,CA:FALSE",
            key_usage: str = "critical,digitalSignature",
        ) -> tuple[Path, Path]:
            key = repository / f"{prefix}-key.pem"
            csr = repository / f"{prefix}.csr"
            certificate = repository / f"{prefix}.pem"
            extensions = repository / f"{prefix}.cnf"
            extensions.write_text(
                "[leaf]\n"
                f"basicConstraints={basic_constraints}\n"
                f"keyUsage={key_usage}\n"
                "extendedKeyUsage=emailProtection\n"
                "subjectKeyIdentifier=hash\n"
                "authorityKeyIdentifier=keyid,issuer\n",
                encoding="ascii",
            )
            self_test_openssl(
                [
                    "req",
                    "-new",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-keyout",
                    str(key),
                    "-out",
                    str(csr),
                    "-subj",
                    f"/CN={common_name}/",
                ],
                "self-test could not create an ephemeral Authority 2 CSR",
            )
            self_test_openssl(
                [
                    "x509",
                    "-req",
                    "-in",
                    str(csr),
                    "-CA",
                    str(ca_certificate_path),
                    "-CAkey",
                    str(ca_private_key_path),
                    "-set_serial",
                    str(secrets.randbelow(2**62) + 2),
                    "-days",
                    "60",
                    "-sha256",
                    "-extfile",
                    str(extensions),
                    "-extensions",
                    "leaf",
                    "-out",
                    str(certificate),
                ],
                "self-test could not issue an ephemeral Authority 2 leaf",
            )
            return key, certificate

        self_test_openssl(
            [
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(ca_private_key_path),
                "-out",
                str(ca_certificate_path),
                "-days",
                "60",
                "-sha256",
                "-subj",
                "/CN=Goblins OS Display Proof Offline CA Self Test/",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ],
            "self-test could not create an ephemeral offline CA",
        )
        leaf_extensions_path.write_text(
            "[leaf]\n"
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature\n"
            "extendedKeyUsage=emailProtection\n"
            "subjectKeyIdentifier=hash\n"
            "authorityKeyIdentifier=keyid,issuer\n",
            encoding="ascii",
        )
        try:
            self_test_openssl(
                [
                    "req",
                    "-new",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-keyout",
                    str(private_key_path),
                    "-out",
                    str(csr_path),
                    "-subj",
                    f"/CN={AUTHORITY_COMMON_NAME}/",
                ],
                "self-test could not create the Authority 2 leaf CSR",
            )
            self_test_openssl(
                [
                    "x509",
                    "-req",
                    "-in",
                    str(csr_path),
                    "-CA",
                    str(ca_certificate_path),
                    "-CAkey",
                    str(ca_private_key_path),
                    "-set_serial",
                    "1",
                    "-days",
                    "60",
                    "-sha256",
                    "-extfile",
                    str(leaf_extensions_path),
                    "-extensions",
                    "leaf",
                    "-out",
                    str(certificate_path),
                ],
                "self-test could not issue the Authority 2 leaf",
            )
        except EvidenceError:
            raise
        certificate_fingerprint = pin_test_certificate(
            certificate_path, fingerprint_path
        )
        ca_certificate_fingerprint = pin_test_certificate(
            ca_certificate_path, ca_fingerprint_path
        )
        (
            certificate_data,
            observed_fingerprint,
            ca_certificate_data,
            observed_ca_fingerprint,
        ) = read_authority_certificates(
            str(certificate_path),
            str(fingerprint_path),
            str(ca_certificate_path),
            str(ca_fingerprint_path),
        )
        attestation_data = canonical_json(
            build_attestation_record(
                seal_data,
                commit,
                image_ref,
                run_date,
                certificate_fingerprint,
                ca_certificate_fingerprint,
            )
        )
        attestation_path.write_bytes(attestation_data)
        self_test_openssl(
            [
                "cms",
                "-sign",
                "-binary",
                "-in",
                str(attestation_path),
                "-signer",
                str(certificate_path),
                "-inkey",
                str(private_key_path),
                "-outform",
                "DER",
                "-out",
                str(signature_path),
                "-md",
                "sha256",
                "-nosmimecap",
                "-nocerts",
            ],
            "self-test could not create a CMS signature",
        )
        attestation_data = attestation_path.read_bytes()
        validate_attestation_record(
            attestation_data,
            seal_data,
            commit,
            image_ref,
            run_date,
            observed_fingerprint,
            observed_ca_fingerprint,
        )
        signature_data = signature_path.read_bytes()
        verify_cms_signature(
            attestation_data,
            signature_data,
            certificate_data,
            ca_certificate_data,
        )
        decoded_directory = repository / "decoded-attestation"
        decode_attestation_input(
            argparse.Namespace(
                seal_base64=base64.b64encode(seal_data).decode("ascii"),
                record_base64=base64.b64encode(attestation_data).decode("ascii"),
                signature_base64=base64.b64encode(signature_data).decode("ascii"),
                evidence_bundle_sha256=hashlib.sha256(seal_data).hexdigest(),
                certificate=str(certificate_path),
                certificate_sha256=str(fingerprint_path),
                ca_certificate=str(ca_certificate_path),
                ca_certificate_sha256=str(ca_fingerprint_path),
                output_directory=str(decoded_directory),
                candidate_commit=commit,
                image_ref=image_ref,
                run_date=run_date,
            )
        )
        if (
            (decoded_directory / SEAL_NAME).read_bytes() != seal_data
            or (decoded_directory / ATTESTATION_NAME).read_bytes()
            != attestation_data
            or (decoded_directory / ATTESTATION_SIGNATURE_NAME).read_bytes()
            != signature_data
            or (decoded_directory / AUTHORITY_CERTIFICATE_NAME).read_bytes()
            != certificate_data
            or (decoded_directory / AUTHORITY_CA_CERTIFICATE_NAME).read_bytes()
            != ca_certificate_data
            or (decoded_directory / AUTHORITY_FINGERPRINT_NAME).read_text(
                encoding="ascii"
            )
            != observed_fingerprint + "\n"
            or (decoded_directory / AUTHORITY_CA_FINGERPRINT_NAME).read_text(
                encoding="ascii"
            )
            != observed_ca_fingerprint + "\n"
        ):
            raise EvidenceError("self-test decoded different signed attestation bytes")
        invalid_attestation = parse_json_bytes(
            attestation_data,
            maximum=MAX_SEAL_BYTES,
            label="self-test attestation",
        )
        invalid_attestation["capture_environment"]["accelerator"] = "kvm"
        try:
            validate_attestation_record(
                canonical_json(invalid_attestation),
                seal_data,
                commit,
                image_ref,
                run_date,
                observed_fingerprint,
                observed_ca_fingerprint,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted a KVM attestation record")
        try:
            verify_cms_signature(
                attestation_data + b" ",
                signature_data,
                certificate_data,
                ca_certificate_data,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted a CMS signature for modified bytes")

        attacker_key_path, attacker_certificate_path = issue_test_leaf(
            "wrong-cn-authority2", "Untrusted dispatcher authority"
        )
        attacker_signature_path = run_dir / "attacker-authority.cms"
        self_test_openssl(
            [
                "cms",
                "-sign",
                "-binary",
                "-in",
                str(attestation_path),
                "-signer",
                str(attacker_certificate_path),
                "-inkey",
                str(attacker_key_path),
                "-outform",
                "DER",
                "-out",
                str(attacker_signature_path),
                "-md",
                "sha256",
                "-nosmimecap",
            ],
            "self-test could not create an untrusted embedded-certificate signature",
        )
        try:
            verify_cms_signature(
                attestation_data,
                attacker_signature_path.read_bytes(),
                certificate_data,
                ca_certificate_data,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError(
                "self-test accepted an untrusted embedded signer certificate"
            )

        self_signed_key = repository / "self-signed-authority2-key.pem"
        self_signed_certificate = repository / "self-signed-authority2.pem"
        self_test_openssl(
            [
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(self_signed_key),
                "-out",
                str(self_signed_certificate),
                "-days",
                "60",
                "-sha256",
                "-subj",
                f"/CN={AUTHORITY_COMMON_NAME}/",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-addext",
                "keyUsage=critical,digitalSignature",
                "-addext",
                "extendedKeyUsage=emailProtection",
            ],
            "self-test could not create a self-signed leaf",
        )
        _, ca_leaf_certificate = issue_test_leaf(
            "ca-leaf-authority2",
            AUTHORITY_COMMON_NAME,
            basic_constraints="critical,CA:TRUE",
            key_usage="critical,digitalSignature,keyCertSign",
        )
        for label, invalid_certificate in (
            ("self-signed", self_signed_certificate),
            ("CA:true leaf", ca_leaf_certificate),
            ("wrong CN", attacker_certificate_path),
        ):
            try:
                validate_authority_certificate_pair(
                    invalid_certificate.read_bytes(), ca_certificate_data
                )
            except EvidenceError:
                pass
            else:
                raise EvidenceError(f"self-test accepted {label} Authority 2 leaf")

        legacy_attestation = json.loads(json.dumps(invalid_attestation))
        legacy_attestation["capture_environment"]["accelerator"] = "hvf"
        legacy_attestation["schema"] = "goblins-os-aarch64-local-display-authority-v1"
        legacy_attestation["signature_purpose"] = "goblins-os-display-proof-authority-v1"
        legacy_attestation["authority_generation"] = 1
        try:
            validate_attestation_record(
                canonical_json(legacy_attestation),
                seal_data,
                commit,
                image_ref,
                run_date,
                observed_fingerprint,
                observed_ca_fingerprint,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted a legacy Authority 1 record")

        mismatched_iso_record = json.loads(json.dumps(invalid_attestation))
        mismatched_iso_record["capture_environment"]["accelerator"] = "hvf"
        mismatched_iso_record["verification_iso_sha256"] = "e" * 64
        try:
            validate_attestation_record(
                canonical_json(mismatched_iso_record),
                seal_data,
                commit,
                image_ref,
                run_date,
                observed_fingerprint,
                observed_ca_fingerprint,
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted an authority record for another ISO")

        excessive_size = json.loads(json.dumps(seal_value))
        excessive_size["entries"][len(REQUIRED_PNGS)]["size"] = MAX_JSON_BYTES + 1
        try:
            validate_seal_structure(
                canonical_json(excessive_size), "aarch64", commit, image_ref, run_date
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted an excessive claimed entry size")

        mismatched_frame = json.loads(json.dumps(seal_value))
        mismatched_frame["entries"][1]["width"] += 1
        try:
            validate_seal_structure(
                canonical_json(mismatched_frame), "aarch64", commit, image_ref, run_date
            )
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted non-uniform PNG dimensions")

        duplicate = seal_data.replace(b'{"architecture":', b'{"schema":"duplicate","architecture":', 1)
        try:
            validate_seal_structure(duplicate, "aarch64", commit, image_ref, run_date)
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted duplicate JSON keys")

        proof = run_dir / REQUIRED_PROOFS[0]
        proof.write_text('{"status":"pass","status":"pass"}\n', encoding="utf-8")
        duplicate_fd = os.open(run_dir, os.O_RDONLY)
        try:
            build_seal(duplicate_fd, "aarch64", commit, image_ref, run_date, "", 0)
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted duplicate proof keys")
        finally:
            os.close(duplicate_fd)
        proof.write_text('{"status":"pass"}\n', encoding="utf-8")

        target = run_dir / REQUIRED_PNGS[0]
        replacement = run_dir / "replacement.png"
        replacement.write_bytes(png)
        target.unlink()
        target.symlink_to(replacement.name)
        try:
            verify_bundle(namespace)
        except EvidenceError:
            pass
        else:
            raise EvidenceError("self-test accepted a symlinked evidence file")

    print("evidence bundle self-test passed")
    return 0


def add_identity_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--architecture", required=True, choices=("aarch64",))
    parser.add_argument("--candidate-commit", required=True)
    parser.add_argument("--image-ref", required=True)
    parser.add_argument("--run-date", required=True)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    create = subparsers.add_parser("create", help="create a canonical evidence bundle")
    create.add_argument("--repository", required=True)
    create.add_argument("--run-dir", required=True)
    add_identity_arguments(create)
    create.add_argument("--capture-workflow-run", default="")
    create.add_argument("--capture-workflow-run-attempt", type=int, default=0)
    create.add_argument("--output")
    create.set_defaults(handler=create_bundle)

    verify = subparsers.add_parser("verify", help="recompute and verify an evidence bundle")
    verify.add_argument("--repository", required=True)
    verify.add_argument("--run-dir", required=True)
    add_identity_arguments(verify)
    verify.set_defaults(handler=verify_bundle)

    inspect = subparsers.add_parser("inspect", help=argparse.SUPPRESS)
    inspect.add_argument("--seal", required=True)
    add_identity_arguments(inspect)
    inspect.set_defaults(handler=inspect_bundle)

    verify_certificate = subparsers.add_parser(
        "verify-authority-certificate", help="verify the repository-pinned display authority"
    )
    verify_certificate.add_argument("--certificate", required=True)
    verify_certificate.add_argument("--certificate-sha256", required=True)
    verify_certificate.add_argument("--ca-certificate", required=True)
    verify_certificate.add_argument("--ca-certificate-sha256", required=True)
    verify_certificate.set_defaults(handler=verify_authority_certificate)

    decode = subparsers.add_parser("decode-attestation-input", help=argparse.SUPPRESS)
    decode.add_argument("--seal-base64", required=True)
    decode.add_argument("--record-base64", required=True)
    decode.add_argument("--signature-base64", required=True)
    decode.add_argument("--evidence-bundle-sha256", required=True)
    decode.add_argument("--certificate", required=True)
    decode.add_argument("--certificate-sha256", required=True)
    decode.add_argument("--ca-certificate", required=True)
    decode.add_argument("--ca-certificate-sha256", required=True)
    decode.add_argument("--output-directory", required=True)
    decode.add_argument("--candidate-commit", required=True)
    decode.add_argument("--image-ref", required=True)
    decode.add_argument("--run-date", required=True)
    decode.set_defaults(handler=decode_attestation_input)

    attest = subparsers.add_parser(
        "create-attestation", help="create a pinned-host display-authority record"
    )
    attest.add_argument("--seal", required=True)
    attest.add_argument("--output", required=True)
    attest.add_argument("--certificate", required=True)
    attest.add_argument("--certificate-sha256", required=True)
    attest.add_argument("--ca-certificate", required=True)
    attest.add_argument("--ca-certificate-sha256", required=True)
    attest.add_argument("--candidate-commit", required=True)
    attest.add_argument("--image-ref", required=True)
    attest.add_argument("--run-date", required=True)
    attest.set_defaults(handler=create_attestation_record)

    verify_attest = subparsers.add_parser("verify-attestation", help=argparse.SUPPRESS)
    verify_attest.add_argument("--seal", required=True)
    verify_attest.add_argument("--record", required=True)
    verify_attest.add_argument("--signature", required=True)
    verify_attest.add_argument("--certificate", required=True)
    verify_attest.add_argument("--certificate-sha256", required=True)
    verify_attest.add_argument("--ca-certificate", required=True)
    verify_attest.add_argument("--ca-certificate-sha256", required=True)
    verify_attest.add_argument("--candidate-commit", required=True)
    verify_attest.add_argument("--image-ref", required=True)
    verify_attest.add_argument("--run-date", required=True)
    verify_attest.set_defaults(handler=verify_attestation_record)

    test = subparsers.add_parser("self-test", help="run adversarial bundle tests")
    test.set_defaults(handler=self_test)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    try:
        return args.handler(args)
    except (EvidenceError, OSError, ValueError, TypeError) as error:
        print(f"evidence bundle rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
