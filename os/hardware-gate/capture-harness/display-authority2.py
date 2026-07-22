#!/usr/bin/env python3
"""Provision, audit, and use the interactive Authority 2 signing boundary.

The helper never accepts a Keychain password or PKCS#12 passphrase. A dedicated
Keychain starts locked, SecurityAgent performs every unlock, the reviewed Swift
Security.framework policy helper audits the key before and after CMS signing,
and only a signature from an unchanged policy is published after relocking.
"""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import hashlib
import importlib.util
import io
import json
import os
import platform
import re
import shlex
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Final, Iterator, TextIO


SCRIPT_DIR: Final = Path(__file__).resolve().parent
SWIFT_POLICY_SOURCE: Final = SCRIPT_DIR / "display-authority2-keychain.swift"
EVIDENCE_BUNDLE_SOURCE: Final = SCRIPT_DIR / "evidence_bundle.py"
AUTHORITY_IDENTITY: Final = "Goblins OS Display Proof Authority 2"
POLICY_SCHEMA: Final = "goblins-os-display-authority2-keychain-policy-v1"
DEFAULT_KEYCHAIN: Final = (
    Path.home()
    / "Library"
    / "Keychains"
    / "GoblinsOS-Display-Authority-2.keychain-db"
)
MAX_COMMAND_OUTPUT: Final = 4 * 1024 * 1024
MAX_SIGNATURE_BYTES: Final = 256 * 1024
MAX_LOCK_INTERVAL_SECONDS: Final = 300
PROMPT_REQUIRE_PASSPHRASE: Final = 0x0001
COMMIT_RE: Final = re.compile(r"[0-9a-f]{40}\Z")
SHA256_RE: Final = re.compile(r"[0-9a-f]{64}\Z")
IMAGE_RE: Final = re.compile(r"[^\s@]+@sha256:([0-9a-f]{64})\Z")
DATE_RE: Final = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}\Z")
SENSITIVE_ENV_NAMES: Final = {
    "KEYCHAIN_PASSWORD",
    "KEYCHAIN_PASSPHRASE",
    "P12_PASSWORD",
    "PKCS12_PASSWORD",
}


class AuthorityError(RuntimeError):
    """A fail-closed Authority 2 policy or signing error."""


def load_evidence_bundle() -> Any:
    spec = importlib.util.spec_from_file_location(
        "goblins_authority2_evidence_bundle", EVIDENCE_BUNDLE_SOURCE
    )
    if spec is None or spec.loader is None:
        raise AuthorityError("could not load the candidate evidence-bundle verifier")
    module = importlib.util.module_from_spec(spec)
    inserted = str(SCRIPT_DIR) not in sys.path
    if inserted:
        sys.path.insert(0, str(SCRIPT_DIR))
    try:
        spec.loader.exec_module(module)
    finally:
        if inserted:
            sys.path.remove(str(SCRIPT_DIR))
    return module


def require_supported_host() -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise AuthorityError(
            "Authority 2 operations require the approved Apple-Silicon Darwin/arm64 host"
        )


def reject_password_environment() -> None:
    forbidden = []
    for name in os.environ:
        upper = name.upper()
        if upper in SENSITIVE_ENV_NAMES or (
            upper.startswith("GOBLINS_OS_")
            and ("PASSWORD" in upper or "PASSPHRASE" in upper)
        ):
            forbidden.append(name)
    if forbidden:
        names = ", ".join(sorted(forbidden))
        raise AuthorityError(
            f"remove password-bearing environment variable names before continuing: {names}"
        )


def command_text(data: bytes) -> str:
    return data.decode("utf-8", errors="replace").strip()


