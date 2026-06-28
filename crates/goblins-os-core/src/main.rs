mod accelerators;
mod accessibility;
mod ai;
mod app_builder;
mod app_permissions;
mod appearance;
mod audio;
mod auth;
mod bluetooth;
mod boot_lock;
mod codex;
mod displays;
mod fingerprint;
mod firewall;
mod focus;
mod hardware;
mod hotspot;
mod http_error;
mod input;
mod install_targets;
mod installer;
mod keychain;
mod live_captions;
mod migration;
mod model_manager;
mod network;
mod notifications;
mod ocr;
mod openai_key;
mod policy;
mod preview;
mod privacy;
mod readiness;
mod resident;
mod service_catalog;
mod session_gate;
mod settings;
mod shortcuts;
mod sound_recognition;
mod studio;
mod switch_control;
mod system;
mod system_image;
mod text_shortcuts;
mod today;
mod vision;
mod voice;
mod voice_control;
mod window_management;

use std::net::SocketAddr;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    accessibility::{accessibility_status, set_accessibility_preference},
    ai::{
        ai_action_catalog, ai_action_history, ask_file_context, ask_notification_context,
        ask_screen_context, ask_selected_text_context, ask_settings_context, ask_system_status,
        change_safe_setting, open_settings_panel, write_selected_text_context,
    },
    app_builder::{app_builder_catalog, create_app_build, list_apps},
    appearance::{
        appearance_status, set_color_scheme, set_wallpaper_placement, set_wallpaper_shading,
    },
    audio::{
        audio_status, set_audio_default_device, set_audio_mute, set_audio_volume,
        set_sound_preference,
    },
    auth::{
        openai_auth_callback, openai_auth_device_poll, openai_auth_device_start,
        openai_auth_refresh, openai_auth_start, openai_auth_status,
    },
    bluetooth::{bluetooth_status, set_bluetooth_power},
    boot_lock::boot_lock_status,
    codex::{codex_login_start, codex_login_url, codex_status},
    displays::{apply_displays, displays_status},
    fingerprint::fingerprint_status,
    firewall::{firewall_status, set_firewall_enabled},
    hardware::hardware_status,
    input::{
        add_input_source, input_status, set_input_preference, set_input_sources,
        switch_to_next_input_source,
    },
    install_targets::{install_progress_status, install_target_status, prepare_install},
    installer::{complete_installer, installer_readiness},
    migration::{
        migration_copy_plan, migration_estimate, migration_progress, migration_sources,
        migration_start,
    },
    model_manager::{install_local_model, local_model_catalog},
    network::{network_status, set_proxy_mode, wifi_connect, wifi_scan},
    notifications::{notifications_status, set_notification_preference},
    openai_key::{openai_key_status, set_openai_key, set_resident_engine},
    policy::{configure_policy, grant_permission, policy_status},
    preview::{open_preview, preview_status},
    privacy::{privacy_status, set_desktop_privacy, set_privacy},
    readiness::readiness,
    resident::{ai_runtime, ai_runtime_status},
    service_catalog::service_catalog,
    session_gate::{session_gate_status, unlock_session},
    settings::{recovery_status, settings_system},
    shortcuts::{set_modifier_remap, set_shortcut_binding, shortcuts_status},
    studio::{studio_file, studio_session, studio_sessions, studio_turn},
    system::{health, system_services},
    system_image::system_image_status,
    voice::{voice_converse, voice_dictate, voice_status},
    window_management::{set_hot_corner, window_management_status},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "goblins_os_core=info,tower_http=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let port = std::env::var("GOBLINS_OS_CORE_PORT")
        .or_else(|_| std::env::var("OPENAI_OS_CORE_PORT"))
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8787);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/readiness", get(readiness))
        .route("/v1/boot-lock", get(boot_lock_status))
        .route("/v1/ai/actions", get(ai_action_catalog))
        .route("/v1/ai/action-history", get(ai_action_history))
        .route("/v1/ai/safe-setting-change", post(change_safe_setting))
        .route("/v1/ai/open-settings-panel", post(open_settings_panel))
        .route("/v1/ai/system-status", post(ask_system_status))
        .route("/v1/ai/file-context", post(ask_file_context))
        .route("/v1/ai/settings-context", post(ask_settings_context))
        .route(
            "/v1/ai/selected-text-context",
            post(ask_selected_text_context),
        )
        .route(
            "/v1/ai/write-selected-text",
            post(write_selected_text_context),
        )
        .route("/v1/ai/screen-context", post(ask_screen_context))
        .route(
            "/v1/ai/notification-context",
            post(ask_notification_context),
        )
        .route("/v1/settings/system", get(settings_system))
        .route("/v1/system/hardware", get(hardware_status))
        .route("/v1/system/image", get(system_image_status))
        .route("/v1/displays/status", get(displays_status))
        .route("/v1/displays/apply", post(apply_displays))
        .route("/v1/system/services", get(system_services))
        .route("/v1/installer/install-targets", get(install_target_status))
        .route(
            "/v1/installer/install-targets/prepare",
            post(prepare_install),
        )
        .route(
            "/v1/installer/install-targets/progress",
            get(install_progress_status),
        )
        .route("/v1/recovery/status", get(recovery_status))
        .route("/v1/session/gate", get(session_gate_status))
        .route("/v1/session/unlock", post(unlock_session))
        .route("/v1/installer/readiness", get(installer_readiness))
        .route("/v1/installer/complete", post(complete_installer))
        .route("/v1/services", get(service_catalog))
        .route("/v1/local-models", get(local_model_catalog))
        .route("/v1/local-models/install", post(install_local_model))
        .route("/v1/appearance/status", get(appearance_status))
        .route("/v1/appearance/color-scheme", post(set_color_scheme))
        .route(
            "/v1/appearance/wallpaper-placement",
            post(set_wallpaper_placement),
        )
        .route(
            "/v1/appearance/wallpaper-shading",
            post(set_wallpaper_shading),
        )
        .route("/v1/accessibility/status", get(accessibility_status))
        .route(
            "/v1/accessibility/preference",
            post(set_accessibility_preference),
        )
        .route("/v1/network/status", get(network_status))
        .route("/v1/network/wifi/scan", get(wifi_scan))
        .route("/v1/network/wifi/connect", post(wifi_connect))
        .route("/v1/network/proxy/mode", post(set_proxy_mode))
        .route("/v1/notifications/status", get(notifications_status))
        .route(
            "/v1/notifications/preference",
            post(set_notification_preference),
        )
        .route("/v1/bluetooth/status", get(bluetooth_status))
        .route("/v1/bluetooth/power", post(set_bluetooth_power))
        .route("/v1/audio/status", get(audio_status))
        .route("/v1/audio/volume", post(set_audio_volume))
        .route("/v1/audio/mute", post(set_audio_mute))
        .route("/v1/audio/default-device", post(set_audio_default_device))
        .route("/v1/audio/preference", post(set_sound_preference))
        .route("/v1/input/status", get(input_status))
        .route("/v1/input/preference", post(set_input_preference))
        .route("/v1/input/sources", post(set_input_sources))
        .route("/v1/input/source", post(add_input_source))
        .route("/v1/input/switch-next", post(switch_to_next_input_source))
        .route("/v1/privacy/status", get(privacy_status))
        .route("/v1/privacy", post(set_privacy))
        .route("/v1/privacy/desktop", post(set_desktop_privacy))
        .route("/v1/voice/status", get(voice_status))
        .route("/v1/voice/converse", post(voice_converse))
        .route("/v1/voice/dictate", post(voice_dictate))
        .route(
            "/v1/live-captions/status",
            get(live_captions::live_captions_status),
        )
        .route(
            "/v1/captions/status",
            get(live_captions::live_captions_status),
        )
        .route(
            "/v1/captions/stream",
            get(live_captions::live_captions_stream),
        )
        .route(
            "/v1/voice/control/vocabulary",
            get(voice_control::voice_control_vocabulary),
        )
        .route(
            "/v1/voice/control/resolve",
            post(voice_control::resolve_voice_command),
        )
        .route("/v1/voice/control", post(voice_control::voice_control))
        .route("/v1/vision/status", get(vision::vision_status))
        .route("/v1/ai/visual-lookup", post(vision::visual_lookup))
        .route("/v1/today/status", get(today::today_status))
        .route("/v1/today/layout", post(today::set_today_layout))
        .route(
            "/v1/sound-recognition/status",
            get(sound_recognition::sound_recognition_status),
        )
        .route(
            "/v1/sound-recognition/preference",
            post(sound_recognition::set_sound_recognition_preference),
        )
        .route(
            "/v1/sound-recognition/sound-toggle",
            post(sound_recognition::set_sound_toggle),
        )
        .route(
            "/v1/accessibility/switch-control/status",
            get(switch_control::switch_control_status),
        )
        .route(
            "/v1/accessibility/switch-control/preference",
            post(switch_control::set_switch_control_preference),
        )
        .route("/v1/ocr/status", get(ocr::ocr_status))
        .route("/v1/ocr/recognize", post(ocr::ocr_recognize))
        .route("/v1/firewall/status", get(firewall_status))
        .route("/v1/firewall/enabled", post(set_firewall_enabled))
        .route("/v1/fingerprint/status", get(fingerprint_status))
        .route("/v1/preview/status", get(preview_status))
        .route("/v1/preview/open", post(open_preview))
        .route("/v1/focus/status", get(focus::focus_status))
        .route("/v1/focus/activate", post(focus::activate_focus))
        .route("/v1/focus/deactivate", post(focus::deactivate_focus))
        .route("/v1/focus/tick", post(focus::focus_tick))
        .route("/v1/focus/mode", post(focus::set_focus_mode))
        .route("/v1/focus/schedule", post(focus::set_focus_schedule))
        .route("/v1/keychain/status", get(keychain::keychain_status))
        .route(
            "/v1/keychain/collections",
            get(keychain::keychain_collections),
        )
        .route(
            "/v1/text-shortcuts",
            get(text_shortcuts::text_shortcuts_status).post(text_shortcuts::set_text_shortcuts),
        )
        .route(
            "/v1/text-shortcuts/preview",
            get(text_shortcuts::preview_text_shortcut),
        )
        .route(
            "/v1/app-privacy/status",
            get(app_permissions::app_privacy_status),
        )
        .route(
            "/v1/app-privacy/revoke",
            post(app_permissions::revoke_app_permission),
        )
        .route("/v1/hotspot/status", get(hotspot::hotspot_status))
        .route("/v1/hotspot/enabled", post(hotspot::set_hotspot_enabled))
        .route(
            "/v1/window-management/status",
            get(window_management_status),
        )
        .route("/v1/window-management/hot-corner", post(set_hot_corner))
        .route("/v1/shortcuts/status", get(shortcuts::shortcuts_status))
        .route("/v1/keyboard/shortcuts/status", get(shortcuts_status))
        .route("/v1/keyboard/shortcuts/binding", post(set_shortcut_binding))
        .route("/v1/keyboard/modifier-remap", post(set_modifier_remap))
        .route(
            "/v1/migration/capabilities",
            get(migration::migration_capabilities),
        )
        .route("/v1/migration/sources", get(migration_sources))
        .route("/v1/migration/copy-plan", post(migration_copy_plan))
        .route("/v1/migration/estimate", post(migration_estimate))
        .route("/v1/migration/start", post(migration_start))
        .route("/v1/migration/progress", get(migration_progress))
        .route("/v1/studio/turn", post(studio_turn))
        .route("/v1/studio/sessions", get(studio_sessions))
        .route("/v1/studio/session", get(studio_session))
        .route("/v1/studio/file", get(studio_file))
        .route("/v1/codex/status", get(codex_status))
        .route("/v1/codex/login", post(codex_login_start))
        .route("/v1/codex/login/url", get(codex_login_url))
        .route("/v1/models/openai-key", get(openai_key_status))
        .route("/v1/models/openai-key", post(set_openai_key))
        .route("/v1/models/engine", post(set_resident_engine))
        .route("/v1/policy/status", get(policy_status))
        .route("/v1/policy/configure", post(configure_policy))
        .route("/v1/policy/permissions/grant", post(grant_permission))
        .route("/v1/apps/build-catalog", get(app_builder_catalog))
        .route("/v1/apps/builds", post(create_app_build))
        .route("/v1/apps", get(list_apps))
        .route("/v1/auth/openai/status", get(openai_auth_status))
        .route("/v1/auth/openai/start", get(openai_auth_start))
        .route("/v1/auth/openai/callback", get(openai_auth_callback))
        .route(
            "/v1/auth/openai/device/start",
            post(openai_auth_device_start),
        )
        .route("/v1/auth/openai/device/poll", post(openai_auth_device_poll))
        .route("/v1/auth/openai/refresh", post(openai_auth_refresh))
        .route("/v1/ai/runtime/status", get(ai_runtime_status))
        .route("/v1/ai/runtime", post(ai_runtime))
        // Compatibility for images and clients built before the Goblins AI
        // runtime route became the product-facing API surface.
        .route("/v1/codex/resident/status", get(ai_runtime_status))
        .route("/v1/codex/resident", post(ai_runtime))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("Goblins OS core listening on http://{address}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
