//! Preview viewer substrate.
//!
//! Goblins OS uses the Fedora/GNOME viewers already installed in the image:
//! Papers for PDF/PostScript-style documents and Loupe for image files. This
//! module keeps the contract narrow: report viewer availability, validate a
//! local file path by extension, and open it through the desktop default handler
//! with `xdg-open`. It never reads file contents or claims rendered proof.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::isolated_session_command;

const PREVIEW_KIND_PDF: &str = "pdf";
const PREVIEW_KIND_IMAGE: &str = "image";

const PDF_EXTENSIONS: &[&str] = &["pdf", "ps"];
const IMAGE_EXTENSIONS: &[&str] = &["bmp", "gif", "jpeg", "jpg", "png", "tif", "tiff", "webp"];

#[derive(Serialize)]
pub struct PreviewStatus {
    source: &'static str,
    available: bool,
    xdg_open_available: bool,
    papers_available: bool,
    loupe_available: bool,
    supported_extensions: Vec<String>,
    detail: String,
}

#[derive(Deserialize)]
pub struct PreviewOpenRequest {
    path: String,
}

#[derive(Serialize)]
pub struct PreviewOpenOutcome {
    ok: bool,
    text: String,
    path: String,
    kind: Option<String>,
}

pub async fn preview_status() -> Json<PreviewStatus> {
    Json(build_preview_status())
}

pub async fn open_preview(
    Json(request): Json<PreviewOpenRequest>,
) -> (StatusCode, Json<PreviewOpenOutcome>) {
    let (status, outcome) = open_preview_outcome(request);
    (status, Json(outcome))
}

fn build_preview_status() -> PreviewStatus {
    let xdg_open_available = executable_exists("xdg-open");
    let papers_available = executable_exists("papers");
    let loupe_available = executable_exists("loupe");
    let available = xdg_open_available && papers_available && loupe_available;

    PreviewStatus {
        source: "goblins-os-core",
        available,
        xdg_open_available,
        papers_available,
        loupe_available,
        supported_extensions: supported_preview_extensions(papers_available, loupe_available),
        detail: preview_detail(xdg_open_available, papers_available, loupe_available).to_string(),
    }
}

fn open_preview_outcome(request: PreviewOpenRequest) -> (StatusCode, PreviewOpenOutcome) {
    let path = PathBuf::from(request.path.trim());
    let Some(kind) = preview_kind_for_path(&path) else {
        return preview_response(
            StatusCode::BAD_REQUEST,
            false,
            "Preview opens only local PDF/PostScript documents and common image files.",
            &path,
            None,
        );
    };
    if !path.is_absolute() {
        return preview_response(
            StatusCode::BAD_REQUEST,
            false,
            "Preview paths must be absolute local filesystem paths.",
            &path,
            Some(kind),
        );
    }
    if !path.is_file() {
        return preview_response(
            StatusCode::NOT_FOUND,
            false,
            "Preview could not find a regular file at that path.",
            &path,
            Some(kind),
        );
    }

    let status = build_preview_status();
    if !status.xdg_open_available {
        return preview_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Preview cannot open files because xdg-open is not installed.",
            &path,
            Some(kind),
        );
    }
    if kind == PREVIEW_KIND_PDF && !status.papers_available {
        return preview_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "PDF preview needs Papers, which is not installed in this image.",
            &path,
            Some(kind),
        );
    }
    if kind == PREVIEW_KIND_IMAGE && !status.loupe_available {
        return preview_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Image preview needs Loupe, which is not installed in this image.",
            &path,
            Some(kind),
        );
    }

    match crate::session_bridge::open_preview(&path, kind) {
        crate::session_bridge::SessionBridgeResult::Success(_) => {
            return preview_response(
                StatusCode::OK,
                true,
                "Opened with the desktop Preview viewer.",
                &path,
                Some(kind),
            );
        }
        crate::session_bridge::SessionBridgeResult::Failed(detail) => {
            return preview_response(
                StatusCode::BAD_GATEWAY,
                false,
                &format!("Preview could not hand the file to the desktop session: {detail}"),
                &path,
                Some(kind),
            );
        }
        crate::session_bridge::SessionBridgeResult::Unavailable => {}
    }

    match isolated_session_command("xdg-open")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => preview_response(
            StatusCode::OK,
            true,
            "Opened with the desktop Preview viewer.",
            &path,
            Some(kind),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => preview_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Preview cannot open files because xdg-open is not installed.",
            &path,
            Some(kind),
        ),
        Err(_) => preview_response(
            StatusCode::BAD_GATEWAY,
            false,
            "Preview could not hand the file to the desktop default app.",
            &path,
            Some(kind),
        ),
    }
}

