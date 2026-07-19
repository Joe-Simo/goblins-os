//! On-device Live Text / OCR for Goblins OS.
//!
//! Recognizes text in an image with the local Tesseract runtime — no network,
//! private by default. Powers the markup editor's "Copy Text" action and the
//! screenshot → on-device-AI handoff, so recognized text reaches the clipboard
//! and the model directly instead of asking the user to retype it. Mirrors the
//! voice module: a capability probe + a shelled-out runtime, honest-gated when
//! the binary or language data is absent.

use std::{collections::BTreeMap, env, path::Path, time::Duration};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, probe_timeout, BoundedCommandError};

/// Recognition is genuinely heavy compute (large images, slow hardware), so it
/// gets a wider bound than the read-only status probes.
const RECOGNIZE_TIMEOUT: Duration = Duration::from_secs(60);

const DEFAULT_LANG: &str = "eng";

#[derive(Serialize)]
pub struct OcrStatus {
    source: &'static str,
    available: bool,
    /// All recognition is local, so OCR is safe in offline / private mode.
    offline_safe: bool,
    language: String,
    detail: String,
}

#[derive(Deserialize)]
pub struct OcrRequest {
    /// Absolute path to a local image to recognize (e.g. a captured screenshot).
    image_path: String,
    /// Optional language override; defaults to English. Must be an installed
    /// Tesseract langpack (e.g. "eng", or "eng+deu").
    #[serde(default)]
    language: Option<String>,
}

/// One recognized line, with its bounding box in image pixels — the markup
/// editor draws selectable overlay boxes from these.
#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct OcrLine {
    text: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Serialize)]
pub struct OcrOutcome {
    ok: bool,
    /// All recognized text, newline-joined, ready for the clipboard.
    text: String,
    lines: Vec<OcrLine>,
    detail: String,
}

pub async fn ocr_status() -> Json<OcrStatus> {
    Json(build_status(DEFAULT_LANG))
}

/// Recognition shells out for up to two 60s Tesseract passes, so the body runs
/// on the blocking pool instead of pinning an async runtime worker.
pub async fn ocr_recognize(Json(request): Json<OcrRequest>) -> (StatusCode, Json<OcrOutcome>) {
    crate::bounded::run_blocking(move || ocr_recognize_blocking(request))
        .await
        .unwrap_or_else(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(OcrOutcome {
                    ok: false,
                    text: String::new(),
                    lines: Vec::new(),
                    detail: crate::bounded::LONG_OPERATION_BUSY_MESSAGE.to_string(),
                }),
            )
        })
}

fn ocr_recognize_blocking(request: OcrRequest) -> (StatusCode, Json<OcrOutcome>) {
    let lang = request
        .language
        .as_deref()
        .unwrap_or(DEFAULT_LANG)
        .to_string();
    match run_recognize(&request.image_path, &lang) {
        Ok((text, lines)) => (
            StatusCode::OK,
            Json(OcrOutcome {
                ok: true,
                detail: if text.is_empty() {
                    "No text found in this image.".to_string()
                } else {
                    "Recognized on-device.".to_string()
                },
                text,
                lines,
            }),
        ),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(OcrOutcome {
                ok: false,
                text: String::new(),
                lines: Vec::new(),
                detail,
            }),
        ),
    }
}

fn build_status(lang: &str) -> OcrStatus {
    let available = ocr_runtime_ready(&tesseract_bin(), lang);
    OcrStatus {
        source: "goblins-os-core",
        available,
        offline_safe: true,
        language: lang.to_string(),
        detail: if available {
            "Text recognition is ready. Capture or open an image and choose Copy Text.".to_string()
        } else {
            "Text recognition is not available on this device.".to_string()
        },
    }
}

fn tesseract_bin() -> String {
    env::var("GOBLINS_OS_TESSERACT_BIN").unwrap_or_else(|_| "tesseract".to_string())
}

fn binary_present(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).exists();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

/// A language token is safe to pass to Tesseract: lowercase script codes joined
/// by `+`. Rejecting anything else keeps an attacker from smuggling CLI flags.
fn language_is_valid(lang: &str) -> bool {
    !lang.is_empty()
        && lang
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+')
}

fn ocr_runtime_ready(bin: &str, lang: &str) -> bool {
    if !binary_present(bin) || !language_is_valid(lang) {
        return false;
    }
    // The runtime is ready only if every requested language is installed.
    match bounded_command_output(bin, &["--list-langs"], probe_timeout()) {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let installed: Vec<&str> = stdout.lines().map(str::trim).collect();
            lang.split('+').all(|want| installed.contains(&want))
        }
        _ => false,
    }
}

fn run_recognize(image_path: &str, lang: &str) -> Result<(String, Vec<OcrLine>), String> {
    let bin = tesseract_bin();
    if !ocr_runtime_ready(&bin, lang) {
        return Err("Text recognition is not available on this device.".to_string());
    }
    let path = Path::new(image_path);
    if !path.is_file() {
        return Err("The image to recognize was not found.".to_string());
    }

    // Plain-text pass for the clipboard payload.
    let text_out = bounded_command_output(
        &bin,
        &[image_path, "stdout", "-l", lang, "--psm", "3"],
        RECOGNIZE_TIMEOUT,
    )
    .map_err(recognize_error_detail)?;
    if !text_out.status.success() {
        return Err("Text recognition failed on this image.".to_string());
    }
    let text = String::from_utf8_lossy(&text_out.stdout).trim().to_string();

    // TSV pass for per-line bounding boxes (the selectable overlay geometry).
    let tsv_out = bounded_command_output(
        &bin,
        &[image_path, "stdout", "-l", lang, "tsv"],
        RECOGNIZE_TIMEOUT,
    )
    .map_err(recognize_error_detail)?;
    let lines = if tsv_out.status.success() {
        parse_tsv_lines(&String::from_utf8_lossy(&tsv_out.stdout))
    } else {
        Vec::new()
    };

    Ok((text, lines))
}

