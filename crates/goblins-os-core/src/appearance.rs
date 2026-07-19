//! Appearance preferences for native Goblins OS surfaces.
//!
//! The desktop color scheme is the standard GNOME/Freedesktop preference used
//! by GTK apps. Settings talks to this core route instead of writing arbitrary
//! GSettings keys from the GUI.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_session_command_output, probe_timeout};

const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const COLOR_SCHEME_KEY: &str = "color-scheme";
const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";
const PICTURE_OPTIONS_KEY: &str = "picture-options";
const PRIMARY_COLOR_KEY: &str = "primary-color";
const SECONDARY_COLOR_KEY: &str = "secondary-color";
const COLOR_SHADING_TYPE_KEY: &str = "color-shading-type";

#[derive(Serialize)]
pub struct AppearanceStatus {
    source: &'static str,
    gsettings_available: bool,
    color_scheme_available: bool,
    color_scheme: String,
    theme: &'static str,
    wallpaper: WallpaperStatus,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetColorSchemeRequest {
    scheme: String,
}

#[derive(Serialize)]
pub struct AppearanceOutcome {
    ok: bool,
    color_scheme: String,
    theme: &'static str,
    text: String,
}

#[derive(Serialize)]
pub struct WallpaperStatus {
    gsettings_available: bool,
    schema_available: bool,
    picture_uri: Option<String>,
    picture_uri_dark: Option<String>,
    picture_options_available: bool,
    picture_options: String,
    primary_color: Option<String>,
    secondary_color: Option<String>,
    color_shading_type_available: bool,
    color_shading_type: String,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetWallpaperPlacementRequest {
    placement: String,
}

#[derive(Serialize)]
pub struct WallpaperPlacementOutcome {
    ok: bool,
    placement: String,
    text: String,
}

#[derive(Deserialize)]
pub struct SetWallpaperShadingRequest {
    shading: String,
}

#[derive(Serialize)]
pub struct WallpaperShadingOutcome {
    ok: bool,
    shading: String,
    text: String,
}

enum GSettingsError {
    Missing,
    Failed(String),
}

pub async fn appearance_status() -> Json<AppearanceStatus> {
    Json(build_appearance_status())
}

pub async fn set_color_scheme(
    Json(request): Json<SetColorSchemeRequest>,
) -> (StatusCode, Json<AppearanceOutcome>) {
    set_color_scheme_outcome(&request.scheme)
}

pub(crate) fn apply_ai_color_scheme(scheme: &str) -> (StatusCode, String) {
    let (status, Json(outcome)) = set_color_scheme_outcome(scheme);
    (status, outcome.text)
}

pub async fn set_wallpaper_placement(
    Json(request): Json<SetWallpaperPlacementRequest>,
) -> (StatusCode, Json<WallpaperPlacementOutcome>) {
    set_wallpaper_placement_outcome(&request.placement)
}

pub async fn set_wallpaper_shading(
    Json(request): Json<SetWallpaperShadingRequest>,
) -> (StatusCode, Json<WallpaperShadingOutcome>) {
    set_wallpaper_shading_outcome(&request.shading)
}

fn build_appearance_status() -> AppearanceStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let color_scheme_available = gsettings_available && color_scheme_key_available();
    let color_scheme = if color_scheme_available {
        gsettings(&["get", INTERFACE_SCHEMA, COLOR_SCHEME_KEY])
            .ok()
            .and_then(|value| parse_gsettings_string(&value))
            .map(|value| normalize_color_scheme(&value).to_string())
            .unwrap_or_else(|| "default".to_string())
    } else {
        "default".to_string()
    };

