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
import hashlib
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
GITHUB_RUN_URL = re.compile(r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*")
QEMU_VERSION_RE = re.compile(r"QEMU emulator version [^\x00-\x1f\x7f]{1,192}")

CAPTURE_ENVIRONMENT_FIXED_VALUES: Final[dict[str, str]] = {
    "host_os": "Darwin",
    "host_architecture": "arm64",
    "accelerator": "hvf",
    "qemu_binary": "qemu-system-aarch64",
    "qemu_machine": "virt,accel=hvf,gic-version=max",
    "qemu_cpu": "host",
}

MANIFEST_FIXED_VALUES: Final[dict[str, object]] = {
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
    "accessibility_adaptivity_proof": "accessibility-adaptivity-proof.json",
    "capture_canvas_width": 5120,
    "capture_canvas_height": 2880,
    "capture_canvas_normalization": "centered padding without resampling",
    "capture_method": (
        "display-backed qemu VM, software GPU/audio substrate "
        "(lavapipe/gamescope/pipewire), honestly labeled"
    ),
}

ACCESSIBILITY_SCREENSHOT_BINDINGS: Final[dict[str, tuple[str, str]]] = {
    "text_scale_screenshot": (
        "text_scale_screenshot_sha256",
        "33-accessibility-text-scaling.png",
    ),
    "high_contrast_screenshot": (
        "high_contrast_screenshot_sha256",
        "34-accessibility-high-contrast.png",
    ),
    "reduce_transparency_screenshot": (
        "reduce_transparency_screenshot_sha256",
        "35-accessibility-reduced-transparency.png",
    ),
    "reduce_motion_screenshot": (
        "reduce_motion_screenshot_sha256",
        "36-accessibility-reduced-motion.png",
    ),
    "locale_expansion_screenshot": (
        "locale_expansion_screenshot_sha256",
        "37-accessibility-localization-expansion.png",
    ),
    "orca_atspi_screenshot": (
        "orca_atspi_screenshot_sha256",
        "38-accessibility-orca-atspi.png",
    ),
    "keyboard_focus_screenshot": (
        "keyboard_focus_screenshot_sha256",
        "39-accessibility-keyboard-focus.png",
    ),
    "window_resize_screenshot": (
        "window_resize_screenshot_sha256",
        "40-accessibility-window-resize.png",
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
    "accessibility-adaptivity": schema(
        "accessibility-adaptivity",
        exact(
            status="pass",
            architecture="aarch64",
            status_route="/v1/accessibility/status",
            preference_route="/v1/accessibility/preference",
            gsettings_available="true",
            interface_schema_available="true",
            visual_schema_available="true",
            reduce_transparency_schema_available="true",
            text_scale_schema="org.gnome.desktop.interface",
            text_scale_key="text-scaling-factor",
            text_scale_target="1.25",
            text_scale_http="200",
            text_scale_ok="true",
            text_scale_readback="1.25",
            text_scale_screenshot="33-accessibility-text-scaling.png",
            high_contrast_schema="org.gnome.desktop.a11y.interface",
            high_contrast_key="high-contrast",
            high_contrast_http="200",
            high_contrast_ok="true",
            high_contrast_readback="true",
            high_contrast_screenshot="34-accessibility-high-contrast.png",
            reduce_transparency_schema="org.goblins.os.a11y.visual",
            reduce_transparency_key="reduce-transparency",
            reduce_transparency_http="200",
            reduce_transparency_ok="true",
            reduce_transparency_readback="true",
            reduce_transparency_screenshot="35-accessibility-reduced-transparency.png",
            reduce_motion_schema="org.gnome.desktop.interface",
            reduce_motion_key="enable-animations",
            reduce_motion_http="200",
            reduce_motion_ok="true",
            reduce_motion_readback="true",
            reduce_motion_screenshot="36-accessibility-reduced-motion.png",
            locale_expansion_surface="goblins-os-settings-language-region",
            locale_expansion_requested_locale="de_DE.UTF-8",
            locale_expansion_language_request="de",
            locale_expansion_runtime="Goblins-Settings-AT-SPI",
            locale_expansion_process_environment="true",
            locale_expansion_active_time_locale="true",
            locale_expansion_active_numeric_locale="true",
            locale_expansion_translation_claimed="false",
            locale_expansion_goblins_copy_language="English",
            locale_expansion_regional_content_present="true",
            locale_expansion_fact_count="5",
            locale_expansion_date_format_changed="true",
            locale_expansion_number_format_changed="true",
            locale_expansion_clipped_visible_node_count="0",
            locale_expansion_layout_bounds_checked="true",
            locale_expansion_screenshot="37-accessibility-localization-expansion.png",
            screen_reader_schema="org.gnome.desktop.a11y.applications",
            screen_reader_key="screen-reader-enabled",
            screen_reader_http="200",
            screen_reader_ok="true",
            screen_reader_readback="true",
            orca_process_active="true",
            atspi_bus="org.a11y.Bus",
            atspi_bus_ready="true",
            atspi_settings_window_present="true",
            orca_atspi_screenshot="38-accessibility-orca-atspi.png",
            keyboard_input_driver="qmp-keyboard",
            keyboard_focus_key="Tab",
            keyboard_focused="true",
            keyboard_focus_screenshot="39-accessibility-keyboard-focus.png",
            resize_action="AT-SPI-Zoom",
            resize_action_invoked="true",
            window_resize_observed="true",
            framebuffer_width="5120",
            framebuffer_height="2880",
            window_resize_screenshot="40-accessibility-window-resize.png",
            uniform_framebuffer="true",
            screenshot_capture_ack="true",
            roundtrip_restored="true",
        ),
        {
            "text_scale_screenshot_sha256": HEX_SHA256,
            "high_contrast_screenshot_sha256": HEX_SHA256,
            "reduce_transparency_screenshot_sha256": HEX_SHA256,
            "reduce_motion_screenshot_sha256": HEX_SHA256,
            "locale_expansion_accessible_name": SAFE_TEXT,
            "locale_expansion_changed_name_count": POSITIVE_INTEGER,
            "locale_expansion_longer_name_count": POSITIVE_INTEGER,
            "locale_expansion_visible_named_node_count": POSITIVE_INTEGER,
            "locale_expansion_screenshot_sha256": HEX_SHA256,
            "atspi_node_count": POSITIVE_INTEGER,
            "orca_atspi_screenshot_sha256": HEX_SHA256,
            "keyboard_focus_role": SAFE_TEXT,
            "keyboard_focus_name": SAFE_TEXT,
            "keyboard_focus_screenshot_sha256": HEX_SHA256,
            "window_maximized_width": POSITIVE_INTEGER,
            "window_maximized_height": POSITIVE_INTEGER,
            "window_resized_width": POSITIVE_INTEGER,
            "window_resized_height": POSITIVE_INTEGER,
            "window_resize_screenshot_sha256": HEX_SHA256,
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
    "text_scale_screenshot_sha256": "1" * 64,
    "high_contrast_screenshot_sha256": "2" * 64,
    "reduce_transparency_screenshot_sha256": "3" * 64,
    "reduce_motion_screenshot_sha256": "4" * 64,
    "locale_expansion_accessible_name": "Regional format: German (Germany)",
    "locale_expansion_changed_name_count": "4",
    "locale_expansion_longer_name_count": "2",
    "locale_expansion_visible_named_node_count": "24",
    "locale_expansion_screenshot_sha256": "5" * 64,
    "atspi_node_count": "64",
    "orca_atspi_screenshot_sha256": "6" * 64,
    "keyboard_focus_role": "push button",
    "keyboard_focus_name": "Text size",
    "keyboard_focus_screenshot_sha256": "7" * 64,
    "window_maximized_width": "1920",
    "window_maximized_height": "1080",
    "window_resized_width": "1200",
    "window_resized_height": "800",
    "window_resize_screenshot_sha256": "8" * 64,
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
    if schema_name == "accessibility-adaptivity":
        if int(payload["atspi_node_count"]) < 8:
            raise ProofValidationError("AT-SPI accessibility tree is implausibly small")
        if int(payload["window_maximized_width"]) <= int(payload["window_resized_width"]):
            raise ProofValidationError("window width did not shrink after the AT-SPI Zoom action")
        if int(payload["window_maximized_height"]) <= int(payload["window_resized_height"]):
            raise ProofValidationError("window height did not shrink after the AT-SPI Zoom action")
        hashes = [payload[sha_field] for sha_field, _ in ACCESSIBILITY_SCREENSHOT_BINDINGS.values()]
        if len(set(hashes)) != len(hashes):
            raise ProofValidationError("accessibility proof reuses a screenshot hash")


def hash_stable_regular_file(path: Path, maximum_bytes: int = 64 * 1024 * 1024) -> str:
    try:
        before = path.lstat()
    except OSError as error:
        raise ProofValidationError(f"cannot stat screenshot: {error}") from error
    if (
        not stat.S_ISREG(before.st_mode)
        or stat.S_ISLNK(before.st_mode)
        or before.st_nlink != 1
        or before.st_size <= 0
        or before.st_size > maximum_bytes
    ):
        raise ProofValidationError("screenshot must be a bounded single-link regular file")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ProofValidationError(f"cannot securely open screenshot: {error}") from error
    digest = hashlib.sha256()
    try:
        opened = os.fstat(descriptor)
        stable = ("st_dev", "st_ino", "st_mode", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")
        if any(getattr(before, field) != getattr(opened, field) for field in stable):
            raise ProofValidationError("screenshot changed before it was opened")
        remaining = opened.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise ProofValidationError("screenshot was truncated while hashing")
            digest.update(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise ProofValidationError("screenshot grew while hashing")
        after = os.fstat(descriptor)
        if any(getattr(opened, field) != getattr(after, field) for field in stable):
            raise ProofValidationError("screenshot changed while hashing")
    finally:
        os.close(descriptor)
    return digest.hexdigest()


def validate_proof_screenshots(schema_name: str, proof_path: Path, run_dir: Path) -> None:
    if schema_name != "accessibility-adaptivity":
        raise ProofValidationError("screenshot binding is not defined for this proof schema")
    try:
        canonical_run_dir = run_dir.resolve(strict=True)
        canonical_proof = proof_path.resolve(strict=True)
    except OSError as error:
        raise ProofValidationError(f"cannot resolve screenshot proof path: {error}") from error
    if canonical_proof.parent != canonical_run_dir:
        raise ProofValidationError("proof and screenshots must share the exact dated run directory")
    if canonical_proof.name != "accessibility-adaptivity-proof.json":
        raise ProofValidationError("unexpected accessibility proof filename")
    validate_proof(canonical_proof, schema_name)
    payload = load_bounded_json(canonical_proof)
    if not isinstance(payload, dict):
        raise ProofValidationError("accessibility proof root must be an object")
    observed_hashes: list[str] = []
    for name_field, (sha_field, expected_name) in ACCESSIBILITY_SCREENSHOT_BINDINGS.items():
        if payload.get(name_field) != expected_name:
            raise ProofValidationError(f"unexpected screenshot binding for {name_field}")
        observed = hash_stable_regular_file(canonical_run_dir / expected_name)
        if payload.get(sha_field) != observed:
            raise ProofValidationError(f"screenshot SHA256 mismatch for {expected_name}")
        observed_hashes.append(observed)
    if len(set(observed_hashes)) != len(observed_hashes):
        raise ProofValidationError("accessibility screenshots are not byte-distinct")


def validate_run_directory(path: Path, repository: Path, architecture: str) -> str:
    if architecture != "aarch64":
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


def validate_capture_environment(value: object) -> dict[str, str]:
    expected_keys = set(CAPTURE_ENVIRONMENT_FIXED_VALUES) | {
        "qemu_binary_sha256",
        "qemu_version",
    }
    if type(value) is not dict or set(value) != expected_keys:
        raise ProofValidationError(
            "capture environment must contain the exact Darwin/arm64/HVF QEMU fields"
        )
    if not all(type(key) is str and type(item) is str for key, item in value.items()):
        raise ProofValidationError("capture environment fields must be strings")
    for key, expected in CAPTURE_ENVIRONMENT_FIXED_VALUES.items():
        if value[key] != expected:
            raise ProofValidationError(f"unexpected capture environment value for {key}")
    if HEX_SHA256.fullmatch(value["qemu_binary_sha256"]) is None:
        raise ProofValidationError("capture QEMU executable SHA256 is invalid")
    if QEMU_VERSION_RE.fullmatch(value["qemu_version"]) is None:
        raise ProofValidationError("capture QEMU version is invalid")
    return value


def validate_manifest(
    path: Path,
    architecture: str,
    candidate_commit: str,
    image_ref: str,
    iso_path: str,
    screenshot_run_dir: str,
) -> dict[str, object]:
    if architecture != "aarch64":
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
        "capture_environment",
        "image_ref",
        "iso",
        "iso_sha256",
        "captured_at",
        "screenshot_run_dir",
        "capture_workflow_run",
        "capture_workflow_run_attempt",
        "native_packaging_gate_proof",
        "native_packaging_gate_run",
        "native_packaging_gate_run_attempt",
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
    validate_capture_environment(payload.get("capture_environment"))
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
    native_run = payload.get("native_packaging_gate_run")
    native_attempt = payload.get("native_packaging_gate_run_attempt")
    if type(workflow_attempt) is not int:
        raise ProofValidationError("capture workflow attempt must be an integer")
    expected_native = f"{screenshot_run_dir}/native-packaging-gate.json"
    if (
        workflow_run != ""
        or workflow_attempt != 0
        or native_proof != expected_native
        or not isinstance(native_run, str)
        or GITHUB_RUN_URL.fullmatch(native_run) is None
        or type(native_attempt) is not int
        or native_attempt < 1
    ):
        raise ProofValidationError("aarch64 workflow/native proof fields are inconsistent")

    native_path = path.parent / "native-packaging-gate.json"
    native_payload = load_bounded_json(native_path)
    if not isinstance(native_payload, dict):
        raise ProofValidationError("native packaging gate root must be an object")
    native_expected_keys = {
        "schema",
        "architecture",
        "candidate_commit",
        "image_ref",
        "image_digest_pinned",
        "source_verifier",
        "installed_root_verifier",
        "services_selftest",
        "verification_iso_sha256",
        "iso_manifest_sha256",
        "bib_manifest_sha256",
        "release_evidence_manifest_sha256",
        "runner_os",
        "runner_architecture",
        "native_runner",
        "source_repository",
        "workflow_run",
        "workflow_run_attempt",
    }
    if set(native_payload) != native_expected_keys:
        raise ProofValidationError("native packaging gate key set is not exact")
    native_expected = {
        "schema": "goblins-os-native-packaging-gate-v1",
        "architecture": "aarch64",
        "candidate_commit": candidate_commit,
        "image_ref": image_ref,
        "image_digest_pinned": True,
        "source_verifier": "pass",
        "installed_root_verifier": "pass",
        "services_selftest": "pass",
        "verification_iso_sha256": payload["iso_sha256"],
        "iso_manifest_sha256": hash_stable_regular_file(
            path.parent / "verification-iso-manifest.json", MAX_PROOF_BYTES
        ),
        "bib_manifest_sha256": hash_stable_regular_file(
            path.parent / "verification-bib-manifest.json", MAX_PROOF_BYTES
        ),
        "release_evidence_manifest_sha256": hash_stable_regular_file(
            path.parent / "verification-release-evidence-manifest.json", MAX_PROOF_BYTES
        ),
        "runner_os": "Linux",
        "runner_architecture": "aarch64",
        "native_runner": True,
        "source_repository": "https://github.com/Joe-Simo/goblins-os",
        "workflow_run": native_run,
        "workflow_run_attempt": native_attempt,
    }
    if any(native_payload.get(key) != value for key, value in native_expected.items()):
        raise ProofValidationError("native packaging gate does not bind the exact capture inputs")
    if (
        native_payload["release_evidence_manifest_sha256"]
        != payload["verification_release_evidence_manifest_sha256"]
    ):
        raise ProofValidationError("native packaging gate release evidence hash disagrees")
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
        run_dir = repository / "os/screenshots/hardware-gate/aarch64/2026-07-21"
        run_dir.mkdir(parents=True)
        relative_run_dir = validate_run_directory(
            Path("os/screenshots/hardware-gate/aarch64/2026-07-21"),
            repository,
            "aarch64",
        )
        verification_files = {
            "verification-iso-manifest.json": b'{"kind":"iso"}\n',
            "verification-bib-manifest.json": b'{"kind":"bib"}\n',
            "verification-release-evidence-manifest.json": b'{"kind":"evidence"}\n',
        }
        verification_hashes = {}
        for name, data in verification_files.items():
            (run_dir / name).write_bytes(data)
            verification_hashes[name] = hashlib.sha256(data).hexdigest()
        native_run = "https://github.com/Joe-Simo/goblins-os/actions/runs/123456789"
        native_attempt = 2
        native_gate = {
            "schema": "goblins-os-native-packaging-gate-v1",
            "architecture": "aarch64",
            "candidate_commit": "1" * 40,
            "image_ref": f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
            "image_digest_pinned": True,
            "source_verifier": "pass",
            "installed_root_verifier": "pass",
            "services_selftest": "pass",
            "verification_iso_sha256": "3" * 64,
            "iso_manifest_sha256": verification_hashes["verification-iso-manifest.json"],
            "bib_manifest_sha256": verification_hashes["verification-bib-manifest.json"],
            "release_evidence_manifest_sha256": verification_hashes[
                "verification-release-evidence-manifest.json"
            ],
            "runner_os": "Linux",
            "runner_architecture": "aarch64",
            "native_runner": True,
            "source_repository": "https://github.com/Joe-Simo/goblins-os",
            "workflow_run": native_run,
            "workflow_run_attempt": native_attempt,
        }
        native_gate_path = run_dir / "native-packaging-gate.json"
        native_gate_path.write_text(json.dumps(native_gate), encoding="utf-8")
        manifest = dict(MANIFEST_FIXED_VALUES)
        manifest.update(
            {
                "architecture": "aarch64",
                "candidate_commit": "1" * 40,
                "capture_environment": {
                    **CAPTURE_ENVIRONMENT_FIXED_VALUES,
                    "qemu_binary_sha256": "6" * 64,
                    "qemu_version": "QEMU emulator version 10.0.3",
                },
                "image_ref": f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
                "iso": "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
                "iso_sha256": "3" * 64,
                "captured_at": "2026-07-21T00:00:00Z",
                "screenshot_run_dir": relative_run_dir,
                "capture_workflow_run": "",
                "capture_workflow_run_attempt": 0,
                "native_packaging_gate_proof": f"{relative_run_dir}/native-packaging-gate.json",
                "native_packaging_gate_run": native_run,
                "native_packaging_gate_run_attempt": native_attempt,
                "verification_release_evidence_manifest_sha256": verification_hashes[
                    "verification-release-evidence-manifest.json"
                ],
                "text_shortcuts_live_ibus_runtime_render_screenshot_sha256": "5" * 64,
            }
        )
        manifest_path = run_dir / "proof-manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        validate_manifest(
            manifest_path,
            "aarch64",
            "1" * 40,
            f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
            "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
            relative_run_dir,
        )
        for field, invalid in (
            ("native_packaging_gate_run", "https://github.com/Joe-Simo/goblins-os/actions/runs/999"),
            ("native_packaging_gate_run_attempt", native_attempt + 1),
        ):
            invalid_manifest = json.loads(json.dumps(manifest))
            invalid_manifest[field] = invalid
            manifest_path.write_text(json.dumps(invalid_manifest), encoding="utf-8")
            try:
                validate_manifest(
                    manifest_path,
                    "aarch64",
                    "1" * 40,
                    f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
                    "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
                    relative_run_dir,
                )
            except ProofValidationError:
                pass
            else:
                raise ProofValidationError(f"mismatched manifest {field} was accepted")
        for field, invalid in (
            ("workflow_run", "https://github.com/Joe-Simo/goblins-os/actions/runs/999"),
            ("workflow_run_attempt", native_attempt + 1),
            ("runner_architecture", "x86_64"),
        ):
            invalid_gate = json.loads(json.dumps(native_gate))
            invalid_gate[field] = invalid
            native_gate_path.write_text(json.dumps(invalid_gate), encoding="utf-8")
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            try:
                validate_manifest(
                    manifest_path,
                    "aarch64",
                    "1" * 40,
                    f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
                    "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
                    relative_run_dir,
                )
            except ProofValidationError:
                pass
            else:
                raise ProofValidationError(f"mismatched native gate {field} was accepted")
        native_gate_path.write_text(json.dumps(native_gate), encoding="utf-8")
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
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
            invalid_manifest = json.loads(json.dumps(manifest))
            invalid_manifest["capture_environment"][field] = invalid
            manifest_path.write_text(json.dumps(invalid_manifest), encoding="utf-8")
            try:
                validate_manifest(
                    manifest_path,
                    "aarch64",
                    "1" * 40,
                    f"ghcr.io/joe-simo/goblins-os@sha256:{'2' * 64}",
                    "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
                    relative_run_dir,
                )
            except ProofValidationError:
                pass
            else:
                raise ProofValidationError(
                    f"invalid capture environment field {field!r} was accepted"
                )
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        for unsupported in ("x86_64", "amd64", "arm64", "riscv64"):
            try:
                validate_run_directory(Path(relative_run_dir), repository, unsupported)
            except ProofValidationError:
                pass
            else:
                raise ProofValidationError(
                    f"non-canonical architecture {unsupported!r} was accepted"
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
        if len(arguments) == 4 and arguments[0] == "--proof-screenshots":
            validate_proof_screenshots(arguments[1], Path(arguments[2]), Path(arguments[3]))
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
            "--proof-screenshots SCHEMA PROOF RUN_DIR | "
            "--manifest FILE ARCH COMMIT IMAGE_REF ISO RUN_DIR | "
            "--run-directory DIR REPOSITORY ARCH | --self-test"
        )
    except ProofValidationError as error:
        print(f"proof validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