/// Honest detail copy for a recognition pass that never produced output: a
/// timeout means the runtime IS present but was killed at the bound, so it
/// must not read as if the runtime were absent.
fn recognize_error_detail(error: BoundedCommandError) -> String {
    match error {
        BoundedCommandError::TimedOut => {
            "Text recognition did not finish in time on this device.".to_string()
        }
        _ => "The text-recognition runtime could not start.".to_string(),
    }
}

/// (block, paragraph, line) — the key that groups TSV words into a line.
type LineKey = (i32, i32, i32);
/// Accumulator for a line: its words plus its bounding box (min_x, min_y, max_x, max_y).
type LineAccum = (Vec<String>, i32, i32, i32, i32);

/// Group Tesseract TSV word rows (level 5) into lines by (block, paragraph, line),
/// joining their words and unioning their bounding boxes. Pure — unit-tested.
fn parse_tsv_lines(tsv: &str) -> Vec<OcrLine> {
    let mut groups: BTreeMap<LineKey, LineAccum> = BTreeMap::new();
    let mut order: Vec<LineKey> = Vec::new();

    for row in tsv.lines().skip(1) {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        let int = |i: usize| cols[i].trim().parse::<i32>().ok();
        // level 5 = a word; only words carry text + a tight box.
        if int(0) != Some(5) {
            continue;
        }
        let (block, par, line) = match (int(2), int(3), int(4)) {
            (Some(b), Some(p), Some(l)) => (b, p, l),
            _ => continue,
        };
        let (left, top, width, height) = match (int(6), int(7), int(8), int(9)) {
            (Some(x), Some(y), Some(w), Some(h)) => (x, y, w, h),
            _ => continue,
        };
        let word = cols[11].trim();
        if word.is_empty() {
            continue;
        }
        let key = (block, par, line);
        let entry = groups.entry(key).or_insert_with(|| {
            order.push(key);
            (Vec::new(), i32::MAX, i32::MAX, i32::MIN, i32::MIN)
        });
        entry.0.push(word.to_string());
        entry.1 = entry.1.min(left);
        entry.2 = entry.2.min(top);
        entry.3 = entry.3.max(left + width);
        entry.4 = entry.4.max(top + height);
    }

    order
        .into_iter()
        .filter_map(|key| {
            let (words, min_x, min_y, max_x, max_y) = groups.remove(&key)?;
            if words.is_empty() {
                return None;
            }
            Some(OcrLine {
                text: words.join(" "),
                x: min_x,
                y: min_y,
                width: (max_x - min_x).max(0),
                height: (max_y - min_y).max(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{language_is_valid, parse_tsv_lines, OcrLine};

    const TSV_HEADER: &str =
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

    #[test]
    fn language_token_validation_blocks_injection() {
        assert!(language_is_valid("eng"));
        assert!(language_is_valid("eng+deu"));
        assert!(!language_is_valid("")); // empty
        assert!(!language_is_valid("eng;rm -rf")); // shell-ish
        assert!(!language_is_valid("../etc")); // path-ish
        assert!(!language_is_valid("-l")); // a flag
    }

    #[test]
    fn tsv_groups_words_into_lines_with_union_bboxes() {
        // Two words on one line, one word on a second line. Levels 1-4 are
        // page/block/par/line rows (ignored); only level-5 word rows count.
        let tsv = format!(
            "{TSV_HEADER}\n\
             1\t1\t0\t0\t0\t0\t0\t0\t500\t200\t-1\t\n\
             5\t1\t1\t1\t1\t1\t10\t20\t40\t12\t96\tHello\n\
             5\t1\t1\t1\t1\t2\t60\t22\t50\t14\t95\tworld\n\
             5\t1\t1\t1\t2\t1\t12\t50\t80\t16\t90\tSecond\n"
        );
        let lines = parse_tsv_lines(&tsv);
        assert_eq!(
            lines,
            vec![
                // line 1: words joined; box = union (x=10,y=20 → right=110,bottom=36)
                OcrLine {
                    text: "Hello world".into(),
                    x: 10,
                    y: 20,
                    width: 100,
                    height: 16
                },
                OcrLine {
                    text: "Second".into(),
                    x: 12,
                    y: 50,
                    width: 80,
                    height: 16
                },
            ]
        );
    }

    #[test]
    fn tsv_skips_empty_and_malformed_rows() {
        let tsv = format!(
            "{TSV_HEADER}\n\
             5\t1\t1\t1\t1\t1\t10\t20\t40\t12\t96\t \n\
             5\t1\t1\t1\t1\t2\t60\t22\t50\t14\t95\tonly\n\
             garbage row that should be ignored\n"
        );
        let lines = parse_tsv_lines(&tsv);
        assert_eq!(
            lines,
            vec![OcrLine {
                text: "only".into(),
                x: 60,
                y: 22,
                width: 50,
                height: 14
            }]
        );
    }

    #[test]
    fn empty_tsv_yields_no_lines() {
        assert!(parse_tsv_lines("").is_empty());
        assert!(parse_tsv_lines(TSV_HEADER).is_empty());
    }
}