    AppearanceStatus {
        source: "goblins-os-core",
        gsettings_available,
        color_scheme_available,
        theme: color_scheme_theme(&color_scheme),
        wallpaper: build_wallpaper_status(gsettings_available),
        detail: appearance_detail(gsettings_available, color_scheme_available, &color_scheme),
        color_scheme,
    }
}

fn set_color_scheme_outcome(scheme: &str) -> (StatusCode, Json<AppearanceOutcome>) {
    let normalized = normalize_color_scheme(scheme);
    if normalized == "invalid" {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppearanceOutcome {
                ok: false,
                color_scheme: "default".to_string(),
                theme: "auto",
                text: "Appearance expects Light, Dark, or Auto.".to_string(),
            }),
        );
    }

    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppearanceOutcome {
                ok: false,
                color_scheme: normalized.to_string(),
                theme: color_scheme_theme(normalized),
                text: "Desktop preferences are not ready, so appearance cannot be changed in this session.".to_string(),
            }),
        );
    }

    if !color_scheme_key_available() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppearanceOutcome {
                ok: false,
                color_scheme: normalized.to_string(),
                theme: color_scheme_theme(normalized),
                text: "The standard color-scheme preference is not supported in this session."
                    .to_string(),
            }),
        );
    }

    match gsettings(&["set", INTERFACE_SCHEMA, COLOR_SCHEME_KEY, normalized]) {
        Ok(_) => (
            StatusCode::OK,
            Json(AppearanceOutcome {
                ok: true,
                color_scheme: normalized.to_string(),
                theme: color_scheme_theme(normalized),
                text: color_scheme_success_detail(normalized).to_string(),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppearanceOutcome {
                ok: false,
                color_scheme: normalized.to_string(),
                theme: color_scheme_theme(normalized),
                text: "Desktop preferences are not ready, so appearance cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(AppearanceOutcome {
                ok: false,
                color_scheme: normalized.to_string(),
                theme: color_scheme_theme(normalized),
                text: if detail.is_empty() {
                    "The desktop session could not save the color scheme.".to_string()
                } else {
                    format!("The desktop session could not save the color scheme: {detail}")
                },
            }),
        ),
    }
}

fn build_wallpaper_status(gsettings_available: bool) -> WallpaperStatus {
    let schema_available = gsettings_available && schema_available(BACKGROUND_SCHEMA);
    let picture_options_available =
        schema_available && key_available(BACKGROUND_SCHEMA, PICTURE_OPTIONS_KEY);
    let color_shading_type_available =
        schema_available && key_available(BACKGROUND_SCHEMA, COLOR_SHADING_TYPE_KEY);
    let picture_options = if picture_options_available {
        gsettings(&["get", BACKGROUND_SCHEMA, PICTURE_OPTIONS_KEY])
            .ok()
            .and_then(|value| parse_gsettings_string(&value))
            .map(|value| normalize_wallpaper_placement(&value).to_string())
            .unwrap_or_else(|| "zoom".to_string())
    } else {
        "zoom".to_string()
    };
    let color_shading_type = if color_shading_type_available {
        gsettings(&["get", BACKGROUND_SCHEMA, COLOR_SHADING_TYPE_KEY])
            .ok()
            .and_then(|value| parse_gsettings_string(&value))
            .map(|value| normalize_wallpaper_shading(&value).to_string())
            .unwrap_or_else(|| "solid".to_string())
    } else {
        "solid".to_string()
    };

    WallpaperStatus {
        gsettings_available,
        schema_available,
        picture_uri: setting_string(BACKGROUND_SCHEMA, "picture-uri"),
        picture_uri_dark: setting_string(BACKGROUND_SCHEMA, "picture-uri-dark"),
        picture_options_available,
        primary_color: setting_string(BACKGROUND_SCHEMA, PRIMARY_COLOR_KEY),
        secondary_color: setting_string(BACKGROUND_SCHEMA, SECONDARY_COLOR_KEY),
        color_shading_type_available,
        color_shading_type,
        detail: wallpaper_detail(
            gsettings_available,
            schema_available,
            picture_options_available,
        ),
        picture_options,
    }
}