fn preview_response(
    status: StatusCode,
    ok: bool,
    text: &str,
    path: &Path,
    kind: Option<&str>,
) -> (StatusCode, PreviewOpenOutcome) {
    (
        status,
        PreviewOpenOutcome {
            ok,
            text: text.to_string(),
            path: path.display().to_string(),
            kind: kind.map(ToString::to_string),
        },
    )
}

fn preview_detail(
    xdg_open_available: bool,
    papers_available: bool,
    loupe_available: bool,
) -> &'static str {
    match (xdg_open_available, papers_available, loupe_available) {
        (true, true, true) => {
            "PDF and image preview opens through desktop defaults: Papers for PDFs and Loupe for images."
        }
        (false, _, _) => "Preview opening is unavailable because xdg-open is not installed.",
        (_, false, false) => "Preview viewers are unavailable because Papers and Loupe are not installed.",
        (_, false, true) => "Image preview is available through Loupe; PDF preview needs Papers.",
        (_, true, false) => "PDF preview is available through Papers; image preview needs Loupe.",
    }
}

fn supported_preview_extensions(papers_available: bool, loupe_available: bool) -> Vec<String> {
    let mut extensions = Vec::new();
    if papers_available {
        extensions.extend(
            PDF_EXTENSIONS
                .iter()
                .map(|extension| (*extension).to_string()),
        );
    }
    if loupe_available {
        extensions.extend(
            IMAGE_EXTENSIONS
                .iter()
                .map(|extension| (*extension).to_string()),
        );
    }
    extensions
}

fn preview_kind_for_path(path: &Path) -> Option<&'static str> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())?;
    if PDF_EXTENSIONS.contains(&extension.as_str()) {
        Some(PREVIEW_KIND_PDF)
    } else if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        Some(PREVIEW_KIND_IMAGE)
    } else {
        None
    }
}

fn executable_exists(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        preview_detail, preview_kind_for_path, supported_preview_extensions, PREVIEW_KIND_IMAGE,
        PREVIEW_KIND_PDF,
    };
    use std::path::Path;

    #[test]
    fn preview_kind_is_allowlisted_and_case_insensitive() {
        assert_eq!(
            preview_kind_for_path(Path::new("/home/goblin/report.PDF")),
            Some(PREVIEW_KIND_PDF)
        );
        assert_eq!(
            preview_kind_for_path(Path::new("/home/goblin/photo.JpEg")),
            Some(PREVIEW_KIND_IMAGE)
        );
        assert_eq!(
            preview_kind_for_path(Path::new("/home/goblin/archive.zip")),
            None
        );
        assert_eq!(preview_kind_for_path(Path::new("/home/goblin/noext")), None);
    }

    #[test]
    fn preview_detail_reports_missing_viewers_honestly() {
        assert!(preview_detail(true, true, true).contains("Papers"));
        assert!(preview_detail(true, true, true).contains("Loupe"));
        assert!(preview_detail(false, true, true).contains("xdg-open"));
        assert!(preview_detail(true, false, true).contains("PDF preview needs Papers"));
        assert!(preview_detail(true, true, false).contains("image preview needs Loupe"));
    }

    #[test]
    fn supported_extensions_follow_installed_viewers() {
        assert_eq!(
            supported_preview_extensions(false, false),
            Vec::<String>::new()
        );
        let pdf_only = supported_preview_extensions(true, false);
        assert!(pdf_only.contains(&"pdf".to_string()));
        assert!(!pdf_only.contains(&"png".to_string()));
        let image_only = supported_preview_extensions(false, true);
        assert!(image_only.contains(&"png".to_string()));
        assert!(!image_only.contains(&"pdf".to_string()));
    }
}
