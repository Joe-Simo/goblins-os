#!/usr/bin/env python3
"""Adversarial, non-mutating self-test for the Authority 2 signing helper."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from typing import Any
from unittest import mock


HERE = Path(__file__).resolve().parent
HELPER = HERE / "display-authority2.py"
SPEC = importlib.util.spec_from_file_location("display_authority2", HELPER)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load display-authority2.py")
authority = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(authority)


class FakeTTY:
    def __init__(self, lines: list[str]) -> None:
        self.lines = iter(lines)
        self.output = io.StringIO()

    def isatty(self) -> bool:
        return True

    def write(self, value: str) -> int:
        return self.output.write(value)

    def flush(self) -> None:
        return None

    def readline(self) -> str:
        return next(self.lines, "")


class Authority2AdversarialTest(unittest.TestCase):
    keychain = Path(
        "/Users/release/Library/Keychains/GoblinsOS-Display-Authority-2.keychain-db"
    )
    fingerprint = "a" * 64

    def safe_report(self) -> dict[str, Any]:
        return {
            "certificate_sha256": self.fingerprint,
            "change_acl_prompt_selector": 1,
            "change_acl_trusted_application_count": 0,
            "identity": authority.AUTHORITY_IDENTITY,
            "identity_count": 1,
            "in_user_search_list": False,
            "is_user_default": False,
            "keychain": str(self.keychain),
            "lock_interval_seconds": 300,
            "lock_on_sleep": True,
            "policy_sha256": "b" * 64,
            "private_key_count": 1,
            "schema": authority.POLICY_SCHEMA,
            "sign_prompt_selector": 1,
            "sign_trusted_application_count": 0,
            "unlocked": True,
        }

    def test_swift_security_framework_policy_self_test(self) -> None:
        with authority.compiled_policy_helper() as binary:
            value = authority.run_json_command([str(binary), "self-test"])
        self.assertEqual(
            value,
            {
                "negative_cases": 10,
                "schema": authority.POLICY_SCHEMA,
                "status": "pass",
            },
        )

    def test_python_report_contract_rejects_unsafe_mutations(self) -> None:
        authority.validate_audit_report(
            self.safe_report(), self.keychain, self.fingerprint
        )
        mutations = [
            ("sign_trusted_application_count", 1),
            ("change_acl_trusted_application_count", 1),
            ("sign_prompt_selector", 0),
            ("change_acl_prompt_selector", 0),
            ("identity_count", 2),
            ("private_key_count", 2),
            ("in_user_search_list", True),
            ("is_user_default", True),
            ("lock_interval_seconds", 0),
            ("lock_on_sleep", False),
        ]
        for name, value in mutations:
            with self.subTest(name=name):
                report = self.safe_report()
                report[name] = value
                with self.assertRaises(authority.AuthorityError):
                    authority.validate_audit_report(
                        report, self.keychain, self.fingerprint
                    )

    def test_mutated_post_sign_acl_never_publishes_signature(self) -> None:
        with tempfile.TemporaryDirectory(prefix="authority2-finalize-test-") as root:
            output = Path(root) / "attestation.cms"
            after = self.safe_report()
            after["sign_trusted_application_count"] = 1
            with self.assertRaises(authority.AuthorityError):
                authority.finalize_signature(
                    b"not-a-real-signature",
                    before_policy_sha256="b" * 64,
                    after_report=after,
                    keychain=self.keychain,
                    fingerprint=self.fingerprint,
                    output=output,
                )
            self.assertFalse(output.exists())

            changed = self.safe_report()
            changed["policy_sha256"] = "c" * 64
            with self.assertRaises(authority.AuthorityError):
                authority.finalize_signature(
                    b"not-a-real-signature",
                    before_policy_sha256="b" * 64,
                    after_report=changed,
                    keychain=self.keychain,
                    fingerprint=self.fingerprint,
                    output=output,
                )
            self.assertFalse(output.exists())

    def test_safe_post_sign_policy_publishes_exclusively(self) -> None:
        with tempfile.TemporaryDirectory(prefix="authority2-finalize-test-") as root:
            output = Path(root) / "attestation.cms"
            authority.finalize_signature(
                b"verified-signature",
                before_policy_sha256="b" * 64,
                after_report=self.safe_report(),
                keychain=self.keychain,
                fingerprint=self.fingerprint,
                output=output,
            )
            self.assertEqual(output.read_bytes(), b"verified-signature")
            with self.assertRaises(authority.AuthorityError):
                authority.finalize_signature(
                    b"replacement",
                    before_policy_sha256="b" * 64,
                    after_report=self.safe_report(),
                    keychain=self.keychain,
                    fingerprint=self.fingerprint,
                    output=output,
                )

    def test_cms_command_is_detached_explicit_and_password_free(self) -> None:
        command = authority.cms_command(
            self.keychain, Path("/private/record"), Path("/private/signature")
        )
        self.assertEqual(command[0:3], ["security", "cms", "-S"])
        self.assertIn("-T", command)
        self.assertEqual(command[command.index("-H") + 1], "SHA256")
        self.assertEqual(command[command.index("-u") + 1], "4")
        self.assertEqual(command[command.index("-k") + 1], str(self.keychain))
        self.assertNotIn("-p", command)
        self.assertNotIn("-P", command)

    def test_typed_confirmation_requires_every_exact_value(self) -> None:
        values = [
            ("candidate commit", "1" * 40),
            ("image digest", "sha256:" + "2" * 64),
            ("run date", "2026-07-22"),
            ("screenshot count", "42"),
            ("seal SHA-256", "3" * 64),
        ]
        correct = FakeTTY(
            [value + "\n" for _, value in values]
            + ["SIGN GOBLINS OS DISPLAY PROOF\n"]
        )
        authority.tty_confirmation(values, stream=correct)

        wrong_lines = [value + "\n" for _, value in values]
        wrong_lines[3] = "41\n"
        wrong = FakeTTY(wrong_lines + ["SIGN GOBLINS OS DISPLAY PROOF\n"])
        with self.assertRaises(authority.AuthorityError):
            authority.tty_confirmation(values, stream=wrong)

    def test_confirmation_values_bind_record_and_seal(self) -> None:
        seal = b"canonical-seal"
        digest = __import__("hashlib").sha256(seal).hexdigest()
        image_ref = "ghcr.io/joe-simo/goblins-os@sha256:" + "2" * 64
        record = {
            "candidate_commit": "1" * 40,
            "evidence_bundle_sha256": digest,
            "image_ref": image_ref,
            "run_date": "2026-07-22",
            "screenshot_count": 42,
        }
        values = authority.confirmation_values(
            record,
            seal,
            candidate_commit="1" * 40,
            image_ref=image_ref,
            run_date="2026-07-22",
            screenshot_count=42,
        )
        self.assertEqual(values[-1], ("seal SHA-256", digest))
        record["screenshot_count"] = 41
        with self.assertRaises(authority.AuthorityError):
            authority.confirmation_values(
                record,
                seal,
                candidate_commit="1" * 40,
                image_ref=image_ref,
                run_date="2026-07-22",
                screenshot_count=42,
            )

    def test_password_bearing_environment_is_rejected_without_reading_values(self) -> None:
        with mock.patch.dict(os.environ, {"KEYCHAIN_PASSWORD": "do-not-log"}, clear=False):
            with self.assertRaisesRegex(
                authority.AuthorityError, "KEYCHAIN_PASSWORD"
            ) as captured:
                authority.reject_password_environment()
            self.assertNotIn("do-not-log", str(captured.exception))


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Authority2AdversarialTest)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    summary = {
        "errors": len(result.errors),
        "failures": len(result.failures),
        "schema": authority.POLICY_SCHEMA,
        "status": "pass" if result.wasSuccessful() else "fail",
        "tests_run": result.testsRun,
    }
    print(json.dumps(summary, sort_keys=True, separators=(",", ":")))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