fn set_wallpaper_placement_outcome(
    placement: &str,
) -> (StatusCode, Json<WallpaperPlacementOutcome>) {
    let normalized = normalize_wallpaper_placement(placement);
    if normalized == "invalid" {
        return wallpaper_placement_outcome(
            StatusCode::BAD_REQUEST,
            "zoom",
            "Wallpaper placement expects Fill, Fit, Center, Stretch, Tile, Span, or None.",
        );
    }

    if gsettings(&["list-schemas"]).is_err() {
        return wallpaper_placement_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so wallpaper placement cannot be changed in this session.",
        );
    }

    if !schema_available(BACKGROUND_SCHEMA)
        || !key_available(BACKGROUND_SCHEMA, PICTURE_OPTIONS_KEY)
    {
        return wallpaper_placement_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "The standard wallpaper placement preference is not supported in this session.",
        );
    }

    match gsettings(&["set", BACKGROUND_SCHEMA, PICTURE_OPTIONS_KEY, normalized]) {
        Ok(_) => wallpaper_placement_outcome(
            StatusCode::OK,
            normalized,
            wallpaper_placement_detail(normalized),
        ),
        Err(GSettingsError::Missing) => wallpaper_placement_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so wallpaper placement cannot be changed in this session.",
        ),
        Err(GSettingsError::Failed(detail)) => wallpaper_placement_outcome(
            StatusCode::BAD_GATEWAY,
            normalized,
            &if detail.is_empty() {
                "The desktop session could not save wallpaper placement.".to_string()
            } else {
                format!("The desktop session could not save wallpaper placement: {detail}")
            },
        ),
    }
}

fn set_wallpaper_shading_outcome(shading: &str) -> (StatusCode, Json<WallpaperShadingOutcome>) {
    let normalized = normalize_wallpaper_shading(shading);
    if normalized == "invalid" {
        return wallpaper_shading_outcome(
            StatusCode::BAD_REQUEST,
            "solid",
            "Wallpaper color shading expects Solid, Horizontal, or Vertical.",
        );
    }

    if gsettings(&["list-schemas"]).is_err() {
        return wallpaper_shading_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so wallpaper color shading cannot be changed in this session.",
        );
    }

    if !schema_available(BACKGROUND_SCHEMA)
        || !key_available(BACKGROUND_SCHEMA, COLOR_SHADING_TYPE_KEY)
    {
        return wallpaper_shading_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "The standard wallpaper color-shading preference is not supported in this session.",
        );
    }

    match gsettings(&["set", BACKGROUND_SCHEMA, COLOR_SHADING_TYPE_KEY, normalized]) {
        Ok(_) => wallpaper_shading_outcome(
            StatusCode::OK,
            normalized,
            wallpaper_shading_detail(normalized),
        ),
        Err(GSettingsError::Missing) => wallpaper_shading_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so wallpaper color shading cannot be changed in this session.",
        ),
        Err(GSettingsError::Failed(detail)) => wallpaper_shading_outcome(
            StatusCode::BAD_GATEWAY,
            normalized,
            &if detail.is_empty() {
                "The desktop session could not save wallpaper color shading.".to_string()
            } else {
                format!("The desktop session could not save wallpaper color shading: {detail}")
            },
        ),
    }
}

fn wallpaper_placement_outcome(
    status: StatusCode,
    placement: &str,
    text: &str,
) -> (StatusCode, Json<WallpaperPlacementOutcome>) {
    (
        status,
        Json(WallpaperPlacementOutcome {
            ok: status == StatusCode::OK,
            placement: placement.to_string(),
            text: text.to_string(),
        }),
    )
}

fn wallpaper_shading_outcome(
    status: StatusCode,
    shading: &str,
    text: &str,
) -> (StatusCode, Json<WallpaperShadingOutcome>) {
    (
        status,
        Json(WallpaperShadingOutcome {
            ok: status == StatusCode::OK,
            shading: shading.to_string(),
            text: text.to_string(),
        }),
    )
}

fn color_scheme_key_available() -> bool {
    key_available(INTERFACE_SCHEMA, COLOR_SCHEME_KEY)
}

