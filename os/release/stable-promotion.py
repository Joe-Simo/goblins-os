#!/usr/bin/env python3
"""Validate and assemble an exact-candidate Goblins OS stable promotion.

This helper deliberately has no GitHub or registry credentials. Network reads and
writes live in narrowly scoped workflow steps; this file validates the bytes that
cross those boundaries and creates the deterministic public release payload.
"""

from __future__ import annotations

import argparse
from contextlib import ExitStack, contextmanager
from dataclasses import dataclass
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import shutil
import ssl
import stat
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import zipfile


REPOSITORY = "Joe-Simo/goblins-os"
SOURCE_REPOSITORY = f"https://github.com/{REPOSITORY}"
ARCHITECTURE = "aarch64"
PLATFORM = "linux/arm64"
IMAGE_RE = re.compile(r"ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}\Z")
COMMIT_RE = re.compile(r"[0-9a-f]{40}\Z")
POSITIVE_INTEGER_RE = re.compile(r"[1-9][0-9]*\Z")
STABLE_TAG_RE = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
DATE_RE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}\Z")
SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
RFC3339_RE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\Z")
DISPLAY_SCHEMA = "goblins-os-aarch64-local-display-authority-v2"
EVIDENCE_SCHEMA = "goblins-os-hardware-evidence-bundle-v5"
PROMOTION_SCHEMA = "goblins-os-stable-promotion-v1"
DISPLAY_PURPOSE = "goblins-os-display-proof-authority-v2"
DISPLAY_AUTHORITY_GENERATION = 2
DISPLAY_SCREENSHOT_COUNT = 42
DISPLAY_SCREENSHOT_NAMES = (
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
SIGNED_DISPLAY_FILES = (
    "evidence-bundle.json",
    "aarch64-local-display-attestation.json",
    "aarch64-local-display-attestation.json.cms",
)
AUTHORITY_PUBLIC_FILES = (
    "display-proof-authority2.pem",
    "display-proof-authority2.sha256",
    "display-proof-authority2-ca.pem",
    "display-proof-authority2-ca.sha256",
)
DISPLAY_FILES = SIGNED_DISPLAY_FILES + AUTHORITY_PUBLIC_FILES
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_ZIP_MEMBER_BYTES = 16 * 1024 * 1024 * 1024
MAX_ZIP_TOTAL_BYTES = 24 * 1024 * 1024 * 1024
MAX_ARCHIVE_BYTES = 24 * 1024 * 1024 * 1024
MAX_COMPRESSED_ISO_BYTES = MAX_ZIP_MEMBER_BYTES
MAX_DISPLAY_TOTAL_BYTES = 24 * 1024 * 1024 * 1024
MAX_DISPLAY_TAR_BYTES = MAX_DISPLAY_TOTAL_BYTES + 64 * 1024 * 1024
PART_BYTES = 1_887_436_800
ZSTD_TIMEOUT_SECONDS = 1800
ZSTD_VERSION = "*** Zstandard CLI (64-bit) v1.5.7, by Yann Collet ***"

CANDIDATE_MEMBERS = {
    "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
    "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso.sha256",
    "os/iso/output/aarch64/manifest-goblins-os-aarch64.json",
    "os/iso/output/aarch64/manifest-anaconda-iso.json",
    "os/signoff-proofs/sbom/aarch64/release-evidence-manifest.json",
    "os/signoff-proofs/sbom/aarch64/cargo-lock-packages.tsv",
    "os/signoff-proofs/sbom/aarch64/rpm-packages.command",
    "os/signoff-proofs/sbom/aarch64/rpm-packages.tsv",
    "candidate-output/aarch64/image-ref.json",
}

FIXED_PAYLOAD_NAMES = {
    "goblins-os-aarch64.iso.sha256",
    "goblins-os-aarch64.iso.zst.sha256",
    "goblins-os-aarch64.iso.zst.parts.sha256",
    "manifest-goblins-os-aarch64.json",
    "manifest-anaconda-iso-aarch64.json",
    "release-evidence-manifest-aarch64.json",
    "cargo-lock-packages-aarch64.tsv",
    "rpm-packages-aarch64.command",
    "rpm-packages-aarch64.tsv",
    "candidate-image-ref-aarch64.json",
    "evidence-bundle.json",
    "aarch64-local-display-attestation.json",
    "aarch64-local-display-attestation.json.cms",
    "display-proof-authority2.pem",
    "display-proof-authority2.sha256",
    "display-proof-authority2-ca.pem",
    "display-proof-authority2-ca.sha256",
    "goblins-os-aarch64-display-proof.tar.zst",
    "goblins-os-aarch64-display-proof.tar.zst.sha256",
    "final-shipping-gate-aarch64.log",
    "signoff-notes-aarch64.md",
}
CONTROL_PAYLOAD_NAMES = {"promotion-manifest.json", "promotion-manifest.sha256"}
PART_RE = re.compile(r"goblins-os-aarch64\.iso\.zst\.part-([0-9]{2})\Z")


class PromotionError(ValueError):
    pass


@dataclass(frozen=True)
class ArchiveSnapshot:
    path: Path
    descriptor: int
    identity: tuple[int, ...]
    sha256: str
    size_bytes: int
    label: str


@dataclass(frozen=True)
class PinnedInput:
    path: Path
    descriptor: int
    identity: tuple[int, ...]
    size_bytes: int
    label: str


def fail(message: str) -> None:
    raise PromotionError(message)


def archive_identity(metadata: os.stat_result) -> tuple[int, ...]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_nlink,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_regular(path: Path, *, maximum: int, label: str) -> bytes:
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        fail(f"{label} is missing: {path}")
        raise AssertionError from error
    if (
        not stat.S_ISREG(before.st_mode)
        or stat.S_ISLNK(before.st_mode)
        or before.st_nlink != 1
        or before.st_size <= 0
        or before.st_size > maximum
    ):
        fail(f"{label} is not a bounded single-link regular file: {path}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
            fail(f"{label} changed before it was opened: {path}")
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > maximum:
                fail(f"{label} exceeds its byte limit: {path}")
            chunks.append(chunk)
        after = os.fstat(descriptor)
        if total != before.st_size or (
            opened.st_size,
            opened.st_mtime_ns,
            opened.st_ctime_ns,
        ) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            fail(f"{label} changed while it was read: {path}")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def load_json(path: Path, *, maximum: int = MAX_JSON_BYTES, label: str = "JSON") -> dict[str, object]:
    data = read_regular(path, maximum=maximum, label=label)
    try:
        value = json.loads(
            data.decode("utf-8"),
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=lambda item: fail(f"non-finite JSON value: {item}"),
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{label} is not strict UTF-8 JSON: {path}: {error}")
    if type(value) is not dict:
        fail(f"{label} must be a JSON object: {path}")
    return value


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, allow_nan=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")


def pinned_certificate_fingerprint(
    certificate_data: bytes, fingerprint_data: bytes, *, label: str
) -> str:
    try:
        certificate_text = certificate_data.decode("ascii")
        der = ssl.PEM_cert_to_DER_cert(certificate_text)
        fingerprint = fingerprint_data.decode("ascii")
    except (UnicodeDecodeError, ValueError) as error:
        raise PromotionError(f"{label} public certificate is not canonical PEM") from error
    if (
        certificate_text.count("-----BEGIN CERTIFICATE-----") != 1
        or certificate_text.count("-----END CERTIFICATE-----") != 1
        or certificate_text != ssl.DER_cert_to_PEM_cert(der)
        or not fingerprint.endswith("\n")
        or SHA256_RE.fullmatch(fingerprint.removesuffix("\n")) is None
    ):
        fail(f"{label} public certificate or fingerprint is not canonical")
    observed = hashlib.sha256(der).hexdigest()
    if fingerprint != observed + "\n":
        fail(f"{label} public certificate does not match its fingerprint")
    return observed


def write_exclusive(path: Path, data: bytes, mode: int = 0o644) -> None:
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    try:
        view = memoryview(data)
        while view:
            written = os.write(descriptor, view)
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def sha256_path(path: Path, maximum: int = MAX_ZIP_MEMBER_BYTES) -> str:
    metadata = path.lstat()
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_nlink != 1
        or metadata.st_size <= 0
        or metadata.st_size > maximum
    ):
        fail(f"unsafe artifact while hashing: {path}")
    digest = hashlib.sha256()
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino):
            fail(f"artifact changed before hashing: {path}")
        total = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > maximum:
                fail(f"artifact exceeds its byte limit: {path}")
            digest.update(chunk)
        after = os.fstat(descriptor)
        if total != metadata.st_size or (
            opened.st_size,
            opened.st_mtime_ns,
            opened.st_ctime_ns,
        ) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            fail(f"artifact changed while hashing: {path}")
    finally:
        os.close(descriptor)
    return digest.hexdigest()


@contextmanager
def snapshot_archive(path: Path, *, label: str):
    """Hash and hold one O_NOFOLLOW archive descriptor for every later ZIP read."""
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        fail(f"{label} is missing: {path}")
        raise AssertionError from error
    if (
        not stat.S_ISREG(before.st_mode)
        or stat.S_ISLNK(before.st_mode)
        or before.st_nlink != 1
        or before.st_size <= 0
        or before.st_size > MAX_ARCHIVE_BYTES
    ):
        fail(f"{label} is not a bounded single-link regular archive: {path}")

    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    source_descriptor = os.open(path, flags)
    try:
        opened = os.fstat(source_descriptor)
        identity = archive_identity(before)
        if archive_identity(opened) != identity:
            fail(f"{label} changed before it was opened: {path}")

        digest = hashlib.sha256()
        total = 0
        while True:
            chunk = os.read(source_descriptor, 1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_ARCHIVE_BYTES:
                fail(f"{label} exceeds its byte limit: {path}")
            digest.update(chunk)

        snapshot = ArchiveSnapshot(
            path=path,
            descriptor=source_descriptor,
            identity=identity,
            sha256=digest.hexdigest(),
            size_bytes=total,
            label=label,
        )
        assert_archive_snapshot_stable(snapshot, action="while it was hashed")
        try:
            yield snapshot
        finally:
            assert_archive_snapshot_stable(snapshot, action="while it was consumed")
    finally:
        os.close(source_descriptor)


def assert_archive_snapshot_stable(archive: ArchiveSnapshot, *, action: str) -> None:
    opened = os.fstat(archive.descriptor)
    try:
        current = archive.path.lstat()
    except FileNotFoundError as error:
        fail(f"{archive.label} disappeared {action}: {archive.path}")
        raise AssertionError from error
    if (
        archive.size_bytes != archive.identity[6]
        or archive_identity(opened) != archive.identity
        or archive_identity(current) != archive.identity
    ):
        fail(f"{archive.label} changed {action}: {archive.path}")


@contextmanager
def open_snapshot_zip(archive: ArchiveSnapshot):
    """Open ZIP bytes only through the descriptor pinned by snapshot_archive."""
    assert_archive_snapshot_stable(archive, action="before a ZIP read")
    duplicate = os.dup(archive.descriptor)
    try:
        os.lseek(duplicate, 0, os.SEEK_SET)
        with os.fdopen(duplicate, "rb", closefd=True) as stream:
            duplicate = -1
            try:
                with zipfile.ZipFile(stream) as handle:
                    yield handle
            except zipfile.BadZipFile as error:
                fail(f"artifact archive is not a valid ZIP: {archive.path}: {error}")
    finally:
        if duplicate >= 0:
            os.close(duplicate)
        assert_archive_snapshot_stable(archive, action="during a ZIP read")


def assert_pinned_input_stable(pinned: PinnedInput, *, action: str) -> None:
    opened = os.fstat(pinned.descriptor)
    try:
        current = pinned.path.lstat()
    except FileNotFoundError as error:
        fail(f"{pinned.label} disappeared {action}: {pinned.path}")
        raise AssertionError from error
    if (
        opened.st_size != pinned.size_bytes
        or archive_identity(opened) != pinned.identity
        or archive_identity(current) != pinned.identity
    ):
        fail(f"{pinned.label} changed {action}: {pinned.path}")


@contextmanager
def pin_regular_input(path: Path, *, maximum: int, label: str):
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        fail(f"{label} is missing: {path}")
        raise AssertionError from error
    if (
        maximum <= 0
        or not stat.S_ISREG(before.st_mode)
        or stat.S_ISLNK(before.st_mode)
        or before.st_nlink != 1
        or before.st_size <= 0
        or before.st_size > maximum
    ):
        fail(f"{label} is not a bounded single-link regular input: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        opened = os.fstat(descriptor)
        identity = archive_identity(before)
        if archive_identity(opened) != identity:
            fail(f"{label} changed before it was pinned: {path}")
        pinned = PinnedInput(path, descriptor, identity, opened.st_size, label)
        try:
            yield pinned
        finally:
            assert_pinned_input_stable(pinned, action="while it was consumed")
    finally:
        os.close(descriptor)


def validate_inputs(
    commit: str,
    candidate_run_id: str,
    candidate_attempt: str,
    display_run_id: str,
    display_attempt: str,
    tag: str,
    confirmation: str,
) -> None:
    if COMMIT_RE.fullmatch(commit) is None:
        fail("candidate commit must be one exact lowercase 40-hex commit")
    for label, value in (
        ("candidate workflow run ID", candidate_run_id),
        ("candidate workflow run attempt", candidate_attempt),
        ("display verification run ID", display_run_id),
        ("display verification run attempt", display_attempt),
    ):
        if POSITIVE_INTEGER_RE.fullmatch(value) is None:
            fail(f"{label} must be a positive base-10 integer")
    if STABLE_TAG_RE.fullmatch(tag) is None:
        fail("stable tag must be a canonical final SemVer tag such as v1.2.3")
    expected = f"promote-aarch64-stable {tag} {commit}"
    if confirmation != expected:
        fail(f"confirmation must exactly equal: {expected}")


def safe_zip_members(
    archive: ArchiveSnapshot, expected_files: set[str]
) -> dict[str, zipfile.ZipInfo]:
    with open_snapshot_zip(archive) as handle:
        seen: dict[str, zipfile.ZipInfo] = {}
        total = 0
        for member in handle.infolist():
            raw_name = member.filename
            pure = PurePosixPath(raw_name)
            if (
                not raw_name
                or raw_name.startswith("/")
                or "\\" in raw_name
                or any(part in ("", ".", "..") for part in pure.parts)
            ):
                fail(f"artifact archive contains an unsafe path: {raw_name!r}")
            normalized = str(pure)
            if normalized in seen:
                fail(f"artifact archive contains a duplicate path: {normalized}")
            unix_mode = member.external_attr >> 16
            kind = stat.S_IFMT(unix_mode)
            if member.is_dir():
                if kind not in (0, stat.S_IFDIR):
                    fail(f"artifact directory has an unsafe file type: {normalized}")
                continue
            if kind not in (0, stat.S_IFREG):
                fail(f"artifact archive contains a non-regular member: {normalized}")
            if member.file_size <= 0 or member.file_size > MAX_ZIP_MEMBER_BYTES:
                fail(f"artifact member has an invalid size: {normalized}")
            total += member.file_size
            if total > MAX_ZIP_TOTAL_BYTES:
                fail("artifact archive expands beyond the fixed total byte limit")
            seen[normalized] = member
        if set(seen) != expected_files:
            missing = sorted(expected_files - set(seen))
            extra = sorted(set(seen) - expected_files)
            fail(f"artifact member allowlist mismatch; missing={missing}, extra={extra}")
        return seen


def read_zip_member_data(
    handle: zipfile.ZipFile,
    member: zipfile.ZipInfo,
    *,
    maximum: int,
    label: str,
) -> bytes:
    if maximum <= 0 or member.file_size <= 0 or member.file_size > maximum:
        fail(f"{label} exceeds its purpose-specific byte limit before reading")
    chunks: list[bytes] = []
    total = 0
    with handle.open(member) as source:
        while True:
            chunk = source.read(min(1024 * 1024, maximum - total + 1))
            if not chunk:
                break
            total += len(chunk)
            if total > maximum or total > member.file_size:
                fail(f"{label} expanded beyond its purpose-specific byte limit")
            chunks.append(chunk)
    if total != member.file_size:
        fail(f"{label} size changed while it was read")
    return b"".join(chunks)


def read_zip_json(
    archive: ArchiveSnapshot, member_name: str, expected_files: set[str]
) -> dict[str, object]:
    members = safe_zip_members(archive, expected_files)
    with open_snapshot_zip(archive) as handle:
        data = read_zip_member_data(
            handle,
            members[member_name],
            maximum=MAX_JSON_BYTES,
            label=f"ZIP JSON member {member_name}",
        )
    try:
        value = json.loads(
            data.decode("utf-8"),
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=lambda item: fail(f"non-finite JSON value: {item}"),
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"ZIP member is not strict JSON: {member_name}: {error}")
    if type(value) is not dict:
        fail(f"ZIP JSON member is not an object: {member_name}")
    return value


def extract_zip(
    archive: ArchiveSnapshot, destination: Path, expected_files: set[str]
) -> None:
    members = safe_zip_members(archive, expected_files)
    if destination.exists():
        fail(f"artifact extraction destination already exists: {destination}")
    destination.mkdir(mode=0o700, parents=True)
    root = destination.resolve(strict=True)
    try:
        with open_snapshot_zip(archive) as handle:
            for name in sorted(members):
                target = destination.joinpath(*PurePosixPath(name).parts)
                target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
                if target.parent.resolve(strict=True) != root and root not in target.parent.resolve(strict=True).parents:
                    fail(f"artifact extraction escaped its destination: {name}")
                descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
                try:
                    with handle.open(members[name]) as source:
                        total = 0
                        while True:
                            chunk = source.read(1024 * 1024)
                            if not chunk:
                                break
                            total += len(chunk)
                            if total > members[name].file_size:
                                fail(f"artifact member expanded beyond its declared size: {name}")
                            view = memoryview(chunk)
                            while view:
                                written = os.write(descriptor, view)
                                view = view[written:]
                        if total != members[name].file_size:
                            fail(f"artifact member size changed during extraction: {name}")
                    os.fsync(descriptor)
                finally:
                    os.close(descriptor)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise


def validate_artifact_digest(
    archive: ArchiveSnapshot, artifact: dict[str, object], label: str
) -> None:
    expected = artifact.get("digest")
    if not isinstance(expected, str) or re.fullmatch(r"sha256:[0-9a-f]{64}", expected) is None:
        fail(f"{label} has no canonical GitHub artifact digest")
    actual = f"sha256:{archive.sha256}"
    if actual != expected:
        fail(f"{label} archive digest is {actual}, expected {expected}")


def validate_run(
    run: dict[str, object],
    *,
    run_id: str,
    attempt: str,
    commit: str,
    allowed_paths: set[str],
    label: str,
) -> None:
    expected = {
        "id": int(run_id),
        "head_sha": commit,
        "run_attempt": int(attempt),
        "status": "completed",
        "conclusion": "success",
        "event": "workflow_dispatch",
        "html_url": f"{SOURCE_REPOSITORY}/actions/runs/{run_id}",
    }
    if any(run.get(key) != value for key, value in expected.items()):
        fail(f"{label} does not identify the exact successful requested attempt")
    repository = run.get("head_repository")
    if type(repository) is not dict or repository.get("full_name") != REPOSITORY:
        fail(f"{label} came from the wrong repository")
    if run.get("path") not in allowed_paths:
        fail(f"{label} used an unapproved workflow path: {run.get('path')!r}")


def select_artifact(
    response: dict[str, object],
    *,
    name_pattern: re.Pattern[str],
    run_id: str,
    commit: str,
    label: str,
) -> dict[str, object]:
    artifacts = response.get("artifacts")
    if type(artifacts) is not list:
        fail(f"{label} artifact response has no artifact list")
    matches: list[dict[str, object]] = []
    for item in artifacts:
        if type(item) is not dict or item.get("expired") is not False:
            continue
        name = item.get("name")
        if not isinstance(name, str) or name_pattern.fullmatch(name) is None:
            continue
        workflow_run = item.get("workflow_run")
        if (
            type(workflow_run) is dict
            and workflow_run.get("id") == int(run_id)
            and workflow_run.get("head_sha") == commit
        ):
            matches.append(item)
    if len(matches) != 1:
        fail(f"{label} requires exactly one unexpired artifact; found {len(matches)}")
    return matches[0]


def validate_remote(args: argparse.Namespace) -> int:
    validate_inputs(
        args.candidate_commit,
        args.candidate_run_id,
        args.candidate_run_attempt,
        args.display_run_id,
        args.display_run_attempt,
        args.stable_tag,
        args.confirmation,
    )
    with snapshot_archive(
        Path(args.candidate_archive), label="candidate workflow artifact"
    ) as candidate_archive, snapshot_archive(
        Path(args.display_archive), label="display verification artifact"
    ) as display_archive:
        return validate_remote_snapshots(args, candidate_archive, display_archive)


def validate_remote_snapshots(
    args: argparse.Namespace,
    candidate_archive: ArchiveSnapshot,
    display_archive: ArchiveSnapshot,
) -> int:
    candidate_run = load_json(Path(args.candidate_run_json), label="candidate workflow run")
    candidate_artifacts = load_json(Path(args.candidate_artifacts_json), label="candidate artifact response")
    display_run = load_json(Path(args.display_run_json), label="display verification workflow run")
    display_artifacts = load_json(Path(args.display_artifacts_json), label="display artifact response")

    validate_run(
        candidate_run,
        run_id=args.candidate_run_id,
        attempt=args.candidate_run_attempt,
        commit=args.candidate_commit,
        allowed_paths={".github/workflows/candidate-artifacts.yml", ".github/workflows/release.yml"},
        label="candidate workflow run",
    )
    validate_run(
        display_run,
        run_id=args.display_run_id,
        attempt=args.display_run_attempt,
        commit=args.candidate_commit,
        allowed_paths={".github/workflows/aarch64-local-display-attestation.yml"},
        label="display verification workflow run",
    )

    candidate_name = f"goblins-os-candidate-{args.candidate_commit}-aarch64"
    candidate_artifact = select_artifact(
        candidate_artifacts,
        name_pattern=re.compile(re.escape(candidate_name)),
        run_id=args.candidate_run_id,
        commit=args.candidate_commit,
        label="candidate workflow",
    )
    validate_artifact_digest(candidate_archive, candidate_artifact, "candidate workflow artifact")
    metadata = read_zip_json(
        candidate_archive, "candidate-output/aarch64/image-ref.json", CANDIDATE_MEMBERS
    )
    image_ref = metadata.get("immutable_image_ref")
    expected_metadata = {
        "schema": "goblins-os-candidate-image-ref-v3",
        "product": "Goblins OS",
        "architecture": ARCHITECTURE,
        "platform": PLATFORM,
        "candidate_commit": args.candidate_commit,
        "oci_revision": args.candidate_commit,
        "workflow_run": f"{SOURCE_REPOSITORY}/actions/runs/{args.candidate_run_id}",
        "workflow_run_attempt": int(args.candidate_run_attempt),
        "workflow_name": "candidate-artifacts",
        "candidate_tag_authoritative": False,
        "non_promotional": True,
    }
    if any(metadata.get(key) != value for key, value in expected_metadata.items()):
        fail("candidate image metadata does not bind the exact requested workflow attempt")
    if not isinstance(image_ref, str) or IMAGE_RE.fullmatch(image_ref) is None:
        fail("candidate image metadata has no canonical immutable aarch64 image")
    if metadata.get("image_digest") != image_ref.rsplit("@", 1)[1]:
        fail("candidate image digest and immutable reference disagree")

    display_pattern = re.compile(
        rf"aarch64-local-display-verified-{re.escape(args.candidate_commit)}-"
        rf"([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})-attempt-{re.escape(args.display_run_attempt)}"
    )
    display_artifact = select_artifact(
        display_artifacts,
        name_pattern=display_pattern,
        run_id=args.display_run_id,
        commit=args.candidate_commit,
        label="display verification workflow",
    )
    display_name = str(display_artifact["name"])
    match = display_pattern.fullmatch(display_name)
    assert match is not None
    run_date = match.group(1)
    try:
        if dt.date.fromisoformat(run_date).isoformat() != run_date:
            fail("display artifact has a non-canonical run date")
    except ValueError as error:
        raise PromotionError("display artifact has an invalid run date") from error
    validate_artifact_digest(display_archive, display_artifact, "display verification artifact")
    record = read_zip_json(
        display_archive,
        "aarch64-local-display-attestation.json",
        set(DISPLAY_FILES),
    )
    seal = read_zip_json(display_archive, "evidence-bundle.json", set(DISPLAY_FILES))
    framebuffer = seal.get("framebuffer")
    if type(framebuffer) is not dict or type(framebuffer.get("required_png_count")) is not int:
        fail("display evidence bundle has no canonical screenshot count")
    screenshot_count = framebuffer["required_png_count"]
    if screenshot_count != DISPLAY_SCREENSHOT_COUNT:
        fail("display evidence bundle does not contain exactly 42 screenshots")
    authority_certificate_sha256 = pinned_certificate_fingerprint(
        read_zip_member(
            display_archive,
            "display-proof-authority2.pem",
            set(DISPLAY_FILES),
            maximum=64 * 1024,
            label="Authority 2 leaf certificate",
        ),
        read_zip_member(
            display_archive,
            "display-proof-authority2.sha256",
            set(DISPLAY_FILES),
            maximum=256,
            label="Authority 2 leaf fingerprint",
        ),
        label="Authority 2 leaf",
    )
    authority_ca_certificate_sha256 = pinned_certificate_fingerprint(
        read_zip_member(
            display_archive,
            "display-proof-authority2-ca.pem",
            set(DISPLAY_FILES),
            maximum=64 * 1024,
            label="Authority 2 CA certificate",
        ),
        read_zip_member(
            display_archive,
            "display-proof-authority2-ca.sha256",
            set(DISPLAY_FILES),
            maximum=256,
            label="Authority 2 CA fingerprint",
        ),
        label="Authority 2 offline CA",
    )
    expected_record = {
        "schema": DISPLAY_SCHEMA,
        "architecture": ARCHITECTURE,
        "authority_generation": DISPLAY_AUTHORITY_GENERATION,
        "authority_certificate_sha256": authority_certificate_sha256,
        "authority_ca_certificate_sha256": authority_ca_certificate_sha256,
        "candidate_commit": args.candidate_commit,
        "image_ref": image_ref,
        "run_date": run_date,
        "screenshot_count": screenshot_count,
        "signature_file": "aarch64-local-display-attestation.json.cms",
        "signature_purpose": DISPLAY_PURPOSE,
    }
    if any(record.get(key) != value for key, value in expected_record.items()):
        fail("display authority record does not bind the exact requested candidate")
    if seal.get("schema") != EVIDENCE_SCHEMA or seal.get("architecture") != ARCHITECTURE:
        fail("display evidence bundle is not the canonical aarch64 schema")
    if seal.get("candidate_commit") != args.candidate_commit or seal.get("image_ref") != image_ref:
        fail("display evidence bundle changes candidate identity")
    if record.get("evidence_bundle_sha256") != hashlib.sha256(
        read_zip_member(
            display_archive,
            "evidence-bundle.json",
            set(DISPLAY_FILES),
            maximum=MAX_JSON_BYTES,
            label="display evidence bundle",
        )
    ).hexdigest():
        fail("display authority record does not bind the downloaded evidence bundle")

    output = {
        "schema": "goblins-os-stable-promotion-remote-inputs-v1",
        "candidate_commit": args.candidate_commit,
        "candidate_run_id": int(args.candidate_run_id),
        "candidate_run_attempt": int(args.candidate_run_attempt),
        "candidate_artifact_name": candidate_name,
        "candidate_artifact_digest": candidate_artifact["digest"],
        "image_ref": image_ref,
        "display_run_id": int(args.display_run_id),
        "display_run_attempt": int(args.display_run_attempt),
        "display_artifact_name": display_name,
        "display_artifact_digest": display_artifact["digest"],
        "run_date": run_date,
        "stable_tag": args.stable_tag,
    }
    output_path = Path(args.output)
    write_exclusive(output_path, canonical_json(output))
    print(image_ref)
    return 0


def read_zip_member(
    archive: ArchiveSnapshot,
    member_name: str,
    expected_files: set[str],
    *,
    maximum: int,
    label: str,
) -> bytes:
    members = safe_zip_members(archive, expected_files)
    with open_snapshot_zip(archive) as handle:
        return read_zip_member_data(
            handle,
            members[member_name],
            maximum=maximum,
            label=label,
        )


def extract_candidate(args: argparse.Namespace) -> int:
    with snapshot_archive(Path(args.archive), label="candidate workflow artifact") as archive:
        if args.expected_digest != f"sha256:{archive.sha256}":
            fail("candidate workflow artifact changed after remote validation")
        extract_zip(archive, Path(args.destination), CANDIDATE_MEMBERS)
    return 0


def extract_display(args: argparse.Namespace) -> int:
    with snapshot_archive(Path(args.archive), label="display verification artifact") as archive:
        if args.expected_digest != f"sha256:{archive.sha256}":
            fail("display verification artifact changed after remote validation")
        extract_zip(archive, Path(args.destination), set(DISPLAY_FILES))
    return 0


def scan_flat_display_source(source: Path) -> list[Path]:
    if source.is_symlink() or not source.is_dir():
        fail(f"local display proof is not a regular directory: {source}")
    files: list[Path] = []
    total = 0
    for item in sorted(source.iterdir(), key=lambda value: value.name.encode("utf-8")):
        if item.name in (".", "..") or "/" in item.name or "\\" in item.name:
            fail(f"local display proof has an unsafe member name: {item.name!r}")
        metadata = item.lstat()
        if not stat.S_ISREG(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode) or metadata.st_nlink != 1:
            fail(f"local display proof contains a non-regular or linked member: {item}")
        if metadata.st_size <= 0 or metadata.st_size > MAX_ZIP_MEMBER_BYTES:
            fail(f"local display proof member has an invalid size: {item}")
        total += metadata.st_size
        if total > MAX_DISPLAY_TOTAL_BYTES:
            fail("local display proof exceeds the fixed total byte limit")
        files.append(item)
    if not files:
        fail("local display proof is empty")
    return files


def install_display(args: argparse.Namespace) -> int:
    source = Path(args.source).resolve(strict=True)
    remote = Path(args.verified_display).resolve(strict=True)
    repository = Path(args.repository).resolve(strict=True)
    remote_inputs = load_json(Path(args.remote_inputs), label="remote promotion inputs")
    run_date = remote_inputs.get("run_date")
    if not isinstance(run_date, str) or DATE_RE.fullmatch(run_date) is None:
        fail("remote promotion inputs have no canonical display run date")
    if source.name != run_date or source.parent.name != ARCHITECTURE:
        fail("local display proof must be the canonical aarch64/YYYY-MM-DD run directory")
    source_files = scan_flat_display_source(source)
    source_by_name = {item.name: item for item in source_files}
    source_names = set(source_by_name)
    required = set(SIGNED_DISPLAY_FILES) | {"signoff-row.md"}
    if not required.issubset(source_names):
        fail(f"local display proof is missing required files: {sorted(required - source_names)}")
    source_seal = load_json(source / "evidence-bundle.json", label="local display seal")
    source_entries = source_seal.get("entries")
    if type(source_entries) is not list or source_seal.get("entry_count") != len(source_entries):
        fail("local display seal has an invalid canonical member set")
    sealed_names: set[str] = set()
    for entry in source_entries:
        name = entry.get("path") if type(entry) is dict else None
        if (
            not isinstance(name, str)
            or PurePosixPath(name).name != name
            or name in ("", ".", "..")
            or name in sealed_names
        ):
            fail("local display seal has an unsafe or duplicate member name")
        sealed_names.add(name)
    canonical_names = sealed_names | required
    if not canonical_names.issubset(source_names):
        fail(
            f"local display proof is missing sealed canonical files: {sorted(canonical_names - source_names)}"
        )
    files = [
        source_by_name[name]
        for name in sorted(canonical_names, key=lambda value: value.encode("utf-8"))
    ]
    remote_files = scan_flat_display_source(remote)
    remote_names = {item.name for item in remote_files}
    if remote_names != set(DISPLAY_FILES):
        fail("downloaded display-verification artifact contains unexpected files")
    for name in SIGNED_DISPLAY_FILES:
        if sha256_path(source / name) != sha256_path(remote / name):
            fail(f"local display proof differs from the exact verified workflow artifact: {name}")
    for name in AUTHORITY_PUBLIC_FILES:
        repository_pin = repository / "os/release" / name
        if sha256_path(repository_pin) != sha256_path(remote / name):
            fail(f"verified display proof does not use the repository-pinned {name}")
    files.extend(remote / name for name in AUTHORITY_PUBLIC_FILES)
    files.sort(key=lambda value: value.name.encode("utf-8"))
    destination = repository / "os/screenshots/hardware-gate/aarch64" / run_date
    if destination.exists() or destination.is_symlink():
        fail(f"canonical display proof destination already exists: {destination}")
    destination.mkdir(mode=0o755, parents=True)
    try:
        for item in files:
            target = destination / item.name
            descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            try:
                flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                source_descriptor = os.open(item, flags)
                try:
                    while True:
                        chunk = os.read(source_descriptor, 1024 * 1024)
                        if not chunk:
                            break
                        view = memoryview(chunk)
                        while view:
                            written = os.write(descriptor, view)
                            view = view[written:]
                finally:
                    os.close(source_descriptor)
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
            if sha256_path(target) != sha256_path(item):
                fail(f"local display proof changed while it was imported: {item.name}")
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(destination)
    return 0


def copy_asset(source: Path, destination: Path) -> None:
    data = read_regular(source, maximum=MAX_ZIP_MEMBER_BYTES, label="release source artifact")
    write_exclusive(destination, data)


def checksum_line(path: Path, name: str | None = None) -> bytes:
    return f"{sha256_path(path)}  {name or path.name}\n".encode("ascii")


def zstd_version(zstd: str) -> str:
    completed = subprocess.run(
        [zstd, "--version"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
    )
    version = completed.stdout.strip()
    if not version or len(version) > 512 or any(ord(character) < 32 for character in version):
        fail("zstd returned an invalid version string")
    return version


def require_exact_zstd(zstd: str) -> None:
    if zstd_version(zstd) != ZSTD_VERSION:
        fail(f"stable assets require the exact reviewed compressor: {ZSTD_VERSION}")


def compress_once(zstd: str, source: Path, destination: Path) -> None:
    command = [
        zstd,
        "--ultra",
        "-19",
        "--long=31",
        "-T1",
        "--no-progress",
        "--force",
    ]
    subprocess.run(command + ["-o", str(destination), str(source)], check=True)


def compress_deterministically(zstd: str, source: Path, destination: Path) -> None:
    duplicate = destination.with_name(destination.name + ".determinism-check")
    compress_once(zstd, source, destination)
    compress_once(zstd, source, duplicate)
    if sha256_path(destination) != sha256_path(duplicate):
        fail(f"zstd did not reproduce identical bytes for {source.name}")
    duplicate.unlink()


def regular_files_are_identical(
    left: Path,
    right: Path,
    *,
    maximum: int,
    label: str,
) -> None:
    with ExitStack() as stack:
        left_input = stack.enter_context(
            pin_regular_input(left, maximum=maximum, label=f"{label} left input")
        )
        right_input = stack.enter_context(
            pin_regular_input(right, maximum=maximum, label=f"{label} right input")
        )
        if left_input.size_bytes != right_input.size_bytes:
            fail(label)
        left_descriptor = os.dup(left_input.descriptor)
        right_descriptor = os.dup(right_input.descriptor)
        try:
            os.lseek(left_descriptor, 0, os.SEEK_SET)
            os.lseek(right_descriptor, 0, os.SEEK_SET)
            while True:
                left_chunk = os.read(left_descriptor, 1024 * 1024)
                right_chunk = os.read(right_descriptor, 1024 * 1024)
                if left_chunk != right_chunk:
                    fail(label)
                if not left_chunk:
                    break
        finally:
            os.close(left_descriptor)
            os.close(right_descriptor)


def canonical_file_matches_parts(
    canonical: Path,
    parts: list[Path],
    *,
    label: str,
) -> None:
    if not parts:
        fail(label)
    with ExitStack() as stack:
        canonical_input = stack.enter_context(
            pin_regular_input(
                canonical,
                maximum=MAX_COMPRESSED_ISO_BYTES,
                label=f"{label} canonical input",
            )
        )
        pinned_parts = [
            stack.enter_context(
                pin_regular_input(
                    part,
                    maximum=MAX_COMPRESSED_ISO_BYTES,
                    label=f"{label} compressed part {part.name}",
                )
            )
            for part in parts
        ]
        if sum(part.size_bytes for part in pinned_parts) != canonical_input.size_bytes:
            fail(label)
        canonical_descriptor = os.dup(canonical_input.descriptor)
        try:
            os.lseek(canonical_descriptor, 0, os.SEEK_SET)
            for part in pinned_parts:
                part_descriptor = os.dup(part.descriptor)
                try:
                    os.lseek(part_descriptor, 0, os.SEEK_SET)
                    while True:
                        part_chunk = os.read(part_descriptor, 1024 * 1024)
                        if not part_chunk:
                            break
                        canonical_chunk = bytearray()
                        while len(canonical_chunk) < len(part_chunk):
                            chunk = os.read(
                                canonical_descriptor,
                                len(part_chunk) - len(canonical_chunk),
                            )
                            if not chunk:
                                fail(label)
                            canonical_chunk.extend(chunk)
                        if bytes(canonical_chunk) != part_chunk:
                            fail(label)
                finally:
                    os.close(part_descriptor)
            if os.read(canonical_descriptor, 1):
                fail(label)
        finally:
            os.close(canonical_descriptor)


def require_canonical_zstd_file(
    zstd: str,
    source: Path,
    observed: Path,
    *,
    label: str,
) -> None:
    with tempfile.TemporaryDirectory(prefix="goblins-canonical-zstd-") as temporary:
        canonical = Path(temporary) / "canonical.zst"
        compress_once(zstd, source, canonical)
        regular_files_are_identical(
            canonical,
            observed,
            maximum=MAX_COMPRESSED_ISO_BYTES,
            label=label,
        )


def require_canonical_zstd_parts(
    zstd: str,
    source: Path,
    parts: list[Path],
    *,
    label: str,
) -> None:
    with tempfile.TemporaryDirectory(prefix="goblins-canonical-zstd-parts-") as temporary:
        canonical = Path(temporary) / "canonical.zst"
        compress_once(zstd, source, canonical)
        canonical_file_matches_parts(canonical, parts, label=label)


def create_display_tar(source: Path, destination: Path, source_epoch: int) -> None:
    files = scan_flat_display_source(source)
    with destination.open("xb") as raw:
        with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            root_info = tarfile.TarInfo("goblins-os-aarch64-display-proof")
            root_info.type = tarfile.DIRTYPE
            root_info.mode = 0o755
            root_info.uid = 0
            root_info.gid = 0
            root_info.uname = ""
            root_info.gname = ""
            root_info.mtime = source_epoch
            archive.addfile(root_info)
            for path in files:
                data = read_regular(
                    path,
                    maximum=MAX_ZIP_MEMBER_BYTES,
                    label="display-proof archive member",
                )
                info = tarfile.TarInfo(f"goblins-os-aarch64-display-proof/{path.name}")
                info.size = len(data)
                info.mode = 0o644
                info.uid = 0
                info.gid = 0
                info.uname = ""
                info.gname = ""
                info.mtime = source_epoch
                archive.addfile(info, io.BytesIO(data))
        raw.flush()
        os.fsync(raw.fileno())


def split_file(source: Path, output: Path) -> list[Path]:
    parts: list[Path] = []
    with source.open("rb") as handle:
        index = 0
        while True:
            first = handle.read(1)
            if not first:
                break
            if index > 99:
                fail("compressed ISO requires more than the allowed 100 release parts")
            part = output / f"goblins-os-aarch64.iso.zst.part-{index:02d}"
            descriptor = os.open(part, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            written_total = 0
            try:
                chunk = first
                while chunk:
                    view = memoryview(chunk)
                    while view:
                        written = os.write(descriptor, view)
                        view = view[written:]
                        written_total += written
                    remaining = PART_BYTES - written_total
                    if remaining == 0:
                        break
                    chunk = handle.read(min(8 * 1024 * 1024, remaining))
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
            if written_total <= 0 or written_total > PART_BYTES:
                fail("compressed ISO splitter produced an invalid part size")
            parts.append(part)
            index += 1
    if not parts:
        fail("compressed ISO splitter produced no parts")
    return parts


def artifact_record(path: Path) -> dict[str, object]:
    before = path.lstat()
    digest = sha256_path(path)
    after = path.lstat()
    before_identity = (
        before.st_dev,
        before.st_ino,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    after_identity = (
        after.st_dev,
        after.st_ino,
        after.st_size,
        after.st_mtime_ns,
        after.st_ctime_ns,
    )
    if before_identity != after_identity:
        fail(f"release artifact changed while its manifest record was created: {path}")
    return {"name": path.name, "sha256": digest, "size_bytes": after.st_size}


def require_exact_display_member_records(
    replayed: list[dict[str, object]],
    declared: list[dict[str, object]],
) -> None:
    if replayed != declared:
        fail("post-build display-proof archive members differ from the promotion manifest")


def validate_timestamp(value: str) -> None:
    if RFC3339_RE.fullmatch(value) is None:
        fail("promotion timestamp must use canonical UTC RFC3339 seconds")
    try:
        parsed = dt.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)
    except ValueError as error:
        raise PromotionError("promotion timestamp is not a real UTC timestamp") from error
    if parsed.strftime("%Y-%m-%dT%H:%M:%SZ") != value:
        fail("promotion timestamp is not canonical")


def validate_run_date(value: str) -> None:
    if DATE_RE.fullmatch(value) is None:
        fail("display run date must use canonical YYYY-MM-DD form")
    try:
        parsed = dt.date.fromisoformat(value)
    except ValueError as error:
        raise PromotionError("display run date is not a real date") from error
    if parsed.isoformat() != value:
        fail("display run date is not canonical")


def workflow_run_url(run_id: str) -> str:
    if POSITIVE_INTEGER_RE.fullmatch(run_id) is None:
        fail("workflow run ID must be a positive base-10 integer")
    return f"{SOURCE_REPOSITORY}/actions/runs/{run_id}"


def workflow_run_id_from_url(value: str, *, label: str) -> str:
    match = re.fullmatch(
        rf"{re.escape(SOURCE_REPOSITORY)}/actions/runs/([1-9][0-9]*)",
        value,
    )
    if match is None:
        fail(f"{label} is not a canonical Goblins OS workflow run URL")
    return match.group(1)


def require_expected_promotion_identity(
    manifest: dict[str, object],
    *,
    candidate_commit: str,
    stable_tag: str,
    candidate_run_id: str,
    candidate_run_attempt: str,
    display_run_id: str,
    display_run_attempt: str,
    promotion_run_id: str,
    promotion_run_attempt: str,
    run_date: str,
    image_ref: str,
) -> None:
    if COMMIT_RE.fullmatch(candidate_commit) is None:
        fail("expected candidate commit is not canonical lowercase 40-hex")
    if STABLE_TAG_RE.fullmatch(stable_tag) is None:
        fail("expected stable tag is not canonical final SemVer")
    if IMAGE_RE.fullmatch(image_ref) is None:
        fail("expected image reference is not one immutable aarch64 image")
    validate_run_date(run_date)
    for label, value in (
        ("candidate workflow run attempt", candidate_run_attempt),
        ("display workflow run attempt", display_run_attempt),
        ("promotion workflow run attempt", promotion_run_attempt),
    ):
        if POSITIVE_INTEGER_RE.fullmatch(value) is None:
            fail(f"expected {label} must be a positive base-10 integer")
    candidate_run = workflow_run_url(candidate_run_id)
    display_run = workflow_run_url(display_run_id)
    promotion_run = workflow_run_url(promotion_run_id)
    candidate = manifest.get("candidate_workflow")
    display = manifest.get("display_verification_workflow")
    container = manifest.get("container")
    if type(container) is not dict or container.get("immutable_image_ref") != image_ref:
        fail("promotion manifest changes the caller-selected immutable image")
    candidate_expected = {
        "run": candidate_run,
        "run_attempt": int(candidate_run_attempt),
        "artifact_name": f"goblins-os-candidate-{candidate_commit}-aarch64",
    }
    display_expected = {
        "run": display_run,
        "run_attempt": int(display_run_attempt),
        "artifact_name": (
            f"aarch64-local-display-verified-{candidate_commit}-{run_date}-"
            f"attempt-{display_run_attempt}"
        ),
        "run_date": run_date,
    }
    if type(candidate) is not dict or set(candidate) != {
        *candidate_expected,
        "artifact_digest",
    }:
        fail("promotion manifest candidate workflow object is not exact")
    if type(display) is not dict or set(display) != {
        *display_expected,
        "artifact_digest",
    }:
        fail("promotion manifest display workflow object is not exact")
    if any(candidate.get(key) != value for key, value in candidate_expected.items()):
        fail("promotion manifest changes the expected candidate workflow identity")
    if any(display.get(key) != value for key, value in display_expected.items()):
        fail("promotion manifest changes the expected display workflow identity")
    for label, value in (
        ("candidate artifact", candidate.get("artifact_digest")),
        ("display artifact", display.get("artifact_digest")),
    ):
        if not isinstance(value, str) or re.fullmatch(r"sha256:[0-9a-f]{64}", value) is None:
            fail(f"promotion manifest {label} digest is not canonical")
    if (
        manifest.get("candidate_commit") != candidate_commit
        or manifest.get("stable_tag") != stable_tag
        or manifest.get("promotion_workflow_run") != promotion_run
        or manifest.get("promotion_workflow_run_attempt") != int(promotion_run_attempt)
    ):
        fail("promotion manifest changes the caller-selected promotion identity")


def expected_public_metadata(
    *,
    stable_tag: str,
    candidate_commit: str,
    promotion_run_id: str,
    image_ref: str,
    promotion_timestamp: str,
    raw_iso_sha256: str,
    raw_iso_size: int,
    compressed_iso_sha256: str,
    compressed_iso_size: int,
    part_records: list[dict[str, object]],
    built_on: object,
) -> tuple[dict[str, object], dict[str, object]]:
    validate_timestamp(promotion_timestamp)
    if not isinstance(built_on, str) or not built_on or len(built_on) > 256:
        fail("public ISO manifest has no bounded built_on identity")
    validate_timestamp(built_on)
    base_url = f"{SOURCE_REPOSITORY}/releases/download/{stable_tag}"
    expected_download_parts: list[dict[str, object]] = []
    for record in part_records:
        if type(record) is not dict or set(record) != {"name", "sha256", "size_bytes"}:
            fail("release ISO part record is malformed")
        name = record.get("name")
        digest = record.get("sha256")
        size = record.get("size_bytes")
        if (
            not isinstance(name, str)
            or PART_RE.fullmatch(name) is None
            or not isinstance(digest, str)
            or SHA256_RE.fullmatch(digest) is None
            or type(size) is not int
            or size <= 0
            or size > PART_BYTES
        ):
            fail("release ISO part record is not bounded and canonical")
        expected_download_parts.append(
            {
                "filename": name,
                "url": f"{base_url}/{name}",
                "sizeBytes": size,
                "sha256": digest,
            }
        )
    expected_container = {
        "immutable_image_ref": image_ref,
        "image_digest": image_ref.rsplit("@", 1)[1],
        "stable_tags": [
            "ghcr.io/joe-simo/goblins-os:aarch64",
            "ghcr.io/joe-simo/goblins-os:stable",
        ],
    }
    expected_website = {
        "releaseTag": stable_tag,
        "releaseBaseUrl": base_url,
        "releaseUrl": f"{SOURCE_REPOSITORY}/releases/tag/{stable_tag}",
        "releaseRunUrl": workflow_run_url(promotion_run_id),
        "targetCommit": candidate_commit,
        "preparedAt": promotion_timestamp,
        "releaseArtifact": {
            "arch": ARCHITECTURE,
            "label": "Arm / aarch64",
            "cpu": "UEFI aarch64 virtual machines. Bare-metal devices require model-specific proof.",
            "isoName": "goblins-os-aarch64.iso",
            "compressedName": "goblins-os-aarch64.iso.zst",
            "rawSizeBytes": raw_iso_size,
            "compressedSizeBytes": compressed_iso_size,
            "sha256": raw_iso_sha256,
            "compressedSha256": compressed_iso_sha256,
            "isoSha256Url": f"{base_url}/goblins-os-aarch64.iso.sha256",
            "compressedSha256Url": f"{base_url}/goblins-os-aarch64.iso.zst.sha256",
            "partsSha256Url": f"{base_url}/goblins-os-aarch64.iso.zst.parts.sha256",
            "manifestUrl": f"{base_url}/manifest-goblins-os-aarch64.json",
            "evidenceUrl": f"{base_url}/release-evidence-manifest-aarch64.json",
            "downloadParts": expected_download_parts,
            "builtOn": built_on,
            "status": "available",
        },
        "containerImage": {
            "arch": ARCHITECTURE,
            "image": "ghcr.io/joe-simo/goblins-os:aarch64",
            "platform": PLATFORM,
            "immutableImage": image_ref,
            "sourceManifestUrl": f"{base_url}/manifest-goblins-os-aarch64.json",
        },
    }
    return expected_container, expected_website


def require_exact_public_metadata(
    manifest: dict[str, object],
    *,
    stable_tag: str,
    candidate_commit: str,
    promotion_run_id: str,
    image_ref: str,
    promotion_timestamp: str,
    raw_iso_sha256: str,
    raw_iso_size: int,
    compressed_iso_sha256: str,
    compressed_iso_size: int,
    part_records: list[dict[str, object]],
    built_on: object,
) -> None:
    expected_container, expected_website = expected_public_metadata(
        stable_tag=stable_tag,
        candidate_commit=candidate_commit,
        promotion_run_id=promotion_run_id,
        image_ref=image_ref,
        promotion_timestamp=promotion_timestamp,
        raw_iso_sha256=raw_iso_sha256,
        raw_iso_size=raw_iso_size,
        compressed_iso_sha256=compressed_iso_sha256,
        compressed_iso_size=compressed_iso_size,
        part_records=part_records,
        built_on=built_on,
    )
    if manifest.get("container") != expected_container:
        fail("promotion manifest container metadata is not the exact selected image")
    if manifest.get("website") != expected_website:
        fail("promotion manifest website metadata is not exactly reproducible from release bytes")


def require_signed_display_iso_binding(
    run_dir: Path,
    seal: dict[str, object],
    record: dict[str, object],
    *,
    release_iso_sha256: str,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
) -> dict[str, object]:
    if SHA256_RE.fullmatch(release_iso_sha256) is None:
        fail("release ISO does not have one canonical SHA256")
    proof_path = run_dir / "proof-manifest.json"
    proof_data = read_regular(
        proof_path,
        maximum=MAX_JSON_BYTES,
        label="signed display proof manifest",
    )
    proof_manifest = load_json(proof_path, label="signed display proof manifest")
    expected_proof_identity = {
        "architecture": ARCHITECTURE,
        "candidate_commit": candidate_commit,
        "image_ref": image_ref,
        "iso": "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
        "iso_sha256": release_iso_sha256,
        "captured_at": f"{run_date}T00:00:00Z",
        "screenshot_run_dir": f"os/screenshots/hardware-gate/aarch64/{run_date}",
    }
    if any(proof_manifest.get(key) != value for key, value in expected_proof_identity.items()):
        fail("signed display proof manifest does not bind the exact release ISO")
    entries = seal.get("entries")
    if type(entries) is not list:
        fail("signed display seal has no canonical member entries")
    proof_entries = [
        entry
        for entry in entries
        if type(entry) is dict and entry.get("path") == "proof-manifest.json"
    ]
    expected_proof_entry = {
        "kind": "json",
        "path": "proof-manifest.json",
        "sha256": hashlib.sha256(proof_data).hexdigest(),
        "size": len(proof_data),
    }
    if proof_entries != [expected_proof_entry]:
        fail("display proof manifest is not one exact member of the signed evidence seal")
    if (
        seal.get("verification_iso_sha256") != release_iso_sha256
        or record.get("verification_iso_sha256") != release_iso_sha256
    ):
        fail("signed display authority identity does not bind the exact release ISO")
    return proof_manifest


def build_assets(args: argparse.Namespace) -> int:
    repository = Path(args.repository).resolve(strict=True)
    display_run = Path(args.display_run).resolve(strict=True)
    output = Path(args.output)
    if output.exists() or output.is_symlink():
        fail(f"promotion output must not already exist: {output}")
    require_exact_zstd(args.zstd)
    validate_timestamp(args.promotion_timestamp)
    remote = load_json(Path(args.remote_inputs), label="remote promotion inputs")
    if remote.get("candidate_commit") != args.candidate_commit or remote.get("stable_tag") != args.stable_tag:
        fail("remote inputs do not match the requested promotion")
    image_ref = remote.get("image_ref")
    if not isinstance(image_ref, str) or IMAGE_RE.fullmatch(image_ref) is None:
        fail("remote inputs have no immutable aarch64 image")
    remote_identity: dict[str, str] = {}
    for key, label in (
        ("candidate_run_id", "candidate workflow run ID"),
        ("candidate_run_attempt", "candidate workflow run attempt"),
        ("display_run_id", "display workflow run ID"),
        ("display_run_attempt", "display workflow run attempt"),
    ):
        value = remote.get(key)
        if type(value) is not int or value <= 0:
            fail(f"remote inputs have no canonical {label}")
        remote_identity[key] = str(value)
    remote_run_date = remote.get("run_date")
    if not isinstance(remote_run_date, str):
        fail("remote inputs have no canonical display run date")
    validate_run_date(remote_run_date)
    promotion_run_id = workflow_run_id_from_url(
        args.promotion_workflow_run,
        label="promotion workflow run",
    )
    if POSITIVE_INTEGER_RE.fullmatch(args.promotion_workflow_run_attempt) is None:
        fail("promotion workflow run attempt must be a positive base-10 integer")
    output.mkdir(mode=0o700, parents=True)
    temporary = Path(tempfile.mkdtemp(prefix="goblins-stable-build-", dir=output.parent))
    try:
        sources = {
            "goblins-os-aarch64.iso.sha256": repository
            / "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso.sha256",
            "manifest-goblins-os-aarch64.json": repository
            / "os/iso/output/aarch64/manifest-goblins-os-aarch64.json",
            "manifest-anaconda-iso-aarch64.json": repository
            / "os/iso/output/aarch64/manifest-anaconda-iso.json",
            "release-evidence-manifest-aarch64.json": repository
            / "os/signoff-proofs/sbom/aarch64/release-evidence-manifest.json",
            "cargo-lock-packages-aarch64.tsv": repository
            / "os/signoff-proofs/sbom/aarch64/cargo-lock-packages.tsv",
            "rpm-packages-aarch64.command": repository
            / "os/signoff-proofs/sbom/aarch64/rpm-packages.command",
            "rpm-packages-aarch64.tsv": repository
            / "os/signoff-proofs/sbom/aarch64/rpm-packages.tsv",
            "candidate-image-ref-aarch64.json": repository
            / "os/signoff-proofs/candidate/aarch64/image-ref.json",
            "evidence-bundle.json": display_run / "evidence-bundle.json",
            "aarch64-local-display-attestation.json": display_run
            / "aarch64-local-display-attestation.json",
            "aarch64-local-display-attestation.json.cms": display_run
            / "aarch64-local-display-attestation.json.cms",
            "display-proof-authority2.pem": display_run
            / "display-proof-authority2.pem",
            "display-proof-authority2.sha256": display_run
            / "display-proof-authority2.sha256",
            "display-proof-authority2-ca.pem": display_run
            / "display-proof-authority2-ca.pem",
            "display-proof-authority2-ca.sha256": display_run
            / "display-proof-authority2-ca.sha256",
            "final-shipping-gate-aarch64.log": Path(args.final_gate_log).resolve(strict=True),
            "signoff-notes-aarch64.md": repository / "os/signoff-notes.md",
        }
        for name, source in sources.items():
            copy_asset(source, output / name)

        source_epoch = int(args.source_date_epoch)
        if source_epoch <= 0:
            fail("source date epoch must be positive")
        display_tar = temporary / "goblins-os-aarch64-display-proof.tar"
        create_display_tar(display_run, display_tar, source_epoch)
        proof_archive = output / "goblins-os-aarch64-display-proof.tar.zst"
        compress_deterministically(args.zstd, display_tar, proof_archive)
        write_exclusive(
            output / "goblins-os-aarch64-display-proof.tar.zst.sha256",
            checksum_line(proof_archive),
        )

        iso = repository / "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso"
        compressed_iso = temporary / "goblins-os-aarch64.iso.zst"
        compress_deterministically(args.zstd, iso, compressed_iso)
        write_exclusive(
            output / "goblins-os-aarch64.iso.zst.sha256",
            checksum_line(compressed_iso),
        )
        parts = split_file(compressed_iso, output)
        parts_checksum = b"".join(checksum_line(part) for part in parts)
        write_exclusive(output / "goblins-os-aarch64.iso.zst.parts.sha256", parts_checksum)

        expected_names = FIXED_PAYLOAD_NAMES | {part.name for part in parts}
        actual_names = {path.name for path in output.iterdir()}
        if actual_names != expected_names:
            fail(f"release payload allowlist mismatch: {sorted(actual_names ^ expected_names)}")
        if any(re.search(r"(?:x86|amd64)", name, flags=re.IGNORECASE) for name in actual_names):
            fail("release payload contains a forbidden x86 or amd64 asset name")

        iso_manifest = load_json(output / "manifest-goblins-os-aarch64.json", label="ISO manifest")
        raw_iso_sha = sha256_path(iso)
        checksum_iso_sha = parse_single_checksum(
            output / "goblins-os-aarch64.iso.sha256", "goblins-os-aarch64.iso"
        )
        if checksum_iso_sha != raw_iso_sha:
            fail("candidate ISO checksum does not match the raw release ISO")
        decompressed_iso_sha, decompressed_iso_size = stream_decompressed_record(
            args.zstd,
            parts,
            maximum=MAX_ZIP_MEMBER_BYTES,
            label="newly compressed release ISO",
        )
        if decompressed_iso_sha != raw_iso_sha or decompressed_iso_size != iso.stat().st_size:
            fail("newly compressed release ISO does not reproduce the raw release ISO")
        compressed_iso_sha = sha256_path(compressed_iso)
        base_url = f"{SOURCE_REPOSITORY}/releases/download/{args.stable_tag}"
        release_url = f"{SOURCE_REPOSITORY}/releases/tag/{args.stable_tag}"
        part_records = [artifact_record(part) for part in parts]
        release_assets = [artifact_record(output / name) for name in sorted(actual_names)]
        display_seal = load_json(display_run / "evidence-bundle.json", label="display seal")
        display_record = load_json(
            display_run / "aarch64-local-display-attestation.json",
            label="display authority record",
        )
        framebuffer = display_seal.get("framebuffer")
        screenshot_count = framebuffer.get("required_png_count") if type(framebuffer) is dict else None
        authority_certificate_sha256 = pinned_certificate_fingerprint(
            read_regular(
                display_run / "display-proof-authority2.pem",
                maximum=64 * 1024,
                label="Authority 2 leaf certificate",
            ),
            read_regular(
                display_run / "display-proof-authority2.sha256",
                maximum=256,
                label="Authority 2 leaf fingerprint",
            ),
            label="Authority 2 leaf",
        )
        authority_ca_certificate_sha256 = pinned_certificate_fingerprint(
            read_regular(
                display_run / "display-proof-authority2-ca.pem",
                maximum=64 * 1024,
                label="Authority 2 CA certificate",
            ),
            read_regular(
                display_run / "display-proof-authority2-ca.sha256",
                maximum=256,
                label="Authority 2 CA fingerprint",
            ),
            label="Authority 2 offline CA",
        )
        require_signed_display_iso_binding(
            display_run,
            display_seal,
            display_record,
            release_iso_sha256=raw_iso_sha,
            candidate_commit=args.candidate_commit,
            image_ref=image_ref,
            run_date=str(remote["run_date"]),
        )
        if (
            display_seal.get("schema") != EVIDENCE_SCHEMA
            or display_record.get("schema") != DISPLAY_SCHEMA
            or display_record.get("authority_generation")
            != DISPLAY_AUTHORITY_GENERATION
            or display_record.get("authority_certificate_sha256")
            != authority_certificate_sha256
            or display_record.get("authority_ca_certificate_sha256")
            != authority_ca_certificate_sha256
            or display_record.get("signature_purpose") != DISPLAY_PURPOSE
            or display_seal.get("candidate_commit") != args.candidate_commit
            or display_record.get("candidate_commit") != args.candidate_commit
            or display_seal.get("image_ref") != image_ref
            or display_record.get("image_ref") != image_ref
            or display_seal.get("run_date") != remote["run_date"]
            or display_record.get("run_date") != remote["run_date"]
            or screenshot_count != DISPLAY_SCREENSHOT_COUNT
            or display_record.get("screenshot_count") != screenshot_count
        ):
            fail("display proof changed identity before promotion manifest creation")
        display_member_records = [
            artifact_record(path) for path in scan_flat_display_source(display_run)
        ]
        proof_archive_record = artifact_record(proof_archive)
        manifest = {
            "schema": PROMOTION_SCHEMA,
            "product": "Goblins OS",
            "architecture": ARCHITECTURE,
            "platform": PLATFORM,
            "candidate_commit": args.candidate_commit,
            "stable_tag": args.stable_tag,
            "promotion_timestamp": args.promotion_timestamp,
            "source_date_epoch": source_epoch,
            "source_repository": SOURCE_REPOSITORY,
            "promotion_workflow_run": args.promotion_workflow_run,
            "promotion_workflow_run_attempt": int(args.promotion_workflow_run_attempt),
            "candidate_workflow": {
                "run": f"{SOURCE_REPOSITORY}/actions/runs/{remote['candidate_run_id']}",
                "run_attempt": remote["candidate_run_attempt"],
                "artifact_name": remote["candidate_artifact_name"],
                "artifact_digest": remote["candidate_artifact_digest"],
            },
            "display_verification_workflow": {
                "run": f"{SOURCE_REPOSITORY}/actions/runs/{remote['display_run_id']}",
                "run_attempt": remote["display_run_attempt"],
                "artifact_name": remote["display_artifact_name"],
                "artifact_digest": remote["display_artifact_digest"],
                "run_date": remote["run_date"],
            },
            "display_proof": {
                "archive": proof_archive_record,
                "authority_generation": DISPLAY_AUTHORITY_GENERATION,
                "authority_certificate_sha256": authority_certificate_sha256,
                "authority_ca_certificate_sha256": authority_ca_certificate_sha256,
                "authority_files": [
                    artifact_record(display_run / name)
                    for name in AUTHORITY_PUBLIC_FILES
                ],
                "authority_record_sha256": sha256_path(
                    display_run / "aarch64-local-display-attestation.json"
                ),
                "authority_schema": DISPLAY_SCHEMA,
                "authority_signature_sha256": sha256_path(
                    display_run / "aarch64-local-display-attestation.json.cms"
                ),
                "evidence_bundle_sha256": sha256_path(display_run / "evidence-bundle.json"),
                "evidence_schema": EVIDENCE_SCHEMA,
                "member_count": len(display_member_records),
                "members": display_member_records,
                "run_date": remote["run_date"],
                "screenshot_count": screenshot_count,
                "screenshot_manifest_sha256": display_seal.get(
                    "screenshot_manifest_sha256"
                ),
                "verification_iso_sha256": raw_iso_sha,
            },
            "container": {
                "immutable_image_ref": image_ref,
                "image_digest": image_ref.rsplit("@", 1)[1],
                "stable_tags": [
                    "ghcr.io/joe-simo/goblins-os:aarch64",
                    "ghcr.io/joe-simo/goblins-os:stable",
                ],
            },
            "compression": {
                "format": "zstd",
                "command": "zstd --ultra -19 --long=31 -T1 --no-progress",
                "tool_version": tool_version,
                "determinism_replay": "pass",
                "part_size_bytes": PART_BYTES,
            },
            "release_assets": release_assets,
            "website": {
                "releaseTag": args.stable_tag,
                "releaseBaseUrl": base_url,
                "releaseUrl": release_url,
                "releaseRunUrl": args.promotion_workflow_run,
                "targetCommit": args.candidate_commit,
                "preparedAt": args.promotion_timestamp,
                "releaseArtifact": {
                    "arch": ARCHITECTURE,
                    "label": "Arm / aarch64",
                    "cpu": "UEFI aarch64 virtual machines. Bare-metal devices require model-specific proof.",
                    "isoName": "goblins-os-aarch64.iso",
                    "compressedName": "goblins-os-aarch64.iso.zst",
                    "rawSizeBytes": iso.stat().st_size,
                    "compressedSizeBytes": compressed_iso.stat().st_size,
                    "sha256": raw_iso_sha,
                    "compressedSha256": compressed_iso_sha,
                    "isoSha256Url": f"{base_url}/goblins-os-aarch64.iso.sha256",
                    "compressedSha256Url": f"{base_url}/goblins-os-aarch64.iso.zst.sha256",
                    "partsSha256Url": f"{base_url}/goblins-os-aarch64.iso.zst.parts.sha256",
                    "manifestUrl": f"{base_url}/manifest-goblins-os-aarch64.json",
                    "evidenceUrl": f"{base_url}/release-evidence-manifest-aarch64.json",
                    "downloadParts": [
                        {
                            "filename": item["name"],
                            "url": f"{base_url}/{item['name']}",
                            "sizeBytes": item["size_bytes"],
                            "sha256": item["sha256"],
                        }
                        for item in part_records
                    ],
                    "builtOn": iso_manifest.get("built_on"),
                    "status": "available",
                },
                "containerImage": {
                    "arch": ARCHITECTURE,
                    "image": "ghcr.io/joe-simo/goblins-os:aarch64",
                    "platform": PLATFORM,
                    "immutableImage": image_ref,
                    "sourceManifestUrl": f"{base_url}/manifest-goblins-os-aarch64.json",
                },
            },
        }
        write_exclusive(output / "promotion-manifest.json", canonical_json(manifest))
        write_exclusive(
            output / "promotion-manifest.sha256",
            checksum_line(output / "promotion-manifest.json"),
        )
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        shutil.rmtree(temporary, ignore_errors=True)
    verify_payload_directory(
        output,
        candidate_commit=args.candidate_commit,
        stable_tag=args.stable_tag,
        candidate_run_id=remote_identity["candidate_run_id"],
        candidate_run_attempt=remote_identity["candidate_run_attempt"],
        display_run_id=remote_identity["display_run_id"],
        display_run_attempt=remote_identity["display_run_attempt"],
        promotion_run_id=promotion_run_id,
        promotion_run_attempt=args.promotion_workflow_run_attempt,
        run_date=remote_run_date,
        image_ref=image_ref,
        zstd=args.zstd,
        repository=repository,
    )
    return 0


def parse_single_checksum(path: Path, expected_name: str) -> str:
    data = read_regular(path, maximum=1024 * 1024, label="checksum file")
    try:
        text = data.decode("ascii")
    except UnicodeDecodeError as error:
        raise PromotionError(f"checksum file is not ASCII: {path}") from error
    match = re.fullmatch(rf"([0-9a-f]{{64}})  {re.escape(expected_name)}\n", text)
    if match is None:
        fail(f"checksum file is not canonical for {expected_name}: {path}")
    return match.group(1)


def pump_zstd_decompression(
    zstd: str,
    inputs: list[PinnedInput],
    *,
    maximum: int,
    label: str,
    destination_descriptor: int | None = None,
) -> tuple[str, int]:
    if not inputs or maximum <= 0:
        fail(f"{label} has no bounded compressed input")
    with tempfile.TemporaryFile() as error_output:
        process = subprocess.Popen(
            [zstd, "--decompress", "--stdout", "--long=31"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=error_output,
        )
        assert process.stdin is not None and process.stdout is not None
        writer_error: list[BaseException] = []

        def feed() -> None:
            try:
                for pinned in inputs:
                    assert_pinned_input_stable(pinned, action="before zstd input streaming")
                    duplicate = os.dup(pinned.descriptor)
                    try:
                        os.lseek(duplicate, 0, os.SEEK_SET)
                        while True:
                            chunk = os.read(duplicate, 1024 * 1024)
                            if not chunk:
                                break
                            view = memoryview(chunk)
                            while view:
                                written = process.stdin.write(view)
                                if written is None or written <= 0:
                                    raise OSError("zstd input pipe stopped accepting bytes")
                                view = view[written:]
                    finally:
                        os.close(duplicate)
                process.stdin.close()
            except BaseException as error:  # pragma: no cover - surfaced below
                writer_error.append(error)
                try:
                    process.stdin.close()
                except OSError:
                    pass

        writer = threading.Thread(target=feed, name="zstd-part-feeder", daemon=True)
        writer.start()
        digest = hashlib.sha256()
        total = 0
        overflow = False
        timed_out = False
        stream_error: BaseException | None = None
        deadline = time.monotonic() + ZSTD_TIMEOUT_SECONDS
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    timed_out = True
                    break
                if not selector.select(remaining):
                    timed_out = True
                    break
                chunk = os.read(process.stdout.fileno(), 1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > maximum:
                    overflow = True
                    break
                digest.update(chunk)
                if destination_descriptor is not None:
                    view = memoryview(chunk)
                    while view:
                        written = os.write(destination_descriptor, view)
                        view = view[written:]
        except BaseException as error:
            stream_error = error
        finally:
            selector.close()
            if overflow or timed_out or stream_error is not None:
                process.kill()
            process.stdout.close()
            remaining = max(0.1, deadline - time.monotonic())
            try:
                returncode = process.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                timed_out = True
                process.kill()
                returncode = process.wait(timeout=30)
            writer.join(timeout=30)
        error_output.seek(0)
        error_text = error_output.read(2000).decode("utf-8", "replace")
        if stream_error is not None:
            raise stream_error
        if overflow:
            fail(f"{label} exceeds its {maximum}-byte decompression limit")
        if timed_out:
            fail(f"{label} exceeded its bounded zstd execution deadline")
        if writer.is_alive() or writer_error:
            fail(f"failed to stream {label}: {writer_error!r}")
        if returncode != 0:
            fail(f"zstd could not decode {label}: {error_text}")
        return digest.hexdigest(), total


def stream_decompressed_record(
    zstd: str,
    parts: list[Path],
    *,
    maximum: int,
    label: str,
) -> tuple[str, int]:
    if not parts:
        fail(f"{label} has no compressed input parts")
    with ExitStack() as stack:
        pinned_inputs = [
            stack.enter_context(
                pin_regular_input(
                    part,
                    maximum=MAX_COMPRESSED_ISO_BYTES,
                    label=f"{label} compressed input {part.name}",
                )
            )
            for part in parts
        ]
        if sum(item.size_bytes for item in pinned_inputs) > MAX_COMPRESSED_ISO_BYTES:
            fail(f"{label} compressed inputs exceed their total byte limit")
        return pump_zstd_decompression(
            zstd,
            pinned_inputs,
            maximum=maximum,
            label=label,
        )


def decompress_zstd_inputs_bounded(
    zstd: str,
    sources: list[Path],
    destination: Path,
    *,
    maximum: int,
    label: str,
) -> int:
    if not sources or maximum <= 0 or destination.exists() or destination.is_symlink():
        fail(f"{label} has no safe bounded decompression destination")
    with ExitStack() as stack:
        pinned_inputs = [
            stack.enter_context(
                pin_regular_input(
                    source,
                    maximum=MAX_COMPRESSED_ISO_BYTES,
                    label=f"{label} compressed input {source.name}",
                )
            )
            for source in sources
        ]
        if sum(item.size_bytes for item in pinned_inputs) > MAX_COMPRESSED_ISO_BYTES:
            fail(f"{label} compressed inputs exceed their total byte limit")
        descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        completed = False
        try:
            _, total = pump_zstd_decompression(
                zstd,
                pinned_inputs,
                maximum=maximum,
                label=label,
                destination_descriptor=descriptor,
            )
            os.fsync(descriptor)
            completed = True
            return total
        finally:
            os.close(descriptor)
            if not completed:
                destination.unlink(missing_ok=True)


def decompress_zstd_file_bounded(
    zstd: str,
    source: Path,
    destination: Path,
    *,
    maximum: int,
    label: str,
) -> int:
    return decompress_zstd_inputs_bounded(
        zstd,
        [source],
        destination,
        maximum=maximum,
        label=label,
    )


def extract_display_proof_archive(
    zstd: str,
    archive: Path,
    destination: Path,
    *,
    source_epoch: int,
) -> Path:
    if source_epoch <= 0:
        fail("display-proof raw USTAR source epoch is not positive")
    if destination.exists() or destination.is_symlink():
        fail(f"display-proof replay destination already exists: {destination}")
    destination.mkdir(mode=0o700, parents=True)
    tar_path = destination / "display-proof.tar"
    decompress_zstd_file_bounded(
        zstd,
        archive,
        tar_path,
        maximum=MAX_DISPLAY_TAR_BYTES,
        label="display-proof archive",
    )
    run_dir = destination / "repository/os/screenshots/hardware-gate/aarch64/replay"
    run_dir.mkdir(mode=0o755, parents=True)
    seen: set[str] = set()
    total = 0
    root_seen = False
    with tarfile.open(tar_path, mode="r:") as archive_handle:
        for member in archive_handle:
            pure = PurePosixPath(member.name)
            if member.name == "goblins-os-aarch64-display-proof":
                if root_seen or not member.isdir():
                    fail("display-proof archive has an invalid root directory")
                root_seen = True
                continue
            if (
                len(pure.parts) != 2
                or pure.parts[0] != "goblins-os-aarch64-display-proof"
                or pure.name in ("", ".", "..")
                or "\\" in member.name
                or pure.name in seen
            ):
                fail(f"display-proof archive contains an unsafe or duplicate member: {member.name!r}")
            if not member.isreg() or member.issym() or member.islnk():
                fail(f"display-proof archive contains a non-regular member: {member.name!r}")
            if member.size <= 0 or member.size > MAX_ZIP_MEMBER_BYTES:
                fail(f"display-proof archive member has an invalid size: {member.name!r}")
            total += member.size
            if total > MAX_DISPLAY_TOTAL_BYTES:
                fail("display-proof archive exceeds its fixed expansion limit")
            source = archive_handle.extractfile(member)
            if source is None:
                fail(f"display-proof archive member could not be read: {member.name!r}")
            target = run_dir / pure.name
            descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            try:
                written = 0
                while True:
                    chunk = source.read(1024 * 1024)
                    if not chunk:
                        break
                    written += len(chunk)
                    if written > member.size:
                        fail(f"display-proof member exceeded its declared size: {member.name!r}")
                    view = memoryview(chunk)
                    while view:
                        count = os.write(descriptor, view)
                        view = view[count:]
                if written != member.size:
                    fail(f"display-proof member size changed during replay: {member.name!r}")
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
                source.close()
            seen.add(pure.name)
    if not root_seen:
        fail("display-proof archive has no canonical root directory")
    required = set(DISPLAY_FILES) | {"signoff-row.md"}
    if not required.issubset(seen):
        fail(f"display-proof archive is incomplete: {sorted(required - seen)}")
    canonical_tar = destination / "canonical-display-proof.tar"
    create_display_tar(run_dir, canonical_tar, source_epoch)
    regular_files_are_identical(
        tar_path,
        canonical_tar,
        maximum=MAX_DISPLAY_TAR_BYTES,
        label="display-proof raw USTAR bytes are not canonical",
    )
    require_canonical_zstd_file(
        zstd,
        canonical_tar,
        archive,
        label="display-proof zstd bytes are not canonical",
    )
    tar_path.unlink()
    canonical_tar.unlink()
    return run_dir


def validate_members_against_signed_seal(
    run_dir: Path,
    *,
    standalone_root: Path | None = None,
) -> dict[str, object]:
    seal_path = run_dir / "evidence-bundle.json"
    record_path = run_dir / "aarch64-local-display-attestation.json"
    seal_data = read_regular(seal_path, maximum=MAX_JSON_BYTES, label="replayed evidence bundle")
    seal = load_json(seal_path, label="replayed evidence bundle")
    record = load_json(record_path, label="replayed display authority record")
    if seal.get("schema") != EVIDENCE_SCHEMA:
        fail("replayed display proof uses an unsupported evidence schema")
    if record.get("schema") != DISPLAY_SCHEMA:
        fail("replayed display proof uses an unsupported authority schema")
    if (
        record.get("authority_generation") != DISPLAY_AUTHORITY_GENERATION
        or record.get("signature_purpose") != DISPLAY_PURPOSE
    ):
        fail("replayed display proof is not an Authority 2 record")
    entries = seal.get("entries")
    if type(entries) is not list or seal.get("entry_count") != len(entries):
        fail("replayed evidence bundle has an invalid entry count")
    observed_paths: set[str] = set()
    screenshot_entries: list[dict[str, object]] = []
    for entry in entries:
        if type(entry) is not dict:
            fail("replayed evidence bundle has a malformed entry")
        name = entry.get("path")
        digest = entry.get("sha256")
        size = entry.get("size")
        kind = entry.get("kind")
        if (
            not isinstance(name, str)
            or PurePosixPath(name).name != name
            or name in ("", ".", "..")
            or name in observed_paths
            or not isinstance(digest, str)
            or SHA256_RE.fullmatch(digest) is None
            or type(size) is not int
            or size <= 0
            or kind not in ("json", "png")
        ):
            fail("replayed evidence bundle has an unsafe entry")
        member = run_dir / name
        if member.stat().st_size != size or sha256_path(member) != digest:
            fail(f"replayed display-proof member no longer matches the signed seal: {name}")
        observed_paths.add(name)
        if kind == "png":
            screenshot_entries.append(entry)
    framebuffer = seal.get("framebuffer")
    if (
        type(framebuffer) is not dict
        or framebuffer.get("required_png_count") != len(screenshot_entries)
        or record.get("screenshot_count") != len(screenshot_entries)
        or len(screenshot_entries) != DISPLAY_SCREENSHOT_COUNT
    ):
        fail("replayed display-proof screenshot count is inconsistent")
    if tuple(str(entry.get("path")) for entry in screenshot_entries) != DISPLAY_SCREENSHOT_NAMES:
        fail("replayed display proof does not contain the exact canonical screenshot set")
    if "proof-manifest.json" not in observed_paths:
        fail("replayed display proof does not seal proof-manifest.json")
    screenshot_manifest = hashlib.sha256(canonical_json(screenshot_entries)).hexdigest()
    if (
        seal.get("screenshot_manifest_sha256") != screenshot_manifest
        or record.get("screenshot_manifest_sha256") != screenshot_manifest
    ):
        fail("replayed screenshot manifest no longer matches the signed seal")
    if (
        record.get("evidence_bundle_sha256") != hashlib.sha256(seal_data).hexdigest()
        or record.get("evidence_bundle_size") != len(seal_data)
    ):
        fail("replayed authority record no longer binds the evidence bundle")
    authority_fingerprint = pinned_certificate_fingerprint(
        read_regular(
            run_dir / "display-proof-authority2.pem",
            maximum=64 * 1024,
            label="replayed Authority 2 leaf",
        ),
        read_regular(
            run_dir / "display-proof-authority2.sha256",
            maximum=256,
            label="replayed Authority 2 leaf fingerprint",
        ),
        label="replayed Authority 2 leaf",
    )
    authority_ca_fingerprint = pinned_certificate_fingerprint(
        read_regular(
            run_dir / "display-proof-authority2-ca.pem",
            maximum=64 * 1024,
            label="replayed Authority 2 CA",
        ),
        read_regular(
            run_dir / "display-proof-authority2-ca.sha256",
            maximum=256,
            label="replayed Authority 2 CA fingerprint",
        ),
        label="replayed Authority 2 offline CA",
    )
    if (
        record.get("authority_certificate_sha256") != authority_fingerprint
        or record.get("authority_ca_certificate_sha256")
        != authority_ca_fingerprint
    ):
        fail("replayed Authority 2 record does not bind the pinned leaf and CA")
    for key in (
        "architecture",
        "candidate_commit",
        "image_ref",
        "run_date",
        "verification_iso_sha256",
    ):
        if record.get(key) != seal.get(key):
            fail(f"replayed authority record changes signed identity field: {key}")
    if standalone_root is not None:
        for name in DISPLAY_FILES:
            if sha256_path(run_dir / name) != sha256_path(standalone_root / name):
                fail(f"standalone release proof differs from archived signed bytes: {name}")
    return seal


def replay_and_verify_display_proof(
    payload: Path,
    *,
    zstd: str,
    repository: Path,
    source_epoch: int,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    release_iso_sha256: str,
) -> list[dict[str, object]]:
    replay_root = Path(tempfile.mkdtemp(prefix="goblins-display-proof-replay-"))
    try:
        replay_run = extract_display_proof_archive(
            zstd,
            payload / "goblins-os-aarch64-display-proof.tar.zst",
            replay_root / "expanded",
            source_epoch=source_epoch,
        )
        seal = validate_members_against_signed_seal(replay_run, standalone_root=payload)
        sealed_run_date = seal.get("run_date")
        if (
            seal.get("architecture") != ARCHITECTURE
            or seal.get("candidate_commit") != candidate_commit
            or seal.get("image_ref") != image_ref
            or sealed_run_date != run_date
        ):
            fail("replayed display-proof archive changes the promotion identity")
        record = load_json(
            replay_run / "aarch64-local-display-attestation.json",
            label="replayed display authority record",
        )
        require_signed_display_iso_binding(
            replay_run,
            seal,
            record,
            release_iso_sha256=release_iso_sha256,
            candidate_commit=candidate_commit,
            image_ref=image_ref,
            run_date=run_date,
        )
        canonical_run = replay_root / "expanded/repository/os/screenshots/hardware-gate/aarch64" / run_date
        replay_run.rename(canonical_run)
        evidence_helper = repository / "os/hardware-gate/capture-harness/evidence_bundle.py"
        subprocess.run(
            [
                "python3",
                str(evidence_helper),
                "verify",
                "--repository",
                str(replay_root / "expanded/repository"),
                "--run-dir",
                str(canonical_run),
                "--architecture",
                ARCHITECTURE,
                "--candidate-commit",
                candidate_commit,
                "--image-ref",
                image_ref,
                "--run-date",
                run_date,
            ],
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=120,
        )
        subprocess.run(
            [
                "python3",
                str(evidence_helper),
                "verify-attestation",
                "--seal",
                str(canonical_run / "evidence-bundle.json"),
                "--record",
                str(canonical_run / "aarch64-local-display-attestation.json"),
                "--signature",
                str(canonical_run / "aarch64-local-display-attestation.json.cms"),
                "--certificate",
                str(canonical_run / "display-proof-authority2.pem"),
                "--certificate-sha256",
                str(canonical_run / "display-proof-authority2.sha256"),
                "--ca-certificate",
                str(canonical_run / "display-proof-authority2-ca.pem"),
                "--ca-certificate-sha256",
                str(canonical_run / "display-proof-authority2-ca.sha256"),
                "--candidate-commit",
                candidate_commit,
                "--image-ref",
                image_ref,
                "--run-date",
                run_date,
            ],
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=120,
        )
        return [artifact_record(path) for path in scan_flat_display_source(canonical_run)]
    finally:
        shutil.rmtree(replay_root, ignore_errors=True)


def verify_payload_directory(
    payload: Path,
    *,
    candidate_commit: str,
    stable_tag: str,
    candidate_run_id: str,
    candidate_run_attempt: str,
    display_run_id: str,
    display_run_attempt: str,
    promotion_run_id: str,
    promotion_run_attempt: str,
    run_date: str,
    image_ref: str,
    zstd: str,
    repository: Path,
) -> dict[str, object]:
    if payload.is_symlink() or not payload.is_dir():
        fail(f"promotion payload is not a regular directory: {payload}")
    entries = sorted(payload.iterdir(), key=lambda item: item.name.encode("utf-8"))
    for entry in entries:
        metadata = entry.lstat()
        if not stat.S_ISREG(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode) or metadata.st_nlink != 1:
            fail(f"promotion payload contains a non-regular or linked entry: {entry}")
    names = {entry.name for entry in entries}
    part_names = sorted(name for name in names if PART_RE.fullmatch(name))
    if not part_names:
        fail("promotion payload has no compressed ISO parts")
    expected_parts = [f"goblins-os-aarch64.iso.zst.part-{index:02d}" for index in range(len(part_names))]
    if part_names != expected_parts:
        fail("promotion payload ISO part numbering is not contiguous from part-00")
    actual_compressed_iso_size = 0
    for index, name in enumerate(part_names):
        part_size = (payload / name).lstat().st_size
        if (
            part_size <= 0
            or part_size > PART_BYTES
            or (index < len(part_names) - 1 and part_size != PART_BYTES)
        ):
            fail("compressed ISO part sizes do not match the deterministic splitter contract")
        actual_compressed_iso_size += part_size
        if actual_compressed_iso_size > MAX_COMPRESSED_ISO_BYTES:
            fail("compressed ISO parts exceed the fixed total input byte limit")
    expected_names = FIXED_PAYLOAD_NAMES | CONTROL_PAYLOAD_NAMES | set(part_names)
    if names != expected_names:
        fail(f"promotion payload allowlist mismatch; unexpected={sorted(names - expected_names)}, missing={sorted(expected_names - names)}")
    if any(re.search(r"(?:x86|amd64)", name, flags=re.IGNORECASE) for name in names):
        fail("promotion payload contains a forbidden x86 or amd64 asset name")

    final_gate_data = read_regular(
        payload / "final-shipping-gate-aarch64.log",
        maximum=16 * 1024 * 1024,
        label="final shipping gate log",
    )
    try:
        final_gate_text = final_gate_data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise PromotionError("final shipping gate log is not UTF-8") from error
    if final_gate_text.splitlines().count("Shipping status gate: PASS") != 1:
        fail("final shipping gate log has no unique canonical success marker")

    signoff_data = read_regular(
        payload / "signoff-notes-aarch64.md",
        maximum=16 * 1024 * 1024,
        label="aarch64 signoff notes",
    )
    try:
        signoff_text = signoff_data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise PromotionError("aarch64 signoff notes are not UTF-8") from error
    manual_blocks = re.split(r"(?m)(?=^## Manual Gate Run: )", signoff_text)
    current_blocks = [block for block in manual_blocks if block.startswith("## Manual Gate Run: ")]
    if not current_blocks:
        fail("aarch64 signoff notes have no canonical manual gate block")
    current_signoff = current_blocks[-1]
    next_heading = re.search(r"(?m)^## (?!Manual Gate Run: ).+$", current_signoff)
    if next_heading is not None:
        current_signoff = current_signoff[: next_heading.start()]
    required_signoff_lines = {
        "- Architecture: aarch64",
        f"- Candidate/source commit: {candidate_commit}",
        "- Verify result (blocked=0): pass",
        "- Self-test result: pass",
        "- Current project completion status: complete",
    }
    signoff_lines = current_signoff.splitlines()
    for required_line in required_signoff_lines:
        if signoff_lines.count(required_line) != 1:
            fail(f"current aarch64 signoff block is missing unique line: {required_line}")

    manifest = load_json(payload / "promotion-manifest.json", label="promotion manifest")
    expected_manifest_keys = {
        "schema",
        "product",
        "architecture",
        "platform",
        "candidate_commit",
        "stable_tag",
        "promotion_timestamp",
        "source_date_epoch",
        "source_repository",
        "promotion_workflow_run",
        "promotion_workflow_run_attempt",
        "candidate_workflow",
        "display_verification_workflow",
        "display_proof",
        "container",
        "compression",
        "release_assets",
        "website",
    }
    if set(manifest) != expected_manifest_keys:
        fail("promotion manifest top-level key set is not exact")
    expected_manifest = {
        "schema": PROMOTION_SCHEMA,
        "product": "Goblins OS",
        "architecture": ARCHITECTURE,
        "platform": PLATFORM,
        "candidate_commit": candidate_commit,
        "stable_tag": stable_tag,
        "source_repository": SOURCE_REPOSITORY,
    }
    if any(manifest.get(key) != value for key, value in expected_manifest.items()):
        fail("promotion manifest does not bind the exact requested stable candidate")
    require_expected_promotion_identity(
        manifest,
        candidate_commit=candidate_commit,
        stable_tag=stable_tag,
        candidate_run_id=candidate_run_id,
        candidate_run_attempt=candidate_run_attempt,
        display_run_id=display_run_id,
        display_run_attempt=display_run_attempt,
        promotion_run_id=promotion_run_id,
        promotion_run_attempt=promotion_run_attempt,
        run_date=run_date,
        image_ref=image_ref,
    )
    promotion_timestamp = manifest.get("promotion_timestamp")
    if not isinstance(promotion_timestamp, str):
        fail("promotion manifest has no canonical promotion timestamp")
    validate_timestamp(promotion_timestamp)
    source_date_epoch = manifest.get("source_date_epoch")
    if type(source_date_epoch) is not int or source_date_epoch <= 0:
        fail("promotion manifest source date epoch is not positive")
    if manifest.get("compression") != {
        "format": "zstd",
        "command": "zstd --ultra -19 --long=31 -T1 --no-progress",
        "tool_version": ZSTD_VERSION,
        "determinism_replay": "pass",
        "part_size_bytes": PART_BYTES,
    }:
        fail("promotion manifest compression contract is not exact")
    if canonical_json(manifest) != read_regular(
        payload / "promotion-manifest.json", maximum=MAX_JSON_BYTES, label="promotion manifest"
    ):
        fail("promotion manifest is not canonical JSON")
    manifest_sha = parse_single_checksum(payload / "promotion-manifest.sha256", "promotion-manifest.json")
    if manifest_sha != sha256_path(payload / "promotion-manifest.json"):
        fail("promotion manifest checksum does not match")

    assets = manifest.get("release_assets")
    if type(assets) is not list:
        fail("promotion manifest has no release asset list")
    expected_asset_names = names - CONTROL_PAYLOAD_NAMES
    observed: dict[str, dict[str, object]] = {}
    for item in assets:
        if type(item) is not dict or set(item) != {"name", "sha256", "size_bytes"}:
            fail("promotion manifest has a malformed release asset record")
        name = item.get("name")
        if not isinstance(name, str) or name in observed:
            fail("promotion manifest has a duplicate or invalid asset name")
        observed[name] = item
    if set(observed) != expected_asset_names:
        fail("promotion manifest release asset list does not match the exact allowlist")
    for name, item in observed.items():
        path = payload / name
        if item.get("sha256") != sha256_path(path) or item.get("size_bytes") != path.stat().st_size:
            fail(f"promotion manifest does not bind the exact bytes of {name}")

    parts_checksum_data = read_regular(
        payload / "goblins-os-aarch64.iso.zst.parts.sha256",
        maximum=1024 * 1024,
        label="parts checksum",
    )
    expected_parts_checksum = b"".join(checksum_line(payload / name) for name in part_names)
    if parts_checksum_data != expected_parts_checksum:
        fail("compressed ISO parts checksum is not canonical or exact")
    compressed_digest = hashlib.sha256()
    for name in part_names:
        with (payload / name).open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                compressed_digest.update(chunk)
    actual_compressed_iso_sha = compressed_digest.hexdigest()
    expected_compressed_iso_sha = parse_single_checksum(
        payload / "goblins-os-aarch64.iso.zst.sha256", "goblins-os-aarch64.iso.zst"
    )
    if actual_compressed_iso_sha != expected_compressed_iso_sha:
        fail("concatenated compressed ISO parts do not match the compressed checksum")
    expected_release_iso_sha = parse_single_checksum(
        payload / "goblins-os-aarch64.iso.sha256", "goblins-os-aarch64.iso"
    )
    release_iso_parts = [payload / name for name in part_names]
    with tempfile.TemporaryDirectory(prefix="goblins-release-iso-replay-") as temporary:
        replayed_iso = Path(temporary) / "goblins-os-aarch64.iso"
        actual_release_iso_size = decompress_zstd_inputs_bounded(
            zstd,
            release_iso_parts,
            replayed_iso,
            maximum=MAX_ZIP_MEMBER_BYTES,
            label="release ISO parts",
        )
        actual_release_iso_sha = sha256_path(
            replayed_iso,
            maximum=MAX_ZIP_MEMBER_BYTES,
        )
        if actual_release_iso_sha != expected_release_iso_sha:
            fail("decompressed ISO parts do not match the exact candidate ISO checksum")
        require_canonical_zstd_parts(
            zstd,
            replayed_iso,
            release_iso_parts,
            label="release ISO zstd bytes are not canonical",
        )

    display_proof = manifest.get("display_proof")
    if type(display_proof) is not dict or set(display_proof) != {
        "archive",
        "authority_ca_certificate_sha256",
        "authority_certificate_sha256",
        "authority_files",
        "authority_generation",
        "authority_record_sha256",
        "authority_schema",
        "authority_signature_sha256",
        "evidence_bundle_sha256",
        "evidence_schema",
        "member_count",
        "members",
        "run_date",
        "screenshot_count",
        "screenshot_manifest_sha256",
        "verification_iso_sha256",
    }:
        fail("promotion manifest has no exact display-proof member binding")
    archive_record = artifact_record(payload / "goblins-os-aarch64-display-proof.tar.zst")
    if display_proof.get("archive") != archive_record:
        fail("promotion manifest does not bind the exact display-proof archive")
    member_records = display_proof.get("members")
    if type(member_records) is not list or display_proof.get("member_count") != len(member_records):
        fail("promotion manifest has an invalid display-proof member count")
    normalized_members: list[dict[str, object]] = []
    member_names: set[str] = set()
    for item in member_records:
        if type(item) is not dict or set(item) != {"name", "sha256", "size_bytes"}:
            fail("promotion manifest has a malformed display-proof member record")
        name = item.get("name")
        digest = item.get("sha256")
        size = item.get("size_bytes")
        if (
            not isinstance(name, str)
            or PurePosixPath(name).name != name
            or name in ("", ".", "..")
            or name in member_names
            or re.search(r"(?:x86|amd64)", name, flags=re.IGNORECASE) is not None
            or not isinstance(digest, str)
            or SHA256_RE.fullmatch(digest) is None
            or type(size) is not int
            or size <= 0
            or size > MAX_ZIP_MEMBER_BYTES
        ):
            fail("promotion manifest has an unsafe display-proof member record")
        member_names.add(name)
        normalized_members.append(item)
    if normalized_members != sorted(normalized_members, key=lambda item: str(item["name"]).encode("utf-8")):
        fail("promotion manifest display-proof members are not canonically ordered")
    required_display_members = set(DISPLAY_FILES) | {"signoff-row.md"}
    if not required_display_members.issubset(member_names):
        fail("promotion manifest omits required canonical display-proof members")

    standalone_seal = load_json(payload / "evidence-bundle.json", label="release display seal")
    standalone_record = load_json(
        payload / "aarch64-local-display-attestation.json",
        label="release display authority record",
    )
    framebuffer = standalone_seal.get("framebuffer")
    screenshot_count = framebuffer.get("required_png_count") if type(framebuffer) is dict else None
    authority_certificate_sha256 = pinned_certificate_fingerprint(
        read_regular(
            payload / "display-proof-authority2.pem",
            maximum=64 * 1024,
            label="release Authority 2 leaf",
        ),
        read_regular(
            payload / "display-proof-authority2.sha256",
            maximum=256,
            label="release Authority 2 leaf fingerprint",
        ),
        label="release Authority 2 leaf",
    )
    authority_ca_certificate_sha256 = pinned_certificate_fingerprint(
        read_regular(
            payload / "display-proof-authority2-ca.pem",
            maximum=64 * 1024,
            label="release Authority 2 CA",
        ),
        read_regular(
            payload / "display-proof-authority2-ca.sha256",
            maximum=256,
            label="release Authority 2 CA fingerprint",
        ),
        label="release Authority 2 offline CA",
    )
    expected_display_binding = {
        "authority_ca_certificate_sha256": authority_ca_certificate_sha256,
        "authority_certificate_sha256": authority_certificate_sha256,
        "authority_generation": DISPLAY_AUTHORITY_GENERATION,
        "authority_record_sha256": sha256_path(
            payload / "aarch64-local-display-attestation.json"
        ),
        "authority_schema": DISPLAY_SCHEMA,
        "authority_signature_sha256": sha256_path(
            payload / "aarch64-local-display-attestation.json.cms"
        ),
        "evidence_bundle_sha256": sha256_path(payload / "evidence-bundle.json"),
        "evidence_schema": EVIDENCE_SCHEMA,
        "run_date": run_date,
        "screenshot_count": screenshot_count,
        "screenshot_manifest_sha256": standalone_seal.get("screenshot_manifest_sha256"),
        "verification_iso_sha256": actual_release_iso_sha,
    }
    if any(display_proof.get(key) != value for key, value in expected_display_binding.items()):
        fail("promotion manifest display-proof identity differs from the signed standalone proof")
    if (
        standalone_seal.get("schema") != EVIDENCE_SCHEMA
        or standalone_record.get("schema") != DISPLAY_SCHEMA
        or standalone_seal.get("architecture") != ARCHITECTURE
        or standalone_record.get("architecture") != ARCHITECTURE
        or standalone_record.get("authority_generation")
        != DISPLAY_AUTHORITY_GENERATION
        or standalone_record.get("signature_purpose") != DISPLAY_PURPOSE
        or standalone_record.get("authority_certificate_sha256")
        != authority_certificate_sha256
        or standalone_record.get("authority_ca_certificate_sha256")
        != authority_ca_certificate_sha256
        or standalone_seal.get("candidate_commit") != candidate_commit
        or standalone_record.get("candidate_commit") != candidate_commit
        or standalone_seal.get("image_ref") != image_ref
        or standalone_record.get("image_ref") != image_ref
        or standalone_seal.get("run_date") != run_date
        or standalone_record.get("run_date") != run_date
        or screenshot_count != DISPLAY_SCREENSHOT_COUNT
        or standalone_record.get("screenshot_count") != screenshot_count
        or standalone_record.get("screenshot_manifest_sha256")
        != standalone_seal.get("screenshot_manifest_sha256")
        or standalone_seal.get("verification_iso_sha256") != actual_release_iso_sha
        or standalone_record.get("verification_iso_sha256") != actual_release_iso_sha
    ):
        fail("standalone display proof does not bind the exact promotion identity")
    members_by_name = {str(item["name"]): item for item in normalized_members}
    authority_files = display_proof.get("authority_files")
    expected_authority_files = [
        artifact_record(payload / name) for name in AUTHORITY_PUBLIC_FILES
    ]
    if authority_files != expected_authority_files:
        fail("promotion manifest does not bind the exact Authority 2 public files")
    for name, expected_digest in (
        ("evidence-bundle.json", expected_display_binding["evidence_bundle_sha256"]),
        (
            "aarch64-local-display-attestation.json",
            expected_display_binding["authority_record_sha256"],
        ),
        (
            "aarch64-local-display-attestation.json.cms",
            expected_display_binding["authority_signature_sha256"],
        ),
        *(
            (name, artifact_record(payload / name)["sha256"])
            for name in AUTHORITY_PUBLIC_FILES
        ),
    ):
        if members_by_name[name].get("sha256") != expected_digest:
            fail(f"display-proof archive member binding differs from standalone {name}")
    sealed_entries = standalone_seal.get("entries")
    if type(sealed_entries) is not list:
        fail("standalone display seal has no member entries")
    sealed_names: set[str] = set()
    for entry in sealed_entries:
        if type(entry) is not dict or not isinstance(entry.get("path"), str):
            fail("standalone display seal has a malformed member entry")
        if entry["path"] in sealed_names:
            fail("standalone display seal has a duplicate member entry")
        sealed_names.add(entry["path"])
        member = members_by_name.get(entry["path"])
        if (
            member is None
            or member.get("sha256") != entry.get("sha256")
            or member.get("size_bytes") != entry.get("size")
        ):
            fail("promotion manifest display members differ from the signed evidence seal")
    if member_names != sealed_names | required_display_members:
        fail("promotion manifest display member set is not exactly the signed canonical run")

    candidate_metadata = load_json(
        payload / "candidate-image-ref-aarch64.json", label="candidate image metadata"
    )
    expected_candidate_metadata_keys = {
        "schema",
        "product",
        "architecture",
        "platform",
        "candidate_commit",
        "candidate_tag",
        "candidate_tag_authoritative",
        "image_digest",
        "immutable_image_ref",
        "oci_revision",
        "iso_sha256",
        "iso_manifest_sha256",
        "bib_manifest_sha256",
        "release_evidence_manifest_sha256",
        "cargo_packages_sha256",
        "rpm_command_sha256",
        "rpm_packages_sha256",
        "installer_config",
        "source_repository",
        "workflow_run",
        "workflow_run_attempt",
        "workflow_name",
        "exact_candidate_gates",
        "non_promotional",
    }
    if set(candidate_metadata) != expected_candidate_metadata_keys:
        fail("candidate metadata key set is not exact")
    candidate_expected = {
        "schema": "goblins-os-candidate-image-ref-v3",
        "product": "Goblins OS",
        "architecture": ARCHITECTURE,
        "platform": PLATFORM,
        "candidate_commit": candidate_commit,
        "candidate_tag": (
            f"{image_ref.split('@', 1)[0]}:candidate-{candidate_commit}-aarch64"
        ),
        "image_digest": image_ref.rsplit("@", 1)[1],
        "immutable_image_ref": image_ref,
        "oci_revision": candidate_commit,
        "iso_sha256": actual_release_iso_sha,
        "iso_manifest_sha256": sha256_path(
            payload / "manifest-goblins-os-aarch64.json"
        ),
        "bib_manifest_sha256": sha256_path(
            payload / "manifest-anaconda-iso-aarch64.json"
        ),
        "release_evidence_manifest_sha256": sha256_path(
            payload / "release-evidence-manifest-aarch64.json"
        ),
        "cargo_packages_sha256": sha256_path(payload / "cargo-lock-packages-aarch64.tsv"),
        "rpm_command_sha256": sha256_path(payload / "rpm-packages-aarch64.command"),
        "rpm_packages_sha256": sha256_path(payload / "rpm-packages-aarch64.tsv"),
        "installer_config": "os/iso/config.toml",
        "source_repository": SOURCE_REPOSITORY,
        "workflow_run": workflow_run_url(candidate_run_id),
        "workflow_run_attempt": int(candidate_run_attempt),
        "workflow_name": "candidate-artifacts",
        "candidate_tag_authoritative": False,
        "non_promotional": True,
    }
    if any(candidate_metadata.get(key) != value for key, value in candidate_expected.items()):
        fail("candidate metadata changes the exact promotion identity")
    if candidate_metadata.get("exact_candidate_gates") != {
        "source_verifier": "pass",
        "installed_root_verifier": "pass",
        "services_selftest": "pass",
    }:
        fail("candidate metadata does not record every exact candidate gate")
    release_evidence = load_json(
        payload / "release-evidence-manifest-aarch64.json", label="release evidence manifest"
    )
    evidence_expected = {
        "schema": "goblins-os-release-evidence-v5",
        "architecture": ARCHITECTURE,
        "candidate_commit": candidate_commit,
        "image_ref": image_ref,
        "image_digest_pinned": True,
        "cargo_packages_sha256": sha256_path(payload / "cargo-lock-packages-aarch64.tsv"),
        "rpm_command_sha256": sha256_path(payload / "rpm-packages-aarch64.command"),
        "rpm_packages_sha256": sha256_path(payload / "rpm-packages-aarch64.tsv"),
    }
    if any(release_evidence.get(key) != value for key, value in evidence_expected.items()):
        fail("release evidence does not bind every exact SBOM byte")
    iso_manifest = load_json(
        payload / "manifest-goblins-os-aarch64.json", label="public ISO manifest"
    )
    iso_expected = {
        "architecture": ARCHITECTURE,
        "candidate_commit": candidate_commit,
        "image": image_ref,
        "builder_source_image": image_ref,
        "installer_payload_source_kind": "release-registry",
        "installer_payload_source_local_only": False,
        "shippable_release": True,
    }
    if any(iso_manifest.get(key) != value for key, value in iso_expected.items()):
        fail("public ISO manifest is not the exact shippable aarch64 candidate")
    require_exact_public_metadata(
        manifest,
        stable_tag=stable_tag,
        candidate_commit=candidate_commit,
        promotion_run_id=promotion_run_id,
        image_ref=image_ref,
        promotion_timestamp=promotion_timestamp,
        raw_iso_sha256=actual_release_iso_sha,
        raw_iso_size=actual_release_iso_size,
        compressed_iso_sha256=actual_compressed_iso_sha,
        compressed_iso_size=actual_compressed_iso_size,
        part_records=[observed[name] for name in part_names],
        built_on=iso_manifest.get("built_on"),
    )

    proof_digest = parse_single_checksum(
        payload / "goblins-os-aarch64-display-proof.tar.zst.sha256",
        "goblins-os-aarch64-display-proof.tar.zst",
    )
    if proof_digest != sha256_path(payload / "goblins-os-aarch64-display-proof.tar.zst"):
        fail("display proof archive checksum does not match")
    replayed_members = replay_and_verify_display_proof(
        payload,
        zstd=zstd,
        repository=repository,
        source_epoch=source_date_epoch,
        candidate_commit=candidate_commit,
        image_ref=image_ref,
        run_date=run_date,
        release_iso_sha256=actual_release_iso_sha,
    )
    require_exact_display_member_records(replayed_members, normalized_members)
    return manifest


def verify_payload(args: argparse.Namespace) -> int:
    require_exact_zstd(args.zstd)
    verify_payload_directory(
        Path(args.payload).resolve(strict=True),
        candidate_commit=args.candidate_commit,
        stable_tag=args.stable_tag,
        candidate_run_id=args.candidate_run_id,
        candidate_run_attempt=args.candidate_run_attempt,
        display_run_id=args.display_run_id,
        display_run_attempt=args.display_run_attempt,
        promotion_run_id=args.promotion_run_id,
        promotion_run_attempt=args.promotion_run_attempt,
        run_date=args.run_date,
        image_ref=args.image_ref,
        zstd=args.zstd,
        repository=Path(args.repository).resolve(strict=True),
    )
    return 0


def extract_payload(args: argparse.Namespace) -> int:
    artifact_response = load_json(Path(args.artifacts_json), label="promotion payload artifact response")
    pattern = re.compile(re.escape(args.artifact_name))
    artifact = select_artifact(
        artifact_response,
        name_pattern=pattern,
        run_id=args.run_id,
        commit=args.candidate_commit,
        label="stable promotion payload",
    )
    with snapshot_archive(
        Path(args.archive), label="stable promotion payload artifact"
    ) as archive:
        validate_artifact_digest(archive, artifact, "stable promotion payload artifact")
        with open_snapshot_zip(archive) as handle:
            file_names = {
                str(PurePosixPath(item.filename))
                for item in handle.infolist()
                if not item.is_dir()
            }
        extract_zip(archive, Path(args.destination), file_names)
    return 0


def self_test(args: argparse.Namespace) -> int:
    require_exact_zstd(args.zstd)
    with tempfile.TemporaryDirectory(prefix="goblins-stable-promotion-self-test-") as temporary:
        root = Path(temporary)
        run = root / "aarch64/2099-01-02"
        run.mkdir(parents=True)
        screenshot_data = b"synthetic-png-bytes-for-seal-replay\n"
        screenshot_names = list(DISPLAY_SCREENSHOT_NAMES)
        candidate_commit = "1" * 40
        image_ref = f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}"
        run_date = "2099-01-02"
        verification_iso_sha256 = "3" * 64
        stable_tag = "v1.2.3"
        candidate_run_id = "101"
        candidate_run_attempt = "2"
        display_run_id = "202"
        display_run_attempt = "3"
        promotion_run_id = "303"
        promotion_run_attempt = "4"
        identity_manifest: dict[str, object] = {
            "candidate_commit": candidate_commit,
            "stable_tag": stable_tag,
            "container": {"immutable_image_ref": image_ref},
            "promotion_workflow_run": workflow_run_url(promotion_run_id),
            "promotion_workflow_run_attempt": int(promotion_run_attempt),
            "candidate_workflow": {
                "run": workflow_run_url(candidate_run_id),
                "run_attempt": int(candidate_run_attempt),
                "artifact_name": f"goblins-os-candidate-{candidate_commit}-aarch64",
                "artifact_digest": f"sha256:{'4' * 64}",
            },
            "display_verification_workflow": {
                "run": workflow_run_url(display_run_id),
                "run_attempt": int(display_run_attempt),
                "artifact_name": (
                    f"aarch64-local-display-verified-{candidate_commit}-{run_date}-"
                    f"attempt-{display_run_attempt}"
                ),
                "artifact_digest": f"sha256:{'5' * 64}",
                "run_date": run_date,
            },
        }
        expected_identity = {
            "candidate_commit": candidate_commit,
            "stable_tag": stable_tag,
            "candidate_run_id": candidate_run_id,
            "candidate_run_attempt": candidate_run_attempt,
            "display_run_id": display_run_id,
            "display_run_attempt": display_run_attempt,
            "promotion_run_id": promotion_run_id,
            "promotion_run_attempt": promotion_run_attempt,
            "run_date": run_date,
            "image_ref": image_ref,
        }
        require_expected_promotion_identity(identity_manifest, **expected_identity)

        identity_mutations: tuple[tuple[object, ...], ...] = (
            ("candidate_workflow", "run", workflow_run_url("999")),
            ("candidate_workflow", "run_attempt", 999),
            ("display_verification_workflow", "run", workflow_run_url("999")),
            ("display_verification_workflow", "run_attempt", 999),
            ("display_verification_workflow", "run_date", "2099-01-03"),
            ("promotion_workflow_run", workflow_run_url("999")),
            ("promotion_workflow_run_attempt", 999),
            (
                "container",
                "immutable_image_ref",
                f"ghcr.io/joe-simo/goblins-os@sha256:{'9' * 64}",
            ),
        )
        for mutation in identity_mutations:
            mutated_identity = json.loads(json.dumps(identity_manifest))
            if len(mutation) == 3:
                parent = mutated_identity[mutation[0]]
                assert type(parent) is dict
                parent[mutation[1]] = mutation[2]
            else:
                mutated_identity[mutation[0]] = mutation[1]
            try:
                require_expected_promotion_identity(mutated_identity, **expected_identity)
            except PromotionError:
                pass
            else:
                fail(f"self-test accepted mutated promotion identity field: {mutation[0]}")

        public_part_records = [
            {
                "name": "goblins-os-aarch64.iso.zst.part-00",
                "sha256": "6" * 64,
                "size_bytes": 512,
            }
        ]
        public_metadata_arguments = {
            "stable_tag": stable_tag,
            "candidate_commit": candidate_commit,
            "promotion_run_id": promotion_run_id,
            "image_ref": image_ref,
            "promotion_timestamp": "2099-01-02T12:34:56Z",
            "raw_iso_sha256": verification_iso_sha256,
            "raw_iso_size": 4096,
            "compressed_iso_sha256": "7" * 64,
            "compressed_iso_size": 512,
            "part_records": public_part_records,
            "built_on": "2099-01-02T12:00:00Z",
        }
        expected_container, expected_website = expected_public_metadata(
            **public_metadata_arguments
        )
        public_manifest = {
            "container": expected_container,
            "website": expected_website,
        }
        require_exact_public_metadata(public_manifest, **public_metadata_arguments)

        public_mutations: tuple[tuple[object, ...], ...] = (
            ("container", "unexpected", True),
            ("container", "immutable_image_ref", f"ghcr.io/joe-simo/goblins-os@sha256:{'8' * 64}"),
            ("website", "preparedAt", "2099-01-02T12:34:57Z"),
            ("website", "releaseArtifact", "downloadParts", 0, "url", "https://example.invalid/part"),
            ("website", "releaseArtifact", "downloadParts", 0, "filename", "wrong.part"),
            ("website", "releaseArtifact", "downloadParts", 0, "sha256", "8" * 64),
            ("website", "releaseArtifact", "downloadParts", 0, "sizeBytes", 513),
            ("website", "releaseArtifact", "status", "pending"),
            ("website", "containerImage", "immutableImage", f"ghcr.io/joe-simo/goblins-os@sha256:{'8' * 64}"),
        )
        for mutation in public_mutations:
            mutated_public = json.loads(json.dumps(public_manifest))
            cursor: object = mutated_public
            for key in mutation[:-2]:
                if isinstance(key, int):
                    assert type(cursor) is list
                    cursor = cursor[key]
                else:
                    assert type(cursor) is dict
                    cursor = cursor[key]
            final_key = mutation[-2]
            assert type(cursor) in (dict, list)
            cursor[final_key] = mutation[-1]
            try:
                require_exact_public_metadata(mutated_public, **public_metadata_arguments)
            except PromotionError:
                pass
            else:
                fail(f"self-test accepted mutated public metadata field: {final_key}")

        legacy_publication_claim = json.loads(json.dumps(public_manifest))
        legacy_website = legacy_publication_claim["website"]
        assert type(legacy_website) is dict
        legacy_website["publishedAt"] = legacy_website.pop("preparedAt")
        try:
            require_exact_public_metadata(
                legacy_publication_claim, **public_metadata_arguments
            )
        except PromotionError:
            pass
        else:
            fail("self-test accepted prepublication payload metadata as publishedAt")

        proof_manifest = {
            "architecture": ARCHITECTURE,
            "candidate_commit": candidate_commit,
            "captured_at": f"{run_date}T00:00:00Z",
            "image_ref": image_ref,
            "iso": "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
            "iso_sha256": verification_iso_sha256,
            "screenshot_run_dir": f"os/screenshots/hardware-gate/aarch64/{run_date}",
        }
        proof_manifest_data = canonical_json(proof_manifest)
        proof_manifest_entry = {
            "kind": "json",
            "path": "proof-manifest.json",
            "sha256": hashlib.sha256(proof_manifest_data).hexdigest(),
            "size": len(proof_manifest_data),
        }
        screenshot_entries = [
            {
                "height": 720,
                "kind": "png",
                "path": name,
                "sha256": hashlib.sha256(screenshot_data).hexdigest(),
                "size": len(screenshot_data),
                "width": 1024,
            }
            for name in screenshot_names
        ]
        seal = {
            "architecture": ARCHITECTURE,
            "candidate_commit": candidate_commit,
            "entries": [proof_manifest_entry, *screenshot_entries],
            "entry_count": DISPLAY_SCREENSHOT_COUNT + 1,
            "framebuffer": {
                "height": 720,
                "required_png_count": DISPLAY_SCREENSHOT_COUNT,
                "width": 1024,
            },
            "image_ref": image_ref,
            "run_date": run_date,
            "schema": EVIDENCE_SCHEMA,
            "screenshot_manifest_sha256": hashlib.sha256(
                canonical_json(screenshot_entries)
            ).hexdigest(),
            "verification_iso_sha256": verification_iso_sha256,
        }
        seal_data = canonical_json(seal)

        ca_key = root / "authority2-ca-key.pem"
        ca_certificate = run / "display-proof-authority2-ca.pem"
        leaf_key = root / "authority2-leaf-key.pem"
        leaf_csr = root / "authority2-leaf.csr"
        leaf_extensions = root / "authority2-leaf.cnf"
        leaf_certificate = run / "display-proof-authority2.pem"
        leaf_extensions.write_text(
            "[leaf]\n"
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature\n"
            "extendedKeyUsage=emailProtection\n"
            "subjectKeyIdentifier=hash\n"
            "authorityKeyIdentifier=keyid,issuer\n",
            encoding="ascii",
        )

        def self_test_openssl(arguments: list[str]) -> None:
            subprocess.run(
                ["openssl", *arguments],
                check=True,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=30,
            )

        self_test_openssl(
            [
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(ca_key),
                "-out",
                str(ca_certificate),
                "-days",
                "60",
                "-sha256",
                "-subj",
                "/CN=Goblins OS Display Proof Offline CA Self Test/",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ]
        )
        self_test_openssl(
            [
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(leaf_key),
                "-out",
                str(leaf_csr),
                "-subj",
                "/CN=Goblins OS Display Proof Authority 2/",
            ]
        )
        self_test_openssl(
            [
                "x509",
                "-req",
                "-in",
                str(leaf_csr),
                "-CA",
                str(ca_certificate),
                "-CAkey",
                str(ca_key),
                "-set_serial",
                "1",
                "-days",
                "60",
                "-sha256",
                "-extfile",
                str(leaf_extensions),
                "-extensions",
                "leaf",
                "-out",
                str(leaf_certificate),
            ]
        )
        leaf_certificate_data = read_regular(
            leaf_certificate, maximum=64 * 1024, label="self-test Authority 2 leaf"
        )
        ca_certificate_data = read_regular(
            ca_certificate, maximum=64 * 1024, label="self-test Authority 2 CA"
        )
        leaf_der = ssl.PEM_cert_to_DER_cert(leaf_certificate_data.decode("ascii"))
        ca_der = ssl.PEM_cert_to_DER_cert(ca_certificate_data.decode("ascii"))
        leaf_fingerprint = hashlib.sha256(leaf_der).hexdigest()
        ca_fingerprint = hashlib.sha256(ca_der).hexdigest()
        write_exclusive(
            run / "display-proof-authority2.sha256",
            (leaf_fingerprint + "\n").encode("ascii"),
        )
        write_exclusive(
            run / "display-proof-authority2-ca.sha256",
            (ca_fingerprint + "\n").encode("ascii"),
        )
        record = {
            "architecture": ARCHITECTURE,
            "authority_ca_certificate_sha256": ca_fingerprint,
            "authority_certificate_sha256": leaf_fingerprint,
            "authority_generation": DISPLAY_AUTHORITY_GENERATION,
            "candidate_commit": seal["candidate_commit"],
            "evidence_bundle_sha256": hashlib.sha256(seal_data).hexdigest(),
            "evidence_bundle_size": len(seal_data),
            "image_ref": seal["image_ref"],
            "run_date": seal["run_date"],
            "schema": DISPLAY_SCHEMA,
            "screenshot_count": DISPLAY_SCREENSHOT_COUNT,
            "screenshot_manifest_sha256": seal["screenshot_manifest_sha256"],
            "signature_purpose": DISPLAY_PURPOSE,
            "verification_iso_sha256": verification_iso_sha256,
        }
        for name in screenshot_names:
            write_exclusive(run / name, screenshot_data)
        write_exclusive(run / "proof-manifest.json", proof_manifest_data)
        write_exclusive(run / "evidence-bundle.json", seal_data)
        write_exclusive(
            run / "aarch64-local-display-attestation.json", canonical_json(record)
        )
        write_exclusive(run / "aarch64-local-display-attestation.json.cms", b"synthetic-cms\n")
        write_exclusive(run / "signoff-row.md", b"synthetic signed row\n")

        require_signed_display_iso_binding(
            run,
            seal,
            record,
            release_iso_sha256=verification_iso_sha256,
            candidate_commit=candidate_commit,
            image_ref=image_ref,
            run_date=run_date,
        )
        try:
            require_signed_display_iso_binding(
                run,
                seal,
                record,
                release_iso_sha256="4" * 64,
                candidate_commit=candidate_commit,
                image_ref=image_ref,
                run_date=run_date,
            )
        except PromotionError:
            pass
        else:
            fail("self-test paired a signed display proof for ISO A with release ISO B")

        tar_path = root / "proof.tar"
        archive = root / "proof.tar.zst"
        create_display_tar(run, tar_path, 1_700_000_000)
        compress_deterministically(args.zstd, tar_path, archive)
        replay = extract_display_proof_archive(
            args.zstd,
            archive,
            root / "valid-replay",
            source_epoch=1_700_000_000,
        )
        validate_members_against_signed_seal(replay, standalone_root=run)
        replay_records = [artifact_record(path) for path in scan_flat_display_source(replay)]
        declared_records = [dict(item) for item in replay_records]
        require_exact_display_member_records(replay_records, declared_records)

        hidden_ustar = root / "hidden-padding.tar"
        hidden_ustar_data = bytearray(tar_path.read_bytes())
        if not hidden_ustar_data or hidden_ustar_data[-1] != 0:
            fail("self-test canonical USTAR fixture has no terminal zero padding")
        hidden_ustar_data[-1] = 1
        hidden_ustar.write_bytes(hidden_ustar_data)
        hidden_ustar_archive = root / "hidden-padding.tar.zst"
        compress_deterministically(args.zstd, hidden_ustar, hidden_ustar_archive)
        try:
            extract_display_proof_archive(
                args.zstd,
                hidden_ustar_archive,
                root / "hidden-padding-replay",
                source_epoch=1_700_000_000,
            )
        except PromotionError as error:
            if str(error) != "display-proof raw USTAR bytes are not canonical":
                raise
        else:
            fail("self-test accepted hidden bytes in raw USTAR padding")

        skippable_payload = b"forbidden hidden release metadata"
        skippable_frame = (
            b"\x50\x2a\x4d\x18"
            + len(skippable_payload).to_bytes(4, "little")
            + skippable_payload
        )
        hidden_zstd_archive = root / "hidden-frame.tar.zst"
        hidden_zstd_archive.write_bytes(archive.read_bytes() + skippable_frame)
        try:
            extract_display_proof_archive(
                args.zstd,
                hidden_zstd_archive,
                root / "hidden-frame-replay",
                source_epoch=1_700_000_000,
            )
        except PromotionError as error:
            if str(error) != "display-proof zstd bytes are not canonical":
                raise
        else:
            fail("self-test accepted a skippable frame in display-proof zstd bytes")

        declared_records[0]["sha256"] = "0" * 64
        try:
            require_exact_display_member_records(replay_records, declared_records)
        except PromotionError:
            pass
        else:
            fail("self-test accepted a mutated promotion-manifest display member binding")

        (run / screenshot_names[0]).write_bytes(b"mutated-after-seal\n")
        mutated_tar = root / "mutated.tar"
        mutated_archive = root / "mutated.tar.zst"
        create_display_tar(run, mutated_tar, 1_700_000_000)
        compress_deterministically(args.zstd, mutated_tar, mutated_archive)
        mutated_replay = extract_display_proof_archive(
            args.zstd,
            mutated_archive,
            root / "mutated-replay",
            source_epoch=1_700_000_000,
        )
        try:
            validate_members_against_signed_seal(mutated_replay)
        except PromotionError:
            pass
        else:
            fail("self-test accepted a post-seal display member mutation")

        traversal = root / "traversal.zip"
        with zipfile.ZipFile(traversal, "w") as zipped:
            zipped.writestr("../escape", b"escape")
        try:
            with snapshot_archive(traversal, label="traversal self-test archive") as snapshot:
                safe_zip_members(snapshot, {"../escape"})
        except PromotionError:
            pass
        else:
            fail("self-test accepted ZIP parent traversal")

        linked = root / "linked.zip"
        with zipfile.ZipFile(linked, "w") as zipped:
            info = zipfile.ZipInfo("linked")
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            zipped.writestr(info, b"target")
        try:
            with snapshot_archive(linked, label="symlink self-test archive") as snapshot:
                safe_zip_members(snapshot, {"linked"})
        except PromotionError:
            pass
        else:
            fail("self-test accepted a symlink ZIP member")

        valid_zip = root / "valid.zip"
        with zipfile.ZipFile(valid_zip, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.writestr("nested/payload.txt", b"stable snapshot payload\n")
        with snapshot_archive(valid_zip, label="valid self-test archive") as snapshot:
            extracted = root / "valid-extracted"
            extract_zip(snapshot, extracted, {"nested/payload.txt"})
        if (extracted / "nested/payload.txt").read_bytes() != b"stable snapshot payload\n":
            fail("self-test archive snapshot extraction changed payload bytes")

        bounded_member_zip = root / "bounded-member.zip"
        with zipfile.ZipFile(
            bounded_member_zip,
            "w",
            compression=zipfile.ZIP_STORED,
        ) as zipped:
            zipped.writestr("payload.json", b'{"value":true}\n')
        try:
            with snapshot_archive(
                bounded_member_zip,
                label="purpose-bounded ZIP self-test archive",
            ) as snapshot:
                read_zip_member(
                    snapshot,
                    "payload.json",
                    {"payload.json"},
                    maximum=4,
                    label="purpose-bounded ZIP self-test member",
                )
        except PromotionError:
            pass
        else:
            fail("self-test allocated a ZIP member beyond its purpose-specific limit")

        zstd_source_data = (b"bounded-zstd-output\n" * 256) + b"end\n"
        zstd_source = root / "bounded-zstd-source.bin"
        zstd_source.write_bytes(zstd_source_data)
        zstd_archive = root / "bounded-zstd-source.bin.zst"
        subprocess.run(
            [args.zstd, "--quiet", "--force", "-o", str(zstd_archive), str(zstd_source)],
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        observed_zstd_sha, observed_zstd_size = stream_decompressed_record(
            args.zstd,
            [zstd_archive],
            maximum=len(zstd_source_data),
            label="bounded zstd self-test stream",
        )
        if (
            observed_zstd_sha != hashlib.sha256(zstd_source_data).hexdigest()
            or observed_zstd_size != len(zstd_source_data)
        ):
            fail("self-test bounded zstd stream changed decompressed bytes")
        try:
            stream_decompressed_record(
                args.zstd,
                [zstd_archive],
                maximum=len(zstd_source_data) - 1,
                label="overflowing zstd self-test stream",
            )
        except PromotionError:
            pass
        else:
            fail("self-test accepted zstd output beyond its streaming limit")

        bounded_zstd_output = root / "bounded-zstd-output.bin"
        bounded_zstd_size = decompress_zstd_file_bounded(
            args.zstd,
            zstd_archive,
            bounded_zstd_output,
            maximum=len(zstd_source_data),
            label="bounded zstd self-test file",
        )
        if (
            bounded_zstd_size != len(zstd_source_data)
            or bounded_zstd_output.read_bytes() != zstd_source_data
        ):
            fail("self-test bounded zstd file changed decompressed bytes")
        overflowing_zstd_output = root / "overflowing-zstd-output.bin"
        try:
            decompress_zstd_file_bounded(
                args.zstd,
                zstd_archive,
                overflowing_zstd_output,
                maximum=len(zstd_source_data) - 1,
                label="overflowing zstd self-test file",
            )
        except PromotionError:
            pass
        else:
            fail("self-test accepted zstd file output beyond its limit")
        if overflowing_zstd_output.exists():
            fail("self-test left a partial file after bounded zstd rejection")

        canonical_iso_zstd = root / "canonical-iso.zst"
        compress_once(args.zstd, zstd_source, canonical_iso_zstd)
        require_canonical_zstd_parts(
            args.zstd,
            zstd_source,
            [canonical_iso_zstd],
            label="release ISO zstd bytes are not canonical",
        )
        hidden_iso_zstd = root / "hidden-iso-frame.zst"
        hidden_iso_payload = b"forbidden hidden ISO metadata"
        hidden_iso_frame = (
            b"\x50\x2a\x4d\x18"
            + len(hidden_iso_payload).to_bytes(4, "little")
            + hidden_iso_payload
        )
        hidden_iso_zstd.write_bytes(canonical_iso_zstd.read_bytes() + hidden_iso_frame)
        try:
            require_canonical_zstd_parts(
                args.zstd,
                zstd_source,
                [hidden_iso_zstd],
                label="release ISO zstd bytes are not canonical",
            )
        except PromotionError as error:
            if str(error) != "release ISO zstd bytes are not canonical":
                raise
        else:
            fail("self-test accepted a skippable frame in release ISO zstd bytes")

        zstd_replace_target = root / "replace-target.zst"
        zstd_replacement = root / "replacement.zst"
        shutil.copyfile(zstd_archive, zstd_replace_target)
        replacement_source = root / "replacement-zstd-source.bin"
        replacement_source.write_bytes(b"replacement zstd bytes\n")
        subprocess.run(
            [
                args.zstd,
                "--quiet",
                "--force",
                "-o",
                str(zstd_replacement),
                str(replacement_source),
            ],
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        try:
            with pin_regular_input(
                zstd_replace_target,
                maximum=MAX_COMPRESSED_ISO_BYTES,
                label="replaced zstd self-test input",
            ) as pinned:
                os.replace(zstd_replacement, zstd_replace_target)
                pump_zstd_decompression(
                    args.zstd,
                    [pinned],
                    maximum=len(zstd_source_data),
                    label="replaced zstd self-test stream",
                )
        except PromotionError:
            pass
        else:
            fail("self-test accepted a zstd input pathname replaced after pinning")

        replace_target = root / "replace-target.zip"
        replacement = root / "replacement.zip"
        with zipfile.ZipFile(replace_target, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.writestr("payload.txt", b"pinned original bytes\n")
        with zipfile.ZipFile(replacement, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.writestr("payload.txt", b"replacement attacker bytes\n")
        read_pinned_original = False
        try:
            with snapshot_archive(
                replace_target,
                label="replaced self-test archive",
            ) as snapshot:
                with open_snapshot_zip(snapshot) as handle:
                    os.replace(replacement, replace_target)
                    read_pinned_original = (
                        handle.read("payload.txt") == b"pinned original bytes\n"
                    )
        except PromotionError:
            pass
        else:
            fail("self-test accepted an archive pathname replaced after snapshotting")
        if not read_pinned_original:
            fail("self-test ZIP consumer reopened a replaced archive pathname")

        archive_payload = root / "archive-payload.bin"
        with archive_payload.open("wb") as handle:
            handle.truncate(64 * 1024 * 1024)
        changing_archive = root / "changing-archive.zip"
        with zipfile.ZipFile(changing_archive, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.write(archive_payload, "payload.bin")
        archive_stop = threading.Event()
        archive_started = threading.Event()

        def mutate_archive_while_snapshotted() -> None:
            descriptor = os.open(changing_archive, os.O_WRONLY)
            try:
                counter = 0
                archive_started.set()
                while not archive_stop.is_set():
                    os.pwrite(descriptor, bytes((counter % 251,)), 128)
                    os.utime(changing_archive, None)
                    counter += 1
            finally:
                os.close(descriptor)

        archive_mutator = threading.Thread(
            target=mutate_archive_while_snapshotted,
            daemon=True,
        )
        archive_mutator.start()
        archive_started.wait(timeout=5)
        try:
            with snapshot_archive(
                changing_archive,
                label="concurrently changing self-test archive",
            ):
                pass
        except PromotionError:
            pass
        else:
            fail("self-test accepted an archive mutated during snapshotting")
        finally:
            archive_stop.set()
            archive_mutator.join(timeout=5)
        if archive_mutator.is_alive():
            fail("self-test archive mutation thread did not stop")

        changing = root / "changing.bin"
        with changing.open("wb") as handle:
            handle.truncate(64 * 1024 * 1024)
        stop = threading.Event()
        started = threading.Event()

        def mutate_while_hashed() -> None:
            descriptor = os.open(changing, os.O_WRONLY)
            try:
                counter = 0
                started.set()
                while not stop.is_set():
                    os.pwrite(descriptor, bytes((counter % 251,)), 0)
                    os.utime(changing, None)
                    counter += 1
            finally:
                os.close(descriptor)

        mutator = threading.Thread(target=mutate_while_hashed, daemon=True)
        mutator.start()
        started.wait(timeout=5)
        try:
            sha256_path(changing)
        except PromotionError:
            pass
        else:
            fail("self-test did not reject a file mutated while it was hashed")
        finally:
            stop.set()
            mutator.join(timeout=5)
        if mutator.is_alive():
            fail("self-test mutation thread did not stop")

    print("stable-promotion-self-test: pass")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    validate = subparsers.add_parser("validate-inputs")
    for option in (
        "candidate-commit",
        "candidate-run-id",
        "candidate-run-attempt",
        "display-run-id",
        "display-run-attempt",
        "stable-tag",
        "confirmation",
    ):
        validate.add_argument(f"--{option}", required=True)
    validate.set_defaults(
        handler=lambda args: (
            validate_inputs(
                args.candidate_commit,
                args.candidate_run_id,
                args.candidate_run_attempt,
                args.display_run_id,
                args.display_run_attempt,
                args.stable_tag,
                args.confirmation,
            )
            or 0
        )
    )

    remote = subparsers.add_parser("validate-remote")
    for option in (
        "candidate-commit",
        "candidate-run-id",
        "candidate-run-attempt",
        "display-run-id",
        "display-run-attempt",
        "stable-tag",
        "confirmation",
        "candidate-run-json",
        "candidate-artifacts-json",
        "candidate-archive",
        "display-run-json",
        "display-artifacts-json",
        "display-archive",
        "output",
    ):
        remote.add_argument(f"--{option}", required=True)
    remote.set_defaults(handler=validate_remote)

    extract_candidate_parser = subparsers.add_parser("extract-candidate")
    extract_candidate_parser.add_argument("--archive", required=True)
    extract_candidate_parser.add_argument("--expected-digest", required=True)
    extract_candidate_parser.add_argument("--destination", required=True)
    extract_candidate_parser.set_defaults(handler=extract_candidate)

    extract_display_parser = subparsers.add_parser("extract-display")
    extract_display_parser.add_argument("--archive", required=True)
    extract_display_parser.add_argument("--expected-digest", required=True)
    extract_display_parser.add_argument("--destination", required=True)
    extract_display_parser.set_defaults(handler=extract_display)

    install = subparsers.add_parser("install-display")
    install.add_argument("--source", required=True)
    install.add_argument("--verified-display", required=True)
    install.add_argument("--repository", required=True)
    install.add_argument("--remote-inputs", required=True)
    install.set_defaults(handler=install_display)

    build = subparsers.add_parser("build-assets")
    for option in (
        "repository",
        "display-run",
        "output",
        "remote-inputs",
        "candidate-commit",
        "stable-tag",
        "promotion-timestamp",
        "promotion-workflow-run",
        "promotion-workflow-run-attempt",
        "source-date-epoch",
        "final-gate-log",
        "zstd",
    ):
        build.add_argument(f"--{option}", required=True)
    build.set_defaults(handler=build_assets)

    verify = subparsers.add_parser("verify-payload")
    for option in (
        "payload",
        "candidate-commit",
        "stable-tag",
        "candidate-run-id",
        "candidate-run-attempt",
        "display-run-id",
        "display-run-attempt",
        "promotion-run-id",
        "promotion-run-attempt",
        "run-date",
        "image-ref",
        "zstd",
        "repository",
    ):
        verify.add_argument(f"--{option}", required=True)
    verify.set_defaults(handler=verify_payload)

    payload = subparsers.add_parser("extract-payload")
    for option in (
        "artifacts-json",
        "artifact-name",
        "run-id",
        "candidate-commit",
        "archive",
        "destination",
    ):
        payload.add_argument(f"--{option}", required=True)
    payload.set_defaults(handler=extract_payload)

    test = subparsers.add_parser("self-test")
    test.add_argument("--zstd", required=True)
    test.set_defaults(handler=self_test)

    args = parser.parse_args()
    try:
        return int(args.handler(args))
    except (PromotionError, OSError, tarfile.TarError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
