#!/usr/bin/env python3
"""Strict validation for display-backed hardware-gate proof JSON.

The capture channel intentionally serializes query values as strings.  This
validator treats that representation as a versioned wire contract: every key
must be present exactly once, every value must be a string, and dynamic values
must match a narrow full-string pattern.  It is shared by capture, signoff, and
shipping verification so textual lookalikes cannot satisfy a proof gate.
"""

from __future__ import annotations

import json
import os
import re
import stat
import sys
import tempfile
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Final, Pattern

MAX_PROOF_BYTES: Final = 256 * 1024
MAX_VALUE_CHARACTERS: Final = 4096
CAPTURED_VIA: Final = "display-backed VM HTTP proof signal"
HEX_SHA256 = re.compile(r"[0-9a-f]{64}")
POSITIVE_INTEGER = re.compile(r"[1-9][0-9]{0,18}")
NONNEGATIVE_INTEGER = re.compile(r"(?:0|[1-9][0-9]{0,18})")
SAFE_IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,255}")
SAFE_TEXT = re.compile(r"[^\x00-\x1f\x7f]{1,1024}")
COMMIT_SHA = re.compile(r"[0-9a-f]{40}")
IMAGE_REF = re.compile(
    r"ghcr\.io/[a-z0-9][a-z0-9._/-]*[a-z0-9]@sha256:[0-9a-f]{64}"
)
WORKFLOW_RUN = re.compile(r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*")

MANIFEST_FIXED_VALUES: Final[dict[str, str]] = {
    "verification_iso_manifest": "verification-iso-manifest.json",
    "verification_bib_manifest": "verification-bib-manifest.json",
    "verification_release_evidence_manifest": "verification-release-evidence-manifest.json",
    "firewall_live_toggle_proof": "firewall-live-toggle-proof.json",
    "text_shortcuts_session_enable_proof": "text-shortcuts-session-enable-proof.json",
    "text_shortcuts_candidate_metadata_proof": "text-shortcuts-candidate-metadata-proof.json",
    "text_shortcuts_overlay_intent_proof": "text-shortcuts-overlay-intent-proof.json",
    "text_shortcuts_candidate_bubble_frame_proof": "text-shortcuts-candidate-bubble-frame-proof.json",
    "text_shortcuts_candidate_bubble_layout_proof": "text-shortcuts-candidate-bubble-layout-proof.json",
    "text_shortcuts_candidate_bubble_render_intent_proof": "text-shortcuts-candidate-bubble-render-intent-proof.json",
    "text_shortcuts_candidate_bubble_render_proof": "text-shortcuts-candidate-bubble-render-proof.json",
    "text_shortcuts_live_ibus_runtime_render_proof": "text-shortcuts-live-ibus-runtime-render-proof.json",
    "keyboard_shortcuts_roundtrip_proof": "keyboard-shortcuts-roundtrip-proof.json",
    "input_sources_roundtrip_proof": "input-sources-roundtrip-proof.json",
    "multi_display_apply_proof": "multi-display-apply-proof.json",
    "focus_arm_roundtrip_proof": "focus-arm-roundtrip-proof.json",
    "app_privacy_revoke_proof": "app-privacy-revoke-proof.json",
    "preview_open_render_proof": "preview-open-render-proof.json",
    "audio_output_proof": "audio-output-proof.json",
    "runtime_build_proof": "runtime-build-proof.json",
    "capture_method": (
        "display-backed qemu VM, software GPU/audio substrate "
        "(lavapipe/gamescope/pipewire), honestly labeled"
    ),
}


class ProofValidationError(ValueError):
    pass


@dataclass(frozen=True)
class ProofSchema:
    name: str
    expected: dict[str, str]
    patterns: dict[str, Pattern[str]]


def exact(**values: str) -> dict[str, str]:
    return values


def schema(
    name: str,
    expected: dict[str, str],
    patterns: dict[str, Pattern[str]] | None = None,
) -> ProofSchema:
    common = {"name": name, "captured_via": CAPTURED_VIA}
    common.update(expected)
    return ProofSchema(name=name, expected=common, patterns=patterns or {})


PROOF_SCHEMAS: Final[dict[str, ProofSchema]] = {
    "firewall-live-toggle": schema(
        "firewall-live-toggle",
        exact(
            status="pass",
            route="/v1/firewall/enabled",
            status_route="/v1/firewall/status",
            disable_http="200",
            disable_ok="true",
            disable_enabled="false",
            disable_active="false",
            enable_http="200",
            enable_ok="true",
            enable_enabled="true",
            enable_active="true",
            unit_template="goblins-os-firewall@.service",
            polkit_rule="60-goblins-os-firewall.rules",
        ),
    ),
    "text-shortcuts-session-enable": schema(
        "text-shortcuts-session-enable",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            proof_scope="session-plumbing",
            service="active",
            service_unit="org.freedesktop.IBus.session.GNOME.service",
            input_source_configured="true",
            preload_configured="true",
            engine_listed="true",
            adapter_self_test="pass",
            engine_set="pass",
            active_engine="goblins-textshortcuts",
            core_http="200",
            core_ibus_available="true",
            core_component_registered="true",
            core_engine_binary_available="true",
            core_input_source_configured="true",
            runtime_ready_claim="false",
        ),
        {
            "core_engine_available": re.compile(r"(?:true|false)"),
            "core_runtime_loop_available": re.compile(r"(?:true|false)"),
        },
    ),
    "text-shortcuts-candidate-metadata": schema(
        "text-shortcuts-candidate-metadata",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-os-shell-text-shortcuts-candidate-proof",
            candidate_replacement="on my way",
            candidate_accept_on="word-boundary",
            candidate_dismiss_key="Escape",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-overlay-intent": schema(
        "text-shortcuts-overlay-intent",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-textshortcuts-ibus-adapter-overlay-intent",
            adapter_self_test="pass",
            show_count="2",
            hide_count="2",
            dismissed_reason="true",
            committed_reason="true",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-candidate-bubble-frame": schema(
        "text-shortcuts-candidate-bubble-frame",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-textshortcuts-accept-bubble-frame",
            adapter_self_test="pass",
            show_frame_count="2",
            hide_frame_count="2",
            dismissed_frame="true",
            committed_frame="true",
            replacement="on my way",
            accept_on="word-boundary",
            accept_keys="Space,Return",
            dismiss_key="Escape",
            style_class="gos-text-shortcuts-candidate",
            text_style_class="gos-text-shortcuts-candidate-text",
            hint_style_class="gos-text-shortcuts-candidate-hint",
            font_family="Inter",
            sensitive_field_refusal="true",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-candidate-bubble-layout": schema(
        "text-shortcuts-candidate-bubble-layout",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-textshortcuts-accept-bubble-layout",
            adapter_self_test="pass",
            frame_surface="goblins-textshortcuts-accept-bubble-frame",
            layout_count="4",
            visible_layout_count="3",
            right_edge_clamped="true",
            bottom_edge_flipped="true",
            hidden_frame_collapses="true",
            style_class="gos-text-shortcuts-candidate",
            font_family="Inter",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-candidate-bubble-render-intent": schema(
        "text-shortcuts-candidate-bubble-render-intent",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-textshortcuts-accept-bubble-render-intent",
            adapter_self_test="pass",
            frame_surface="goblins-textshortcuts-accept-bubble-frame",
            layout_surface="goblins-textshortcuts-accept-bubble-layout",
            render_intent_count="8",
            show_intent_count="4",
            hide_intent_count="4",
            dismissed_intent="true",
            committed_intent="true",
            focus_out_hide="true",
            sensitive_hide="true",
            pass_through_unchanged="true",
            key_release_preserved_candidate="true",
            runtime_failure_cleanup="true",
            sink_failure_fail_open="true",
            style_class="gos-text-shortcuts-candidate",
            font_family="Inter",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-candidate-bubble-render": schema(
        "text-shortcuts-candidate-bubble-render",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            surface="goblins-os-shell-text-shortcuts-candidate-bubble-render",
            render_intent_surface="goblins-textshortcuts-accept-bubble-render-intent",
            layout_surface="goblins-textshortcuts-accept-bubble-layout",
            frame_surface="goblins-textshortcuts-accept-bubble-frame",
            replacement="on my way",
            accept_on="word-boundary",
            dismiss_key="Escape",
            style_class="gos-text-shortcuts-candidate",
            text_style_class="gos-text-shortcuts-candidate-text",
            hint_style_class="gos-text-shortcuts-candidate-hint",
            font_family="Inter",
            screenshot="31-text-shortcuts-candidate-bubble-render.png",
            rendered_candidate_surface="true",
            rendered_bubble_ready_claim="false",
            live_overlay_claim="false",
            runtime_ready_claim="false",
        ),
    ),
    "text-shortcuts-live-ibus-runtime-render": schema(
        "text-shortcuts-live-ibus-runtime-render",
        exact(
            status="pass",
            route="/v1/text-shortcuts",
            preview_route="/v1/text-shortcuts/preview",
            surface="goblins-textshortcuts-live-ibus-runtime-render",
            input_driver="qmp-keyboard",
            active_engine="goblins-textshortcuts",
            seed_write_http="200",
            seed_read_http="200",
            seed_roundtrip="true",
            seed_loaded="true",
            core_write_http="200",
            core_read_http="200",
            core_preview_http="200",
            file_contract_http="200",
            core_table_roundtrip="true",
            core_preview_roundtrip="true",
            desktop_file_contract="true",
            desktop_parent_contract="true",
            desktop_file_owner_mode="true",
            desktop_file_single_link="true",
            desktop_file_size_bounded="true",
            desktop_file_bounded_read="true",
            legacy_service_table_absent="true",
            live_watcher_reload="true",
            post_keystroke_read_http="200",
            post_keystroke_file_http="200",
            post_keystroke_roundtrip="true",
            normal_actual="on my way.",
            passthrough_actual="hello.",
            password_refusal="true",
            password_sensitive_purpose="true",
            password_process_key_callback="true",
            password_commit_absent="true",
            password_candidate_absent="true",
            password_popup_absent="true",
            normal_stage_ledger_scoped="true",
            focused_field_callback="true",
            process_key_event_callback="true",
            cursor_location_callback="true",
            pre_boundary_commit_absent="true",
            boundary_stage_ledger_scoped="true",
            boundary_stage_commit_count="1",
            normal_stage_commit="true",
            ibus_commit_operation="true",
            focused_entry_readback="true",
            ibus_commit_delivered="true",
            boundary_popup_action="hide-candidate",
            boundary_popup_reason="committed",
            candidate_intent_seen="true",
            native_ibus_candidate_published="true",
            native_popup_generation_current="true",
            native_popup_record_current_at_capture="true",
            native_popup_action="show-candidate",
            native_popup_has_cursor_rect="true",
            native_popup_expected_replacement="true",
            native_popup_hint_published="true",
            renderer="native-ibus-lookup-table",
            cursor_anchor="ibus-input-context",
            synthetic_overlay="false",
            screenshot="32-text-shortcuts-live-ibus-runtime-render.png",
            screenshot_capture_ack="true",
            native_candidate_popup_ready_claim="true",
            live_overlay_claim="true",
            runtime_ready_claim="true",
            core_readiness_flip="live",
        ),
        {
            "native_popup_generation": POSITIVE_INTEGER,
            "native_popup_record_ordinal": POSITIVE_INTEGER,
            "screenshot_sha256": HEX_SHA256,
        },
    ),
    "keyboard-shortcuts-roundtrip": schema(
        "keyboard-shortcuts-roundtrip",
        exact(
            status="pass",
            shortcut_route="/v1/keyboard/shortcuts/binding",
            modifier_route="/v1/keyboard/modifier-remap",
            shortcut_action="window-hud",
            shortcut_binding="<Super><Shift>H",
            shortcut_http="200",
            shortcut_gsettings_readback="true",
            shortcut_reset_http="200",
            shortcut_reset_binding="<Super>w",
            modifier_target="caps-lock",
            modifier_value="control",
            modifier_http="200",
            modifier_gsettings_readback="ctrl:nocaps",
            modifier_reset_http="200",
            modifier_restore="default",
            roundtrip_restored="true",
        ),
    ),
    "input-sources-roundtrip": schema(
        "input-sources-roundtrip",
        exact(
            status="pass",
            source_route="/v1/input/sources",
            switch_route="/v1/input/switch-next",
            test_sources="xkb-us,xkb-gb",
            set_http="200",
            set_ok="true",
            sources_gsettings_readback="true",
            current_before_switch="0",
            switch_http="200",
            switch_ok="true",
            switch_switched="true",
            current_after_switch="1",
            restore_sources="true",
            restore_current="true",
            roundtrip_restored="true",
        ),
    ),
    "multi-display-apply": schema(
        "multi-display-apply",
        exact(
            status="pass",
            status_route="/v1/displays/status",
            apply_route="/v1/displays/apply",
            display_config="org.gnome.Mutter.DisplayConfig",
            verify_http="200",
            verify_ok="true",
            temporary_http="200",
            temporary_ok="true",
            persistent_guard_http="400",
            persistent_confirmation_required="true",
            stale_serial_http="409",
            stale_serial_rejected="true",
            roundtrip_restored="true",
            persistent_keep_claim="false",
            same_layout_noop="true",
        ),
        {
            "connector": SAFE_TEXT,
            "mode_id": SAFE_TEXT,
            "serial": NONNEGATIVE_INTEGER,
            "stale_serial": NONNEGATIVE_INTEGER,
        },
    ),
    "focus-arm-roundtrip": schema(
        "focus-arm-roundtrip",
        exact(
            status="pass",
            status_route="/v1/focus/status",
            activate_route="/v1/focus/activate",
            deactivate_route="/v1/focus/deactivate",
            status_http="200",
            available="true",
            test_mode="gate-work",
            test_mode_configured="true",
            baseline_active_mode="",
            baseline_banners="true",
            activate_http="200",
            activate_ok="true",
            activate_active_mode="gate-work",
            active_mode_gsettings_readback="gate-work",
            armed_by_schedule_after_activate="false",
            restore_banners_after_activate="true",
            notification_banners_after_activate="false",
            deactivate_http="200",
            deactivate_ok="true",
            deactivate_active_mode="",
            active_mode_after_deactivate="",
            armed_by_schedule_after_deactivate="false",
            restore_banners_after_deactivate="",
            notification_banners_after_deactivate="true",
            original_focus_state_restored="true",
            original_notification_banners_restored="true",
            roundtrip_restored="true",
            mode_crud_claim="false",
            schedule_claim="false",
            per_app_breakthroughs_claim="false",
        ),
    ),
    "app-privacy-revoke": schema(
        "app-privacy-revoke",
        exact(
            status="pass",
            route="/v1/app-privacy/revoke",
            table="location",
            app="org.goblins.GatePrivacyProof",
            id="org.goblins.GatePrivacyProof",
            seed_method="PermissionStore.SetPermission",
            revoke_method="PermissionStore.DeletePermission",
            readback_method="PermissionStore.GetPermission",
            seed_grant="yes",
            seed_readback="true",
            revoke_http="200",
            revoke_ok="true",
            post_revoke_absent="true",
            restore_prior_state="true",
            roundtrip_restored="true",
            resource_keyed_claim="false",
            device_revoke_claim="false",
        ),
        {"seed_attempt": re.compile(r"(?:typed|plain)")},
    ),
    "preview-open-render": schema(
        "preview-open-render",
        exact(
            status="pass",
            status_route="/v1/preview/status",
            route="/v1/preview/open",
            status_http="200",
            available="true",
            xdg_open="true",
            papers="true",
            loupe="true",
            pdf_default="org.gnome.Papers.desktop",
            image_default="org.gnome.Loupe.desktop",
            jpeg_default="org.gnome.Loupe.desktop",
            pdf_http="200",
            pdf_ok="true",
            pdf_kind="pdf",
            pdf_process="papers",
            pdf_screenshot="29-preview-pdf-open.png",
            rendered_pdf_frame="true",
            image_http="200",
            image_ok="true",
            image_kind="image",
            image_process="loupe",
            image_screenshot="30-preview-image-open.png",
            rendered_image_frame="true",
            unsupported_http="400",
            unsupported_ok="false",
            unsupported_rejected="true",
        ),
    ),
    "audio-output": schema(
        "audio-output",
        exact(
            status="pass",
            status_route="/v1/audio/status",
            status_http="200",
            wireplumber_available="true",
            output_available="true",
            test_tone_seconds="45",
            screenshot="24-audio-output.png",
            rendered_sound_panel="true",
        ),
        {
            "output_volume": re.compile(r"(?:0|[1-9][0-9]{0,2})"),
            "output_muted": re.compile(r"(?:true|false)"),
            "player": re.compile(r"(?:pw-play|paplay)"),
            "core_restarts": NONNEGATIVE_INTEGER,
        },
    ),
    "runtime-build": schema(
        "runtime-build",
        exact(status="pass", route="/v1/apps/builds", engine_mode="local-model"),
        {
            "intent": SAFE_TEXT,
            "engine_source": re.compile(r"[A-Za-z0-9._:-]+-built"),
            "built_artifact_id": SAFE_IDENTIFIER,
            "built_artifact_name": SAFE_TEXT,
            "response_bytes": POSITIVE_INTEGER,
        },
    ),
}