fn setting_string(schema: &str, key: &str) -> Option<String> {
    key_available(schema, key)
        .then(|| gsettings(&["get", schema, key]).ok())
        .flatten()
        .and_then(|value| parse_gsettings_string(&value))
}

fn schema_available(schema: &str) -> bool {
    gsettings(&["list-schemas"])
        .map(|stdout| stdout.lines().any(|line| line.trim() == schema))
        .unwrap_or(false)
}

fn key_available(schema: &str, key: &str) -> bool {
    gsettings(&["list-keys", schema])
        .map(|stdout| stdout.lines().any(|line| line.trim() == key))
        .unwrap_or(false)
}

fn parse_gsettings_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if let Some(stripped) = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return Some(stripped.to_string());
    }
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn normalize_color_scheme(value: &str) -> &'static str {
    match value.trim() {
        "light" | "prefer-light" => "prefer-light",
        "dark" | "prefer-dark" => "prefer-dark",
        "auto" | "default" => "default",
        _ => "invalid",
    }
}

fn color_scheme_theme(color_scheme: &str) -> &'static str {
    match color_scheme {
        "prefer-light" => "light",
        "prefer-dark" => "dark",
        _ => "auto",
    }
}

fn normalize_wallpaper_placement(value: &str) -> &'static str {
    match value.trim().trim_matches('\'') {
        "none" => "none",
        "wallpaper" => "wallpaper",
        "centered" => "centered",
        "scaled" => "scaled",
        "stretched" => "stretched",
        "zoom" => "zoom",
        "spanned" => "spanned",
        _ => "invalid",
    }
}

fn normalize_wallpaper_shading(value: &str) -> &'static str {
    match value.trim().trim_matches('\'') {
        "solid" => "solid",
        "horizontal" => "horizontal",
        "vertical" => "vertical",
        _ => "invalid",
    }
}

fn appearance_detail(
    gsettings_available: bool,
    color_scheme_available: bool,
    color_scheme: &str,
) -> String {
    if !gsettings_available {
        return "Desktop preferences are not ready, so Appearance is read-only in this session."
            .to_string();
    }
    if !color_scheme_available {
        return "The standard color-scheme preference is not supported in this session."
            .to_string();
    }
    color_scheme_success_detail(color_scheme).to_string()
}

fn color_scheme_success_detail(color_scheme: &str) -> &'static str {
    match color_scheme {
        "prefer-light" => {
            "Light appearance is active across Goblins OS and apps that follow the desktop preference."
        }
        "prefer-dark" => {
            "Dark appearance is active across Goblins OS and apps that follow the desktop preference."
        }
        _ => {
            "Auto appearance is active. Goblins OS follows the standard desktop preference when it changes."
        }
    }
}

fn wallpaper_detail(
    gsettings_available: bool,
    schema_available: bool,
    picture_options_available: bool,
) -> String {
    if !gsettings_available {
        return "Desktop preferences are not ready, so wallpaper settings are read-only in this session."
            .to_string();
    }
    if !schema_available {
        return "The standard desktop background preferences are not supported in this session."
            .to_string();
    }
    if !picture_options_available {
        return "The standard wallpaper placement preference is not supported in this session."
            .to_string();
    }
    "Wallpaper preferences are ready for this desktop.".to_string()
}

fn wallpaper_placement_detail(placement: &str) -> &'static str {
    match normalize_wallpaper_placement(placement) {
        "none" => "No image is drawn; the configured desktop color is shown.",
        "wallpaper" => "The wallpaper image is tiled at its original size.",
        "centered" => "The wallpaper image is centered at its original size.",
        "scaled" => "The wallpaper image fits inside the desktop without cropping.",
        "stretched" => "The wallpaper image stretches to fill the desktop.",
        "spanned" => "The wallpaper image spans multiple monitors.",
        _ => "The wallpaper image fills the desktop while preserving proportions.",
    }
}

