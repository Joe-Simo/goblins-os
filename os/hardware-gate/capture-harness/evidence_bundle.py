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
import re
import secrets
import stat
import struct
import sys
import tempfile
import zlib
from pathlib import Path, PurePath
from typing import Final

from png_validation import MAX_CAPTURE_PNG_BYTES, validate_png_bytes


SCHEMA: Final = "goblins-os-hardware-evidence-bundle-v1"
ATTESTATION_SCHEMA: Final = "goblins-os-aarch64-local-display-attestation-v1"
SEAL_NAME: Final = "evidence-bundle.json"
ATTESTATION_NAME: Final = "aarch64-local-display-attestation.json"
ATTESTATION_WORKFLOW: Final = ".github/workflows/aarch64-local-display-attestation.yml"
SOURCE_REPOSITORY: Final = "https://github.com/Joe-Simo/goblins-os"
MAX_JSON_BYTES: Final = 16 * 1024 * 1024
MAX_SEAL_BYTES: Final = 256 * 1024
MAX_ATTESTATION_INPUT_BYTES: Final = 512 * 1024
MIN_FRAMEBUFFER_WIDTH: Final = 1024
MAX_FRAMEBUFFER_WIDTH: Final = 7680
MIN_FRAMEBUFFER_HEIGHT: Final = 720
MAX_FRAMEBUFFER_HEIGHT: Final = 4320
COMMIT_RE: Final = re.compile(r"[0-9a-f]{40}\Z")
SHA256_RE: Final = re.compile(r"[0-9a-f]{64}\Z")
IMAGE_RE: Final = re.compile(
    r"ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}\Z"
)
RUN_URL_RE: Final = re.compile(
    r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*\Z"
)

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
        raise EvidenceError(f"{label} does not have the exact v1 key set")
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


def validate_metadata(
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    capture_workflow_run: str,
    capture_workflow_run_attempt: int,
) -> str:
    if architecture not in {"aarch64", "x86_64"}:
        raise EvidenceError("architecture must be aarch64 or x86_64")
    if type(candidate_commit) is not str or not COMMIT_RE.fullmatch(candidate_commit):
        raise EvidenceError("candidate commit must be exactly 40 lowercase hexadecimal characters")
    if type(image_ref) is not str or not IMAGE_RE.fullmatch(image_ref):
        raise EvidenceError("image reference must be the exact digest-pinned Goblins OS package")
    validate_date(run_date)
    if type(capture_workflow_run_attempt) is not int:
        raise EvidenceError("capture workflow attempt must be an integer")
    if architecture == "x86_64":
        if not RUN_URL_RE.fullmatch(capture_workflow_run):
            raise EvidenceError("x86_64 evidence requires its exact GitHub capture run URL")
        if capture_workflow_run_attempt < 1:
            raise EvidenceError("x86_64 evidence requires a positive capture run attempt")
        return "github-actions-kvm"
    if capture_workflow_run != "" or capture_workflow_run_attempt != 0:
        raise EvidenceError("local aarch64/HVF evidence must not claim a GitHub capture run")
    return "local-aarch64-hvf"


def expected_names(architecture: str) -> tuple[str, ...]:
    names = REQUIRED_PNGS + REQUIRED_PROOFS + COPIED_VERIFICATION_FILES
    if architecture == "aarch64":
        names += ("native-packaging-gate.json",)
    return names


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
) -> None:
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
    capture_route = validate_metadata(
        architecture,
        candidate_commit,
        image_ref,
        run_date,
        capture_workflow_run,
        capture_workflow_run_attempt,
    )
    proof_manifest_data = read_regular_at(
        directory_fd, "proof-manifest.json", MAX_JSON_BYTES
    )
    require_manifest_binding(
        manifest_metadata(proof_manifest_data),
        architecture,
        candidate_commit,
        image_ref,
        run_date,
        capture_workflow_run,
        capture_workflow_run_attempt,
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
    if dimensions is None or len(REQUIRED_PNGS) != 32:
        raise EvidenceError("the v1 evidence contract must contain exactly 32 screenshots")
    width, height = dimensions
    return {
        "architecture": architecture,
        "candidate_commit": candidate_commit,
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
            "capture_route",
            "capture_workflow_run",
            "capture_workflow_run_attempt",
            "entries",
            "entry_count",
            "framebuffer",
            "image_ref",
            "run_date",
            "schema",
        },
        "evidence bundle",
    )
    if canonical_json(seal) != data:
        raise EvidenceError("evidence bundle is not encoded in canonical JSON form")
    if seal["schema"] != SCHEMA:
        raise EvidenceError("evidence bundle schema is not supported")
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
        or framebuffer["required_png_count"] != 32
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


def attestation_artifact_name(candidate_commit: str, run_date: str, attempt: int) -> str:
    return f"aarch64-local-display-attestation-{candidate_commit}-{run_date}-attempt-{attempt}"


def x86_artifact_name(candidate_commit: str, run_date: str, attempt: int) -> str:
    return f"hardware-gate-evidence-{candidate_commit}-x86_64-{run_date}-attempt-{attempt}"


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