def run_checked(
    arguments: list[str],
    *,
    input_bytes: bytes | None = None,
    timeout: int = 60,
) -> subprocess.CompletedProcess[bytes]:
    if not arguments or any("\x00" in argument for argument in arguments):
        raise AuthorityError("refused an invalid subprocess argument")
    try:
        result = subprocess.run(
            arguments,
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AuthorityError(f"command could not complete: {arguments[0]}") from error
    if len(result.stdout) > MAX_COMMAND_OUTPUT or len(result.stderr) > MAX_COMMAND_OUTPUT:
        raise AuthorityError(f"command produced excessive output: {arguments[0]}")
    if result.returncode != 0:
        detail = command_text(result.stderr) or command_text(result.stdout)
        if len(detail) > 2000:
            detail = detail[:2000] + "..."
        raise AuthorityError(
            f"command failed ({result.returncode}): {arguments[0]}"
            + (f": {detail}" if detail else "")
        )
    return result


def stable_file_bytes(path: Path, *, maximum: int, label: str) -> bytes:
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise AuthorityError(f"could not open {label} as a direct file: {path}") from error
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise AuthorityError(f"{label} must be one direct regular file")
        if before.st_size <= 0 or before.st_size > maximum:
            raise AuthorityError(f"{label} has an invalid size")
        chunks: list[bytes] = []
        remaining = maximum + 1
        while remaining > 0:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        data = b"".join(chunks)
        after = os.fstat(descriptor)
        stable = (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) == (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if not stable or len(data) != before.st_size:
            raise AuthorityError(f"{label} changed while it was being read")
        return data
    finally:
        os.close(descriptor)


def validate_keychain_path(raw: str, *, must_exist: bool) -> Path:
    candidate = Path(raw).expanduser()
    if not candidate.is_absolute():
        raise AuthorityError("--keychain must be an absolute path")
    keychain_directory = (Path.home() / "Library" / "Keychains").resolve(strict=True)
    if candidate.parent.resolve(strict=True) != keychain_directory:
        raise AuthorityError(
            "Authority 2 Keychain must be a direct file in this user's Library/Keychains"
        )
    if "authority-2" not in candidate.name.lower():
        raise AuthorityError("Authority 2 Keychain filename must contain 'Authority-2'")
    if candidate.name.lower().startswith("login.keychain"):
        raise AuthorityError("the login Keychain can never be used for Authority 2")
    directory_status = keychain_directory.stat()
    if directory_status.st_uid != os.getuid() or directory_status.st_mode & 0o022:
        raise AuthorityError("Library/Keychains has unsafe ownership or write permissions")
    if not must_exist:
        if candidate.exists() or candidate.is_symlink():
            raise AuthorityError("refusing to overwrite an existing Authority 2 Keychain")
        return candidate
    try:
        status = candidate.lstat()
    except OSError as error:
        raise AuthorityError(f"Authority 2 Keychain is missing: {candidate}") from error
    if not stat.S_ISREG(status.st_mode) or candidate.is_symlink():
        raise AuthorityError("Authority 2 Keychain must be a direct regular file")
    if status.st_uid != os.getuid() or status.st_mode & 0o022:
        raise AuthorityError("Authority 2 Keychain has unsafe ownership or write permissions")
    return candidate.resolve(strict=True)


def validate_output_path(raw: str) -> Path:
    path = Path(raw).expanduser()
    if not path.is_absolute():
        raise AuthorityError("--signature must be an absolute path")
    if path.exists() or path.is_symlink():
        raise AuthorityError("refusing to overwrite an existing signature")
    parent = path.parent.resolve(strict=True)
    if parent != path.parent:
        raise AuthorityError("signature parent directory must not use symlinks")
    status = parent.stat()
    if (
        not stat.S_ISDIR(status.st_mode)
        or status.st_uid != os.getuid()
        or status.st_mode & 0o022
    ):
        raise AuthorityError("signature parent directory has unsafe ownership")
    return path


def write_private_file(path: Path, data: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        view = memoryview(data)
        while view:
            written = os.write(descriptor, view)
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def write_public_exclusive(path: Path, data: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags, 0o644)
    except OSError as error:
        raise AuthorityError(f"could not create signature exclusively: {path}") from error
    complete = False
    try:
        view = memoryview(data)
        while view:
            written = os.write(descriptor, view)
            view = view[written:]
        os.fsync(descriptor)
        complete = True
    finally:
        os.close(descriptor)
        if not complete:
            with contextlib.suppress(OSError):
                path.unlink()


@contextlib.contextmanager
def compiled_policy_helper() -> Iterator[Path]:
    source = stable_file_bytes(
        SWIFT_POLICY_SOURCE,
        maximum=512 * 1024,
        label="Authority 2 Security.framework policy source",
    )
    with tempfile.TemporaryDirectory(prefix="goblins-authority2-policy-") as temporary:
        root = Path(temporary)
        source_snapshot = root / "display-authority2-keychain.swift"
        binary = root / "display-authority2-keychain"
        write_private_file(source_snapshot, source)
        run_checked(
            [
                "xcrun",
                "swiftc",
                "-O",
                "-suppress-warnings",
                "-framework",
                "Security",
                "-framework",
                "CryptoKit",
                str(source_snapshot),
                "-o",
                str(binary),
            ],
            timeout=120,
        )
        self_test = run_json_command([str(binary), "self-test"])
        if self_test != {
            "negative_cases": 10,
            "schema": POLICY_SCHEMA,
            "status": "pass",
        }:
            raise AuthorityError("Authority 2 policy helper self-test returned an invalid result")
        yield binary


def run_json_command(arguments: list[str], *, timeout: int = 60) -> dict[str, Any]:
    result = run_checked(arguments, timeout=timeout)
    try:
        value = json.loads(result.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError("Authority 2 policy helper returned invalid JSON") from error
    if type(value) is not dict:
        raise AuthorityError("Authority 2 policy helper did not return one JSON object")
    return value


def policy_command(
    binary: Path,
    command: str,
    keychain: Path,
    *,
    fingerprint: str | None = None,
) -> dict[str, Any]:
    arguments = [str(binary), command, "--keychain", str(keychain)]
    if command != "status":
        if fingerprint is None or not SHA256_RE.fullmatch(fingerprint):
            raise AuthorityError("missing canonical Authority 2 certificate fingerprint")
        arguments.extend(
            [
                "--identity",
                AUTHORITY_IDENTITY,
                "--certificate-sha256",
                fingerprint,
            ]
        )
    return run_json_command(arguments, timeout=300 if command == "harden" else 60)


STATUS_KEYS: Final = {
    "in_user_search_list",
    "is_user_default",
    "keychain",
    "lock_interval_seconds",
    "lock_on_sleep",
    "unlocked",
}
AUDIT_KEYS: Final = STATUS_KEYS | {
    "certificate_sha256",
    "change_acl_prompt_selector",
    "change_acl_trusted_application_count",
    "identity",
    "identity_count",
    "policy_sha256",
    "private_key_count",
    "schema",
    "sign_prompt_selector",
    "sign_trusted_application_count",
}


def validate_status_report(
    report: dict[str, Any], keychain: Path, *, expected_unlocked: bool
) -> None:
    if set(report) != STATUS_KEYS:
        raise AuthorityError("Authority 2 Keychain status report has unexpected fields")
    expected = {
        "in_user_search_list": False,
        "is_user_default": False,
        "keychain": str(keychain),
        "lock_on_sleep": True,
        "unlocked": expected_unlocked,
    }
    if any(report.get(name) != value for name, value in expected.items()):
        raise AuthorityError("Authority 2 Keychain is default, searchable, or in the wrong lock state")
    timeout = report.get("lock_interval_seconds")
    if type(timeout) is not int or not 1 <= timeout <= MAX_LOCK_INTERVAL_SECONDS:
        raise AuthorityError("Authority 2 Keychain lock timeout is unsafe")


def validate_audit_report(
    report: dict[str, Any], keychain: Path, fingerprint: str
) -> str:
    if set(report) != AUDIT_KEYS:
        raise AuthorityError("Authority 2 ACL audit report has unexpected fields")
    validate_status_report(
        {name: report[name] for name in STATUS_KEYS},
        keychain,
        expected_unlocked=True,
    )
    expected = {
        "certificate_sha256": fingerprint,
        "change_acl_trusted_application_count": 0,
        "identity": AUTHORITY_IDENTITY,
        "identity_count": 1,
        "private_key_count": 1,
        "schema": POLICY_SCHEMA,
        "sign_trusted_application_count": 0,
    }
    if any(report.get(name) != value for name, value in expected.items()):
        raise AuthorityError("Authority 2 identity or ACL policy is not exact")
    for name in ("sign_prompt_selector", "change_acl_prompt_selector"):
        value = report.get(name)
        if type(value) is not int or value & PROMPT_REQUIRE_PASSPHRASE == 0:
            raise AuthorityError("Authority 2 ACL does not require the Keychain passphrase")
    policy_digest = report.get("policy_sha256")
    if type(policy_digest) is not str or not SHA256_RE.fullmatch(policy_digest):
        raise AuthorityError("Authority 2 ACL policy digest is invalid")
    return policy_digest


def read_fingerprint(path: Path) -> str:
    data = stable_file_bytes(path, maximum=256, label="Authority 2 fingerprint")
    try:
        value = data.decode("ascii").strip()
    except UnicodeDecodeError as error:
        raise AuthorityError("Authority 2 fingerprint is not ASCII") from error
    if not SHA256_RE.fullmatch(value):
        raise AuthorityError("Authority 2 fingerprint must be one lowercase SHA-256 value")
    return value


def certificate_common_name(certificate: Path) -> str:
    result = run_checked(
        [
            "openssl",
            "x509",
            "-in",
            str(certificate),
            "-noout",
            "-subject",
            "-nameopt",
            "sep_multiline,utf8,space_eq,sname",
        ]
    )
    names = re.findall(r"^\s*CN = (.+)$", command_text(result.stdout), re.MULTILINE)
    if len(names) != 1:
        raise AuthorityError("Authority 2 certificate must contain exactly one common name")
    return names[0]


def read_pinned_certificates(
    certificate: Path,
    fingerprint_path: Path,
    ca_certificate: Path,
    ca_fingerprint_path: Path,
) -> tuple[bytes, str, bytes, str]:
    evidence = load_evidence_bundle()
    try:
        (
            certificate_data,
            fingerprint,
            ca_certificate_data,
            ca_fingerprint,
        ) = evidence.read_authority_certificates(
            str(certificate),
            str(fingerprint_path),
            str(ca_certificate),
            str(ca_fingerprint_path),
        )
    except Exception as error:
        raise AuthorityError(
            "Authority 2 leaf/CA certificate contract is invalid"
        ) from error
    if certificate_common_name(certificate) != AUTHORITY_IDENTITY:
        raise AuthorityError("pinned certificate is not the fixed Authority 2 identity")
    if fingerprint != read_fingerprint(fingerprint_path):
        raise AuthorityError("Authority 2 fingerprint changed during validation")
    if ca_fingerprint != read_fingerprint(ca_fingerprint_path):
        raise AuthorityError("Authority 2 CA fingerprint changed during validation")
    return certificate_data, fingerprint, ca_certificate_data, ca_fingerprint


@contextlib.contextmanager
def securityagent_unlock(binary: Path, keychain: Path) -> Iterator[None]:
    initial = policy_command(binary, "status", keychain)
    validate_status_report(initial, keychain, expected_unlocked=False)
    unlocked = False
    primary_error: BaseException | None = None
    try:
        # -u delegates password collection to SecurityAgent. No password is
        # accepted by this helper, its argv, its environment, or shell history.
        run_checked(
            ["security", "unlock-keychain", "-u", str(keychain)], timeout=300
        )
        unlocked = True
        current = policy_command(binary, "status", keychain)
        validate_status_report(current, keychain, expected_unlocked=True)
        yield
    except BaseException as error:
        primary_error = error
        raise
    finally:
        lock_error: BaseException | None = None
        if unlocked:
            try:
                run_checked(["security", "lock-keychain", str(keychain)])
            except BaseException as error:
                lock_error = error
        try:
            final = policy_command(binary, "status", keychain)
            validate_status_report(final, keychain, expected_unlocked=False)
        except BaseException as error:
            lock_error = lock_error or error
        if lock_error is not None:
            message = "Authority 2 Keychain could not be proven locked"
            if primary_error is not None:
                message += " after an earlier failure"
            raise AuthorityError(message) from lock_error


def tty_confirmation(
    values: list[tuple[str, str]],
    *,
    stream: TextIO | None = None,
    intent_phrase: str = "SIGN GOBLINS OS DISPLAY PROOF",
) -> None:
    close_stream = False
    if stream is None:
        try:
            stream = open("/dev/tty", "r+", encoding="utf-8", buffering=1)
            close_stream = True
        except OSError as error:
            raise AuthorityError("interactive Authority 2 confirmation requires /dev/tty") from error
    try:
        if hasattr(stream, "isatty") and not stream.isatty() and not isinstance(stream, io.StringIO):
            raise AuthorityError("Authority 2 confirmation input is not an interactive terminal")
        stream.write("\nAuthority 2 release-signing confirmation\n")
        stream.write("Review and re-enter every complete value. Abbreviations are rejected.\n\n")
        for label, expected in values:
            stream.write(f"{label}: {expected}\n")
        stream.write("\n")
        for label, expected in values:
            stream.write(f"Retype {label}: ")
            stream.flush()
            observed = stream.readline()
            if observed == "":
                raise AuthorityError("Authority 2 confirmation was cancelled")
            observed = observed.removesuffix("\n").removesuffix("\r")
            if observed != expected:
                raise AuthorityError(f"Authority 2 confirmation mismatch for {label}")
        stream.write(f"Type {intent_phrase}: ")
        stream.flush()
        observed = stream.readline().removesuffix("\n").removesuffix("\r")
        if observed != intent_phrase:
            raise AuthorityError("Authority 2 operator intent was not confirmed")
    finally:
        if close_stream:
            stream.close()


def confirmation_values(
    record: dict[str, Any],
    seal_data: bytes,
    *,
    candidate_commit: str,
    image_ref: str,
    run_date: str,
    screenshot_count: int,
) -> list[tuple[str, str]]:
    match = IMAGE_RE.fullmatch(image_ref)
    if match is None:
        raise AuthorityError("image reference must bind one immutable SHA-256 digest")
    expected_seal_digest = hashlib.sha256(seal_data).hexdigest()
    expected = {
        "candidate_commit": candidate_commit,
        "evidence_bundle_sha256": expected_seal_digest,
        "image_ref": image_ref,
        "run_date": run_date,
        "screenshot_count": screenshot_count,
    }
    if any(record.get(name) != value for name, value in expected.items()):
        raise AuthorityError("Authority 2 record does not match the explicit signing inputs")
    return [
        ("candidate commit", candidate_commit),
        ("image digest", "sha256:" + match.group(1)),
        ("run date", run_date),
        ("screenshot count", str(screenshot_count)),
        ("seal SHA-256", expected_seal_digest),
    ]


def validate_signing_inputs(args: argparse.Namespace) -> tuple[str, str, str, int]:
    candidate_commit = args.candidate_commit.lower()
    if not COMMIT_RE.fullmatch(candidate_commit):
        raise AuthorityError("--candidate-commit must be one exact 40-hex commit")
    image_ref = args.image_ref
    if IMAGE_RE.fullmatch(image_ref) is None:
        raise AuthorityError("--image-ref must contain one immutable sha256 digest")
    run_date = args.run_date
    if not DATE_RE.fullmatch(run_date):
        raise AuthorityError("--run-date must use YYYY-MM-DD")
    try:
        if dt.date.fromisoformat(run_date).isoformat() != run_date:
            raise ValueError
    except ValueError as error:
        raise AuthorityError("--run-date is not a real canonical date") from error
    screenshot_count = args.screenshot_count
    if type(screenshot_count) is not int or not 1 <= screenshot_count <= 100:
        raise AuthorityError("--screenshot-count is outside the supported range")
    return candidate_commit, image_ref, run_date, screenshot_count


def cms_command(
    keychain: Path, record_snapshot: Path, signature_temporary: Path
) -> list[str]:
    return [
        "security",
        "cms",
        "-S",
        "-N",
        AUTHORITY_IDENTITY,
        "-T",
        "-H",
        "SHA256",
        "-u",
        "4",
        "-k",
        str(keychain),
        "-i",
        str(record_snapshot),
        "-o",
        str(signature_temporary),
    ]


def finalize_signature(
    signature: bytes,
    *,
    before_policy_sha256: str,
    after_report: dict[str, Any],
    keychain: Path,
    fingerprint: str,
    output: Path,
) -> None:
    after_policy_sha256 = validate_audit_report(after_report, keychain, fingerprint)
    if after_policy_sha256 != before_policy_sha256:
        raise AuthorityError("Authority 2 ACL policy changed during signing")
    if not signature or len(signature) > MAX_SIGNATURE_BYTES:
        raise AuthorityError("Authority 2 CMS signature has an invalid size")
    write_public_exclusive(output, signature)


def command_sign(args: argparse.Namespace) -> int:
    require_supported_host()
    reject_password_environment()
    keychain = validate_keychain_path(args.keychain, must_exist=True)
    output = validate_output_path(args.signature)
    certificate = Path(args.certificate).resolve(strict=True)
    fingerprint_path = Path(args.certificate_sha256).resolve(strict=True)
    ca_certificate = Path(args.ca_certificate).resolve(strict=True)
    ca_fingerprint_path = Path(args.ca_certificate_sha256).resolve(strict=True)
    (
        certificate_data,
        fingerprint,
        ca_certificate_data,
        ca_fingerprint,
    ) = read_pinned_certificates(
        certificate,
        fingerprint_path,
        ca_certificate,
        ca_fingerprint_path,
    )
    candidate_commit, image_ref, run_date, screenshot_count = validate_signing_inputs(args)
    evidence = load_evidence_bundle()
    try:
        seal_data = evidence.read_direct_regular(
            args.seal, evidence.MAX_SEAL_BYTES, "Authority 2 evidence bundle"
        )
        record_data = evidence.read_direct_regular(
            args.record, evidence.MAX_SEAL_BYTES, "Authority 2 attestation record"
        )
        record = evidence.validate_attestation_record(
            record_data,
            seal_data,
            candidate_commit,
            image_ref,
            run_date,
            fingerprint,
            ca_fingerprint,
        )
    except Exception as error:
        raise AuthorityError("Authority 2 record or evidence bundle is invalid") from error
    values = confirmation_values(
        record,
        seal_data,
        candidate_commit=candidate_commit,
        image_ref=image_ref,
        run_date=run_date,
        screenshot_count=screenshot_count,
    )
    tty_confirmation(values)

    with tempfile.TemporaryDirectory(prefix="goblins-authority2-sign-") as temporary:
        temporary_root = Path(temporary)
        record_snapshot = temporary_root / "attestation.json"
        signature_temporary = temporary_root / "attestation.json.cms"
        write_private_file(record_snapshot, record_data)
        with compiled_policy_helper() as binary:
            with securityagent_unlock(binary, keychain):
                before_report = policy_command(
                    binary, "audit", keychain, fingerprint=fingerprint
                )
                before_policy = validate_audit_report(
                    before_report, keychain, fingerprint
                )
                run_checked(
                    cms_command(keychain, record_snapshot, signature_temporary),
                    timeout=300,
                )
                signature = stable_file_bytes(
                    signature_temporary,
                    maximum=MAX_SIGNATURE_BYTES,
                    label="temporary Authority 2 CMS signature",
                )
                try:
                    evidence.verify_cms_signature(
                        record_data,
                        signature,
                        certificate_data,
                        ca_certificate_data,
                    )
                except Exception as error:
                    raise AuthorityError(
                        "Authority 2 CMS signature failed pinned-certificate verification"
                    ) from error
                after_report = policy_command(
                    binary, "audit", keychain, fingerprint=fingerprint
                )
                after_policy = validate_audit_report(
                    after_report, keychain, fingerprint
                )
                if after_policy != before_policy:
                    raise AuthorityError("Authority 2 ACL policy changed during signing")
            final_status = policy_command(binary, "status", keychain)
            validate_status_report(final_status, keychain, expected_unlocked=False)
            finalize_signature(
                signature,
                before_policy_sha256=before_policy,
                after_report=after_report,
                keychain=keychain,
                fingerprint=fingerprint,
                output=output,
            )
    print(f"Authority 2 signature written after relock: {output}")
    return 0


def command_audit(args: argparse.Namespace) -> int:
    require_supported_host()
    reject_password_environment()
    keychain = validate_keychain_path(args.keychain, must_exist=True)
    _, fingerprint, _, _ = read_pinned_certificates(
        Path(args.certificate).resolve(strict=True),
        Path(args.certificate_sha256).resolve(strict=True),
        Path(args.ca_certificate).resolve(strict=True),
        Path(args.ca_certificate_sha256).resolve(strict=True),
    )
    with compiled_policy_helper() as binary:
        with securityagent_unlock(binary, keychain):
            report = policy_command(binary, "audit", keychain, fingerprint=fingerprint)
            validate_audit_report(report, keychain, fingerprint)
        validate_status_report(
            policy_command(binary, "status", keychain),
            keychain,
            expected_unlocked=False,
        )
    print(json.dumps(report, sort_keys=True, separators=(",", ":")))
    return 0


def command_harden(args: argparse.Namespace) -> int:
    require_supported_host()
    reject_password_environment()
    keychain = validate_keychain_path(args.keychain, must_exist=True)
    _, fingerprint, _, _ = read_pinned_certificates(
        Path(args.certificate).resolve(strict=True),
        Path(args.certificate_sha256).resolve(strict=True),
        Path(args.ca_certificate).resolve(strict=True),
        Path(args.ca_certificate_sha256).resolve(strict=True),
    )
    tty_confirmation(
        [
            ("Authority 2 certificate SHA-256", fingerprint),
            ("dedicated Keychain", str(keychain)),
        ],
        intent_phrase="HARDEN GOBLINS OS DISPLAY AUTHORITY 2",
    )
    with compiled_policy_helper() as binary:
        with securityagent_unlock(binary, keychain):
            report = policy_command(binary, "harden", keychain, fingerprint=fingerprint)
            first_digest = validate_audit_report(report, keychain, fingerprint)
            repeated = policy_command(binary, "audit", keychain, fingerprint=fingerprint)
            if validate_audit_report(repeated, keychain, fingerprint) != first_digest:
                raise AuthorityError("Authority 2 ACL changed immediately after hardening")
        validate_status_report(
            policy_command(binary, "status", keychain),
            keychain,
            expected_unlocked=False,
        )
    print(json.dumps(report, sort_keys=True, separators=(",", ":")))
    return 0


def current_search_list() -> list[str]:
    output = command_text(
        run_checked(["security", "list-keychains", "-d", "user"]).stdout
    )
    try:
        paths = shlex.split(output)
    except ValueError as error:
        raise AuthorityError("could not parse the current user Keychain search list") from error
    if not paths:
        raise AuthorityError("refusing to replace an unexpectedly empty Keychain search list")
    return paths


def restore_search_list(paths: list[str]) -> None:
    run_checked(["security", "list-keychains", "-d", "user", "-s", *paths])


def initialize_confirmation(keychain: Path) -> None:
    phrase = f"CREATE {keychain}"
    try:
        with open("/dev/tty", "r+", encoding="utf-8", buffering=1) as terminal:
            terminal.write(
                "This creates an empty dedicated Keychain and restores the existing search list.\n"
            )
            terminal.write(f"Type {phrase}: ")
            terminal.flush()
            observed = terminal.readline().removesuffix("\n").removesuffix("\r")
    except OSError as error:
        raise AuthorityError("Keychain initialization requires /dev/tty") from error
    if observed != phrase:
        raise AuthorityError("Authority 2 Keychain creation was not confirmed")


def command_initialize(args: argparse.Namespace) -> int:
    require_supported_host()
    reject_password_environment()
    keychain = validate_keychain_path(args.keychain, must_exist=False)
    initialize_confirmation(keychain)
    search_list = current_search_list()
    created = False
    operation_error: BaseException | None = None
    try:
        # Uppercase -P is the documented SecurityAgent prompt. Lowercase -p,
        # which would expose the password in argv, is intentionally unsupported.
        run_checked(["security", "create-keychain", "-P", str(keychain)], timeout=300)
        created = True
        run_checked(
            [
                "security",
                "set-keychain-settings",
                "-l",
                "-u",
                "-t",
                str(MAX_LOCK_INTERVAL_SECONDS),
                str(keychain),
            ]
        )
    except BaseException as error:
        operation_error = error
        raise
    finally:
        cleanup_error: BaseException | None = None
        try:
            restore_search_list(search_list)
        except BaseException as error:
            cleanup_error = error
        if created or keychain.exists():
            try:
                run_checked(["security", "lock-keychain", str(keychain)])
            except BaseException as error:
                cleanup_error = cleanup_error or error
        if cleanup_error is not None:
            message = "could not restore the search list and locked state"
            if operation_error is not None:
                message += " after Keychain initialization failed"
            raise AuthorityError(message) from cleanup_error
    keychain = validate_keychain_path(str(keychain), must_exist=True)
    with compiled_policy_helper() as binary:
        validate_status_report(
            policy_command(binary, "status", keychain),
            keychain,
            expected_unlocked=False,
        )
    print(f"Created locked, non-default Authority 2 Keychain: {keychain}")
    print("No identity was created. Run provision-checklist for the reviewed next steps.")
    return 0


def command_checklist(args: argparse.Namespace) -> int:
    keychain = Path(args.keychain).expanduser().absolute()
    certificate = Path(args.certificate).expanduser().absolute()
    fingerprint = Path(args.certificate_sha256).expanduser().absolute()
    ca_certificate = Path(args.ca_certificate).expanduser().absolute()
    ca_fingerprint = Path(args.ca_certificate_sha256).expanduser().absolute()
    helper = Path(__file__).resolve()
    quote = shlex.quote
    print(
        f"""Authority 2 controlled provisioning checklist

1. Use a dedicated, non-admin signing account. Do not run the Actions runner in
   that account. Stop candidate capture code before provisioning or signing.

2. Create the empty dedicated Keychain through SecurityAgent:
   {quote(str(helper))} initialize-keychain --keychain {quote(str(keychain))}

3. Mount a temporary RAM disk for CA material. Generate an encrypted ephemeral
   offline CA there; never place its private key in the repository or persistent
   storage. Save only the CA public certificate before ejecting the RAM disk.

4. Generate the leaf key directly inside the dedicated Keychain (certtool will
   interactively collect CSR fields; use CN '{AUTHORITY_IDENTITY}'):
   security unlock-keychain -u {quote(str(keychain))}
   certtool r <RAM-DISK>/authority2.csr k={quote(str(keychain))}

5. On the RAM disk, sign that CSR for the reviewed validity period with SHA-256,
   keyUsage=digitalSignature and extendedKeyUsage=emailProtection. Import only
   the leaf certificate back into the dedicated Keychain:
   certtool i <RAM-DISK>/authority2-leaf.pem k={quote(str(keychain))}

6. Export and review the public leaf certificate at:
   {certificate}
   Write its lowercase DER SHA-256, followed by one newline, at:
   {fingerprint}
   Publish the offline CA public certificate at:
   {ca_certificate}
   Write its lowercase DER SHA-256, followed by one newline, at:
   {ca_fingerprint}
   Eject the RAM disk to destroy the CA private key. Never publish private key
   material.

7. Harden the exact leaf private key through Security.framework, then audit and
   relock it:
   {quote(str(helper))} harden --keychain {quote(str(keychain))} \\
     --certificate {quote(str(certificate))} \\
     --certificate-sha256 {quote(str(fingerprint))} \\
     --ca-certificate {quote(str(ca_certificate))} \\
     --ca-certificate-sha256 {quote(str(ca_fingerprint))}

8. Run the independent audit before integration:
   {quote(str(helper))} audit --keychain {quote(str(keychain))} \\
     --certificate {quote(str(certificate))} \\
     --certificate-sha256 {quote(str(fingerprint))} \\
     --ca-certificate {quote(str(ca_certificate))} \\
     --ca-certificate-sha256 {quote(str(ca_fingerprint))}

Never use security import -A, import -T, set-key-partition-list, a password
argument, a password environment variable, or “Always Allow.” The helper fails
closed unless the key has exactly sign-only and change_acl-only interactive ACLs,
zero trusted apps, no partitions, exactly one identity/private key, a 1-300s
timeout, lock-on-sleep, and no default-search-list membership.
"""
    )
    return 0


def add_identity_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--keychain", default=str(DEFAULT_KEYCHAIN))
    parser.add_argument("--certificate", required=True)
    parser.add_argument("--certificate-sha256", required=True)
    parser.add_argument("--ca-certificate", required=True)
    parser.add_argument("--ca-certificate-sha256", required=True)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Interactive, fail-closed Goblins OS Display Authority 2 helper"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    initialize = subparsers.add_parser(
        "initialize-keychain", help="create only the locked dedicated Keychain"
    )
    initialize.add_argument("--keychain", default=str(DEFAULT_KEYCHAIN))
    initialize.set_defaults(handler=command_initialize)

    checklist = subparsers.add_parser(
        "provision-checklist", help="print non-secret Authority 2 provisioning steps"
    )
    add_identity_arguments(checklist)
    checklist.set_defaults(handler=command_checklist)

    audit = subparsers.add_parser("audit", help="SecurityAgent-unlock, audit, and relock")
    add_identity_arguments(audit)
    audit.set_defaults(handler=command_audit)

    harden = subparsers.add_parser(
        "harden", help="interactively restrict the Authority 2 key to signing"
    )
    add_identity_arguments(harden)
    harden.set_defaults(handler=command_harden)

    sign = subparsers.add_parser(
        "sign", help="confirm, audit, CMS-sign, re-audit, verify, and relock"
    )
    add_identity_arguments(sign)
    sign.add_argument("--seal", required=True)
    sign.add_argument("--record", required=True)
    sign.add_argument("--signature", required=True)
    sign.add_argument("--candidate-commit", required=True)
    sign.add_argument("--image-ref", required=True)
    sign.add_argument("--run-date", required=True)
    sign.add_argument("--screenshot-count", required=True, type=int)
    sign.set_defaults(handler=command_sign)
    return parser


def main() -> int:
    try:
        args = build_parser().parse_args()
        return int(args.handler(args))
    except AuthorityError as error:
        print(f"display-authority2: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