fn wallpaper_shading_detail(shading: &str) -> &'static str {
    match normalize_wallpaper_shading(shading) {
        "horizontal" => "Desktop colors blend horizontally behind the wallpaper.",
        "vertical" => "Desktop colors blend vertically behind the wallpaper.",
        _ => "The primary desktop color appears behind the wallpaper.",
    }
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    match bounded_session_command_output("gsettings", args, probe_timeout()) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(GSettingsError::Failed(gsettings_error_detail(
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ))),
        Err(_) => Err(GSettingsError::Missing),
    }
}

fn gsettings_error_detail(stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    stdout.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        color_scheme_theme, normalize_color_scheme, normalize_wallpaper_placement,
        normalize_wallpaper_shading, parse_gsettings_string, wallpaper_placement_detail,
        wallpaper_shading_detail,
    };

    #[test]
    fn color_scheme_inputs_are_normalized_to_gnome_values() {
        assert_eq!(normalize_color_scheme("light"), "prefer-light");
        assert_eq!(normalize_color_scheme("prefer-light"), "prefer-light");
        assert_eq!(normalize_color_scheme("dark"), "prefer-dark");
        assert_eq!(normalize_color_scheme("prefer-dark"), "prefer-dark");
        assert_eq!(normalize_color_scheme("auto"), "default");
        assert_eq!(normalize_color_scheme("default"), "default");
        assert_eq!(normalize_color_scheme("sepia"), "invalid");
    }

    #[test]
    fn color_scheme_theme_maps_to_settings_segments() {
        assert_eq!(color_scheme_theme("prefer-light"), "light");
        assert_eq!(color_scheme_theme("prefer-dark"), "dark");
        assert_eq!(color_scheme_theme("default"), "auto");
        assert_eq!(color_scheme_theme("invalid"), "auto");
    }

    #[test]
    fn parses_gsettings_string_values() {
        assert_eq!(
            parse_gsettings_string("'prefer-dark'\n"),
            Some("prefer-dark".to_string())
        );
        assert_eq!(
            parse_gsettings_string("default"),
            Some("default".to_string())
        );
        assert_eq!(parse_gsettings_string("  "), None);
    }

    #[test]
    fn wallpaper_placement_inputs_are_normalized_to_gnome_values() {
        assert_eq!(normalize_wallpaper_placement("zoom"), "zoom");
        assert_eq!(normalize_wallpaper_placement("'scaled'"), "scaled");
        assert_eq!(normalize_wallpaper_placement("centered"), "centered");
        assert_eq!(normalize_wallpaper_placement("stretched"), "stretched");
        assert_eq!(normalize_wallpaper_placement("wallpaper"), "wallpaper");
        assert_eq!(normalize_wallpaper_placement("spanned"), "spanned");
        assert_eq!(normalize_wallpaper_placement("none"), "none");
        assert_eq!(normalize_wallpaper_placement("crop"), "invalid");
    }

    #[test]
    fn wallpaper_placement_detail_tracks_selected_mode() {
        assert!(wallpaper_placement_detail("zoom").contains("fills"));
        assert!(wallpaper_placement_detail("scaled").contains("fits"));
        assert!(wallpaper_placement_detail("centered").contains("centered"));
        assert!(wallpaper_placement_detail("wallpaper").contains("tiled"));
        assert!(wallpaper_placement_detail("spanned").contains("spans"));
        assert!(wallpaper_placement_detail("none").contains("No image"));
    }

    #[test]
    fn wallpaper_shading_inputs_are_allowlisted_gnome_values() {
        assert_eq!(normalize_wallpaper_shading("solid"), "solid");
        assert_eq!(normalize_wallpaper_shading("'horizontal'"), "horizontal");
        assert_eq!(normalize_wallpaper_shading("vertical"), "vertical");
        assert_eq!(normalize_wallpaper_shading("radial"), "invalid");
    }

    #[test]
    fn wallpaper_shading_detail_tracks_selected_mode() {
        assert!(wallpaper_shading_detail("solid").contains("primary"));
        assert!(wallpaper_shading_detail("horizontal").contains("horizontally"));
        assert!(wallpaper_shading_detail("vertical").contains("vertically"));
    }
}
