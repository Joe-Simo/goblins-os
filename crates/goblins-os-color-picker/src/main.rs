//! Goblins OS color picker — pick any pixel on screen, get an exact color.
//!
//! macOS's Digital Color Meter / screenshot eyedropper, in the Goblins voice.
//! Drives the freedesktop Screenshot portal's `PickColor` (GNOME renders its own
//! Wayland-correct magnified loupe), formats the sampled sRGB color as HEX / rgb()
//! / hsl(), copies HEX to the clipboard, and posts a calm toast. Headless-first:
//! the clipboard write and the printed values succeed even with no notifier, and
//! the whole flow degrades honestly when the portal is absent or declined.
//!
//! Bound to a desktop shortcut. The color formatting is pure and unit-tested; the
//! portal handshake + clipboard + toast are the OS-integration shell.

use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};

use ashpd::desktop::Color;

/// Generous but finite: the eyedropper waits for a human to click a pixel, but a
/// wedged backend or an abandoned pick still ends instead of hanging forever.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    match pick_color() {
        Ok((r, g, b)) => {
            let hex = to_hex(r, g, b);
            let rgb = to_rgb(r, g, b);
            let hsl = to_hsl(r, g, b);
            let copied = copy_to_clipboard(&hex);
            notify(&hex, &rgb, &hsl, copied);
            // Also print (newline-separated) for scripting / a no-notifier fallback.
            println!("{hex}\n{rgb}\n{hsl}");
        }
        Err(detail) => {
            // Honest gating: no panel, nothing copied, a clear reason on stderr.
            eprintln!("goblins-os-color-picker: {detail}");
        }
    }
}

/// Drive the portal PickColor request to completion and return the sampled sRGB
/// channels as f64 in `[0, 1]`. Runs on a single-threaded Tokio runtime that
/// blocks for the one result this helper needs.
fn pick_color() -> Result<(f64, f64, f64), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the color-picker runtime: {error}"))?;

    let pick = async {
        let color = Color::pick()
            .send()
            .await
            .map_err(|error| format!("the desktop color portal request failed: {error}"))?
            .response()
            .map_err(|error| format!("the desktop color portal was declined: {error}"))?;
        Ok::<(f64, f64, f64), String>((color.red(), color.green(), color.blue()))
    };

    runtime.block_on(async {
        match tokio::time::timeout(PORTAL_TIMEOUT, pick).await {
            Ok(result) => result,
            Err(_) => Err(format!(
                "the desktop color portal did not respond within {}s — nothing was copied",
                PORTAL_TIMEOUT.as_secs()
            )),
        }
    })
}

/// Copy text to the Wayland clipboard via `wl-copy`. Returns false (caller still
/// shows the value) when the clipboard tool is absent or fails.
fn copy_to_clipboard(text: &str) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().map(|status| status.success()).unwrap_or(false)
}

/// Best-effort calm toast. Absent notifier → silently skipped (the value is still
/// on the clipboard and printed).
fn notify(hex: &str, rgb: &str, hsl: &str, copied: bool) {
    let body = if copied {
        format!("{hex} · {rgb} · {hsl}\nCopied to clipboard")
    } else {
        format!("{hex} · {rgb} · {hsl}")
    };
    let _ = Command::new("notify-send")
        .args([
            "--app-name=Goblins OS",
            "--icon=color-select-symbolic",
            "Color picked",
            &body,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ── Pure color formatting (unit-tested) ──────────────────────────────────────

/// An sRGB channel in `[0, 1]` → an `[0, 255]` byte, clamped and rounded.
fn channel_to_u8(channel: f64) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn to_hex(r: f64, g: f64, b: f64) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        channel_to_u8(r),
        channel_to_u8(g),
        channel_to_u8(b)
    )
}

fn to_rgb(r: f64, g: f64, b: f64) -> String {
    format!(
        "rgb({}, {}, {})",
        channel_to_u8(r),
        channel_to_u8(g),
        channel_to_u8(b)
    )
}

fn to_hsl(r: f64, g: f64, b: f64) -> String {
    let (h, s, l) = srgb_to_hsl(r, g, b);
    format!(
        "hsl({}, {}%, {}%)",
        h.round() as i64,
        (s * 100.0).round() as i64,
        (l * 100.0).round() as i64
    )
}

/// sRGB (each channel in `[0, 1]`) → HSL with hue in degrees `[0, 360)` and
/// saturation/lightness in `[0, 1]`. Standard conversion; achromatic → hue 0.
fn srgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let r = r.clamp(0.0, 1.0);
    let g = g.clamp(0.0, 1.0);
    let b = b.clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta.abs() < f64::EPSILON {
        return (0.0, 0.0, lightness); // gray — no hue, no saturation
    }
    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let hue_sextant = if (max - r).abs() < f64::EPSILON {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let mut hue = hue_sextant * 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    (hue, saturation, lightness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_formats_uppercase_and_clamps() {
        assert_eq!(to_hex(0.0, 0.0, 0.0), "#000000");
        assert_eq!(to_hex(1.0, 1.0, 1.0), "#FFFFFF");
        assert_eq!(to_hex(1.0, 0.5, 0.0), "#FF8000"); // 0.5*255 = 127.5 → 128
        assert_eq!(to_hex(-1.0, 2.0, 0.5), "#00FF80"); // out-of-range clamps
    }

    #[test]
    fn rgb_string_is_byte_triplet() {
        assert_eq!(to_rgb(1.0, 0.0, 0.0), "rgb(255, 0, 0)");
        assert_eq!(to_rgb(0.0, 0.5, 1.0), "rgb(0, 128, 255)");
    }

    #[test]
    fn hsl_matches_known_colors() {
        assert_eq!(to_hsl(1.0, 0.0, 0.0), "hsl(0, 100%, 50%)"); // red
        assert_eq!(to_hsl(0.0, 1.0, 0.0), "hsl(120, 100%, 50%)"); // green
        assert_eq!(to_hsl(0.0, 0.0, 1.0), "hsl(240, 100%, 50%)"); // blue
        assert_eq!(to_hsl(1.0, 1.0, 1.0), "hsl(0, 0%, 100%)"); // white (achromatic)
        assert_eq!(to_hsl(0.5, 0.5, 0.5), "hsl(0, 0%, 50%)"); // gray (achromatic)
    }
}