def create_attestation_record(args: argparse.Namespace) -> int:
    seal_data = read_direct_regular(args.seal, MAX_SEAL_BYTES, "evidence bundle")
    seal = validate_seal_structure(
        seal_data, "aarch64", args.candidate_commit, args.image_ref, args.run_date
    )
    if seal["capture_route"] != "local-aarch64-hvf":
        raise EvidenceError("only local aarch64/HVF evidence can use this attestation route")
    if not RUN_URL_RE.fullmatch(args.workflow_run) or type(args.workflow_run_attempt) is not int or args.workflow_run_attempt < 1:
        raise EvidenceError("attestation workflow run identity is invalid")
    artifact_name = attestation_artifact_name(
        args.candidate_commit, args.run_date, args.workflow_run_attempt
    )
    record = {
        "architecture": "aarch64",
        "artifact_name": artifact_name,
        "candidate_commit": args.candidate_commit,
        "evidence_bundle_sha256": hashlib.sha256(seal_data).hexdigest(),
        "evidence_bundle_size": len(seal_data),
        "image_ref": args.image_ref,
        "run_date": args.run_date,
        "schema": ATTESTATION_SCHEMA,
        "signer_workflow": ATTESTATION_WORKFLOW,
        "source_repository": SOURCE_REPOSITORY,
        "workflow_run": args.workflow_run,
        "workflow_run_attempt": args.workflow_run_attempt,
    }
    data = canonical_json(record)
    write_direct_atomic(args.output, data)
    print(artifact_name)
    return 0


def validate_attestation_record(
    record_data: bytes,
    seal_data: bytes,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
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
            "artifact_name",
            "candidate_commit",
            "evidence_bundle_sha256",
            "evidence_bundle_size",
            "image_ref",
            "run_date",
            "schema",
            "signer_workflow",
            "source_repository",
            "workflow_run",
            "workflow_run_attempt",
        },
        "aarch64 local-display attestation",
    )
    if canonical_json(record) != record_data:
        raise EvidenceError("aarch64 attestation is not canonical JSON")
    attempt = record["workflow_run_attempt"]
    expected = {
        "architecture": "aarch64",
        "artifact_name": attestation_artifact_name(candidate_commit, run_date, attempt),
        "candidate_commit": candidate_commit,
        "evidence_bundle_sha256": hashlib.sha256(seal_data).hexdigest(),
        "evidence_bundle_size": len(seal_data),
        "image_ref": image_ref,
        "run_date": run_date,
        "schema": ATTESTATION_SCHEMA,
        "signer_workflow": ATTESTATION_WORKFLOW,
        "source_repository": SOURCE_REPOSITORY,
    }
    if type(attempt) is not int or attempt < 1:
        raise EvidenceError("aarch64 attestation has an invalid workflow attempt")
    if not RUN_URL_RE.fullmatch(record["workflow_run"]):
        raise EvidenceError("aarch64 attestation has an invalid workflow run URL")
    if any(record.get(key) != expected_value for key, expected_value in expected.items()):
        raise EvidenceError("aarch64 attestation does not bind the exact local evidence")
    return record


def verify_attestation_record(args: argparse.Namespace) -> int:
    seal_data = read_direct_regular(args.seal, MAX_SEAL_BYTES, "evidence bundle")
    record_data = read_direct_regular(
        args.record, MAX_SEAL_BYTES, "aarch64 local-display attestation"
    )
    record = validate_attestation_record(
        record_data, seal_data, args.candidate_commit, args.image_ref, args.run_date
    )
    print(f"{record['workflow_run']} {record['workflow_run_attempt']} {record['artifact_name']}")
    return 0


def decode_attestation_input(args: argparse.Namespace) -> int:
    try:
        encoded = args.base64.encode("ascii")
    except UnicodeEncodeError as error:
        raise EvidenceError("attestation input is not ASCII base64") from error
    if len(encoded) > MAX_ATTESTATION_INPUT_BYTES:
        raise EvidenceError("attestation input exceeds its fixed byte limit")
    try:
        data = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise EvidenceError("attestation input is not canonical base64") from error
    if base64.b64encode(data) != encoded:
        raise EvidenceError("attestation input is not canonical unwrapped base64")
    validate_seal_structure(
        data, "aarch64", args.candidate_commit, args.image_ref, args.run_date
    )
    if hashlib.sha256(data).hexdigest() != args.evidence_bundle_sha256:
        raise EvidenceError("attestation input digest does not match the selected bundle")
    write_direct_atomic(args.output, data)
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
            "image_ref": image_ref,
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
    parser.add_argument("--architecture", required=True, choices=("aarch64", "x86_64"))
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

    decode = subparsers.add_parser("decode-attestation-input", help=argparse.SUPPRESS)
    decode.add_argument("--base64", required=True)
    decode.add_argument("--evidence-bundle-sha256", required=True)
    decode.add_argument("--output", required=True)
    decode.add_argument("--candidate-commit", required=True)
    decode.add_argument("--image-ref", required=True)
    decode.add_argument("--run-date", required=True)
    decode.set_defaults(handler=decode_attestation_input)

    attest = subparsers.add_parser("create-attestation", help=argparse.SUPPRESS)
    attest.add_argument("--seal", required=True)
    attest.add_argument("--output", required=True)
    attest.add_argument("--workflow-run", required=True)
    attest.add_argument("--workflow-run-attempt", required=True, type=int)
    attest.add_argument("--candidate-commit", required=True)
    attest.add_argument("--image-ref", required=True)
    attest.add_argument("--run-date", required=True)
    attest.set_defaults(handler=create_attestation_record)

    verify_attest = subparsers.add_parser("verify-attestation", help=argparse.SUPPRESS)
    verify_attest.add_argument("--seal", required=True)
    verify_attest.add_argument("--record", required=True)
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