PATTERN_SELF_TEST_VALUES: Final[dict[str, str]] = {
    "core_engine_available": "false",
    "core_runtime_loop_available": "false",
    "native_popup_generation": "1",
    "native_popup_record_ordinal": "1",
    "screenshot_sha256": "0" * 64,
    "connector": "Virtual-1",
    "mode_id": "1920x1080@60",
    "serial": "2",
    "stale_serial": "1",
    "seed_attempt": "typed",
    "output_volume": "100",
    "output_muted": "false",
    "player": "pw-play",
    "core_restarts": "0",
    "intent": "A focus timer that counts down 25 minutes and rings.",
    "engine_source": "llama3.2:1b-built",
    "built_artifact_id": "focus-timer",
    "built_artifact_name": "Focus Timer",
    "response_bytes": "1024",
}


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ProofValidationError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def reject_json_constant(value: str) -> object:
    raise ProofValidationError(f"non-finite JSON number: {value}")


def load_bounded_json(path: Path) -> object:
    try:
        before = path.lstat()
    except OSError as error:
        raise ProofValidationError(f"cannot stat proof: {error}") from error
    if not stat.S_ISREG(before.st_mode) or stat.S_ISLNK(before.st_mode):
        raise ProofValidationError("proof must be a regular non-symlink file")
    if before.st_nlink != 1:
        raise ProofValidationError("proof must not be hard-linked")
    if before.st_size <= 0 or before.st_size > MAX_PROOF_BYTES:
        raise ProofValidationError("proof size is outside the bounded contract")

    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ProofValidationError(f"cannot securely open proof: {error}") from error
    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode):
            raise ProofValidationError("opened proof is not a regular file")
        if opened.st_nlink != 1:
            raise ProofValidationError("opened proof must not be hard-linked")
        if (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) != (
            opened.st_dev,
            opened.st_ino,
            opened.st_size,
            opened.st_mtime_ns,
            opened.st_ctime_ns,
        ):
            raise ProofValidationError("proof changed while it was opened")
        encoded = bytearray()
        while len(encoded) <= MAX_PROOF_BYTES:
            chunk = os.read(descriptor, min(64 * 1024, MAX_PROOF_BYTES + 1 - len(encoded)))
            if not chunk:
                break
            encoded.extend(chunk)
        if len(encoded) > MAX_PROOF_BYTES:
            raise ProofValidationError("proof exceeded the read bound")
        after = os.fstat(descriptor)
        if len(encoded) != opened.st_size:
            raise ProofValidationError("proof size changed while it was read")
        if (
            opened.st_dev,
            opened.st_ino,
            opened.st_nlink,
            opened.st_size,
            opened.st_mtime_ns,
            opened.st_ctime_ns,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise ProofValidationError("proof changed while it was read")
    finally:
        os.close(descriptor)

    try:
        text = encoded.decode("utf-8", errors="strict")
        return json.loads(
            text,
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=reject_json_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ProofValidationError) as error:
        raise ProofValidationError(f"invalid proof JSON: {error}") from error


def validate_proof(path: Path, schema_name: str) -> None:
    selected = PROOF_SCHEMAS.get(schema_name)
    if selected is None:
        raise ProofValidationError(f"unknown proof schema: {schema_name}")
    payload = load_bounded_json(path)
    if not isinstance(payload, dict):
        raise ProofValidationError("proof root must be an object")
    if not all(isinstance(key, str) and isinstance(value, str) for key, value in payload.items()):
        raise ProofValidationError("proof keys and values must all be strings")
    for value in payload.values():
        if len(value) > MAX_VALUE_CHARACTERS:
            raise ProofValidationError("proof value exceeds the character bound")
        if any(ord(character) < 0x20 and character not in "\t" for character in value):
            raise ProofValidationError("proof value contains a control character")

    required_keys = set(selected.expected) | set(selected.patterns)
    actual_keys = set(payload)
    if actual_keys != required_keys:
        missing = sorted(required_keys - actual_keys)
        extra = sorted(actual_keys - required_keys)
        raise ProofValidationError(f"proof key set mismatch; missing={missing}, extra={extra}")
    for key, expected_value in selected.expected.items():
        if payload[key] != expected_value:
            raise ProofValidationError(f"unexpected value for {key}")
    for key, pattern in selected.patterns.items():
        if pattern.fullmatch(payload[key]) is None:
            raise ProofValidationError(f"value for {key} does not match its schema")

    if schema_name == "text-shortcuts-session-enable":
        if payload["core_engine_available"] != payload["core_runtime_loop_available"]:
            raise ProofValidationError("Text Shortcuts readiness observations disagree")
    if schema_name == "audio-output" and int(payload["output_volume"]) > 150:
        raise ProofValidationError("audio output volume is outside the bounded range")
    if schema_name == "multi-display-apply":
        if payload["stale_serial"] == payload["serial"]:
            raise ProofValidationError("stale display serial equals the current serial")


def validate_run_directory(path: Path, repository: Path, architecture: str) -> str:
    if architecture not in {"aarch64", "x86_64"}:
        raise ProofValidationError("unsupported hardware-gate architecture")
    try:
        repository = repository.resolve(strict=True)
    except OSError as error:
        raise ProofValidationError(f"cannot resolve candidate repository: {error}") from error
    if not repository.is_dir():
        raise ProofValidationError("candidate repository is not a directory")

    if path.is_absolute():
        candidate = path
    else:
        if any(part in {"", ".", ".."} for part in path.parts):
            raise ProofValidationError("run directory has a non-canonical path component")
        candidate = repository / path
    candidate = Path(os.path.abspath(candidate))
    expected_parent = repository / "os" / "screenshots" / "hardware-gate" / architecture
    try:
        relative = candidate.relative_to(expected_parent)
    except ValueError as error:
        raise ProofValidationError("run directory is outside its architecture proof root") from error
    if len(relative.parts) != 1:
        raise ProofValidationError("run directory must be one dated child of its architecture root")
    try:
        parsed_date = date.fromisoformat(relative.name)
    except ValueError as error:
        raise ProofValidationError("run directory name is not a real ISO calendar date") from error
    if parsed_date.isoformat() != relative.name:
        raise ProofValidationError("run directory name is not canonical YYYY-MM-DD")

    current = repository
    for component in candidate.relative_to(repository).parts:
        current /= component
        try:
            metadata = current.lstat()
        except OSError as error:
            raise ProofValidationError(f"cannot stat run-directory component: {error}") from error
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
            raise ProofValidationError("run-directory path contains a symlink or non-directory")
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise ProofValidationError(f"cannot resolve run directory: {error}") from error
    if resolved != candidate:
        raise ProofValidationError("run directory is not canonical")
    return str(candidate.relative_to(repository))


def validate_manifest(
    path: Path,
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    iso_path: str,
    screenshot_run_dir: str,
) -> dict[str, object]:
    if architecture not in {"aarch64", "x86_64"}:
        raise ProofValidationError("unsupported manifest architecture")
    if COMMIT_SHA.fullmatch(candidate_commit) is None:
        raise ProofValidationError("candidate commit is not lowercase 40-hex")
    if IMAGE_REF.fullmatch(image_ref) is None or ".." in image_ref or "//" in image_ref:
        raise ProofValidationError("candidate image reference is not canonical and digest-pinned")
    expected_iso = f"os/iso/output/{architecture}/bootiso/goblins-os-{architecture}.iso"
    if iso_path != expected_iso:
        raise ProofValidationError("verification ISO path is not canonical")
    expected_run_prefix = f"os/screenshots/hardware-gate/{architecture}/"
    if not screenshot_run_dir.startswith(expected_run_prefix):
        raise ProofValidationError("screenshot run directory does not match the architecture")
    run_date = screenshot_run_dir.removeprefix(expected_run_prefix)
    if "/" in run_date:
        raise ProofValidationError("screenshot run directory has nested path components")
    try:
        parsed_date = date.fromisoformat(run_date)
    except ValueError as error:
        raise ProofValidationError("manifest run date is invalid") from error
    if parsed_date.isoformat() != run_date:
        raise ProofValidationError("manifest run date is not canonical")

    payload = load_bounded_json(path)
    if not isinstance(payload, dict):
        raise ProofValidationError("proof manifest root must be an object")
    expected_keys = set(MANIFEST_FIXED_VALUES) | {
        "architecture",
        "candidate_commit",
        "image_ref",
        "iso",
        "iso_sha256",
        "captured_at",
        "screenshot_run_dir",
        "capture_workflow_run",
        "capture_workflow_run_attempt",
        "native_packaging_gate_proof",
        "verification_release_evidence_manifest_sha256",
        "text_shortcuts_live_ibus_runtime_render_screenshot_sha256",
    }
    if set(payload) != expected_keys:
        missing = sorted(expected_keys - set(payload))
        extra = sorted(set(payload) - expected_keys)
        raise ProofValidationError(f"manifest key set mismatch; missing={missing}, extra={extra}")
    for key, value in MANIFEST_FIXED_VALUES.items():
        if payload.get(key) != value:
            raise ProofValidationError(f"unexpected manifest value for {key}")
    expected = {
        "architecture": architecture,
        "candidate_commit": candidate_commit,
        "image_ref": image_ref,
        "iso": iso_path,
        "captured_at": f"{run_date}T00:00:00Z",
        "screenshot_run_dir": screenshot_run_dir,
    }
    for key, value in expected.items():
        if payload.get(key) != value:
            raise ProofValidationError(f"unexpected manifest value for {key}")
    for key in (
        "iso_sha256",
        "verification_release_evidence_manifest_sha256",
        "text_shortcuts_live_ibus_runtime_render_screenshot_sha256",
    ):
        value = payload.get(key)
        if not isinstance(value, str) or HEX_SHA256.fullmatch(value) is None:
            raise ProofValidationError(f"manifest {key} is not lowercase SHA256")

    workflow_run = payload.get("capture_workflow_run")
    workflow_attempt = payload.get("capture_workflow_run_attempt")
    native_proof = payload.get("native_packaging_gate_proof")
    if type(workflow_attempt) is not int:
        raise ProofValidationError("capture workflow attempt must be an integer")
    if architecture == "x86_64":
        if not isinstance(workflow_run, str) or WORKFLOW_RUN.fullmatch(workflow_run) is None:
            raise ProofValidationError("x86_64 manifest lacks a canonical capture workflow run")
        if workflow_attempt < 1 or native_proof != "":
            raise ProofValidationError("x86_64 workflow/native proof fields are inconsistent")
    else:
        expected_native = f"{screenshot_run_dir}/native-packaging-gate.json"
        if workflow_run != "" or workflow_attempt != 0 or native_proof != expected_native:
            raise ProofValidationError("aarch64 workflow/native proof fields are inconsistent")
    return payload


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="goblins-proof-validation-") as temporary:
        root = Path(temporary)
        for selected in PROOF_SCHEMAS.values():
            valid = dict(selected.expected)
            for key in selected.patterns:
                valid[key] = PATTERN_SELF_TEST_VALUES[key]
            valid_path = root / f"{selected.name}.json"
            valid_path.write_text(json.dumps(valid), encoding="utf-8")
            validate_proof(valid_path, selected.name)

        duplicate_path = root / "duplicate.json"
        duplicate_path.write_text('{"name":"a","name":"b"}', encoding="utf-8")
        try:
            validate_proof(duplicate_path, selected.name)
        except ProofValidationError:
            pass
        else:
            raise ProofValidationError("duplicate-key regression was accepted")

        selected = PROOF_SCHEMAS["firewall-live-toggle"]
        injected = dict(selected.expected)
        injected.pop("status")
        injected["diagnostic"] = '\"status\": \"pass\"'
        injected_path = root / "injected.json"
        injected_path.write_text(json.dumps(injected), encoding="utf-8")
        try:
            validate_proof(injected_path, selected.name)
        except ProofValidationError:
            pass
        else:
            raise ProofValidationError("textual-lookalike regression was accepted")

        hard_link_path = root / "hard-linked-proof.json"
        os.link(injected_path, hard_link_path)
        try:
            validate_proof(hard_link_path, selected.name)
        except ProofValidationError:
            pass
        else:
            raise ProofValidationError("hard-linked proof regression was accepted")
        hard_link_path.unlink()

        symbolic_link_path = root / "symlinked-proof.json"
        symbolic_link_path.symlink_to(injected_path)
        try:
            validate_proof(symbolic_link_path, selected.name)
        except ProofValidationError:
            pass
        else:
            raise ProofValidationError("symlinked proof regression was accepted")

        repository = root / "repo"
        run_dir = repository / "os/screenshots/hardware-gate/x86_64/2026-07-21"
        run_dir.mkdir(parents=True)
        relative_run_dir = validate_run_directory(
            Path("os/screenshots/hardware-gate/x86_64/2026-07-21"),
            repository,
            "x86_64",
        )
        manifest = dict(MANIFEST_FIXED_VALUES)
        manifest.update(
            {
                "architecture": "x86_64",
                "candidate_commit": "1" * 40,
                "image_ref": f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
                "iso": "os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso",
                "iso_sha256": "3" * 64,
                "captured_at": "2026-07-21T00:00:00Z",
                "screenshot_run_dir": relative_run_dir,
                "capture_workflow_run": "https://github.com/Joe-Simo/goblins-os/actions/runs/1",
                "capture_workflow_run_attempt": 1,
                "native_packaging_gate_proof": "",
                "verification_release_evidence_manifest_sha256": "4" * 64,
                "text_shortcuts_live_ibus_runtime_render_screenshot_sha256": "5" * 64,
            }
        )
        manifest_path = run_dir / "proof-manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        validate_manifest(
            manifest_path,
            "x86_64",
            "1" * 40,
            f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
            "os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso",
            relative_run_dir,
        )


def main(arguments: list[str]) -> int:
    try:
        if arguments == ["--self-test"]:
            run_self_test()
            print("goblins-proof-validation-self-test: pass")
            return 0
        if len(arguments) == 3 and arguments[0] == "--proof":
            validate_proof(Path(arguments[2]), arguments[1])
            return 0
        if len(arguments) == 4 and arguments[0] == "--run-directory":
            print(validate_run_directory(Path(arguments[1]), Path(arguments[2]), arguments[3]))
            return 0
        if len(arguments) == 7 and arguments[0] == "--manifest":
            validate_manifest(
                Path(arguments[1]),
                arguments[2],
                arguments[3],
                arguments[4],
                arguments[5],
                arguments[6],
            )
            return 0
        raise ProofValidationError(
            "usage: proof_validation.py --proof SCHEMA FILE | "
            "--manifest FILE ARCH COMMIT IMAGE_REF ISO RUN_DIR | "
            "--run-directory DIR REPOSITORY ARCH | --self-test"
        )
    except ProofValidationError as error:
        print(f"proof validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
