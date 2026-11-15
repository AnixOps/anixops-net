//! Phone debugging through this computer: the phone sets the computer as its Wi-Fi proxy and opens the setup
//! page of the engine's LAN share (`engine_embedded::lan_share`), which the window offers as a QR code.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// The share's control commands the window may send to the engine.
pub const CONTROL_COMMANDS: [&str; 3] = ["lan_share", "start_lan_share", "stop_lan_share"];

/// Longest text put in a QR code: a setup page address is far shorter.
const MAX_QR_TEXT: usize = 512;

/// An SVG QR code of `text` (black on white, with the quiet zone), or why there is none.
pub fn qr_svg(text: &str) -> Result<String, String> {
    if text.is_empty() || text.len() > MAX_QR_TEXT {
        return Err("the QR code text is empty or too long".to_string());
    }
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|error| error.to_string())?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .quiet_zone(true)
        .build())
}

/// Longest text [`qr_rows`] makes a code of: a share link with its keys still fits and can still be read
/// off a screen.
const MAX_QR_ROWS_TEXT: usize = 1024;

/// A QR code of `text` as its rows from the top, one character per module (`#` dark, `.` light), without
/// the quiet zone, for a window that draws it itself; or why there is none.
pub fn qr_rows(text: &str) -> Result<Vec<String>, String> {
    if text.is_empty() || text.len() > MAX_QR_ROWS_TEXT {
        return Err("the QR code text is empty or too long".to_string());
    }
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|error| error.to_string())?;
    let width = code.width();
    Ok(code
        .to_colors()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|module| {
                    if *module == qrcode::Color::Dark {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_codes_are_svg_and_refuse_bad_text() {
        let svg = qr_svg("http://192.168.1.5:8899/").unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("#000000"));
        assert!(qr_svg("").is_err());
        assert!(qr_svg(&"x".repeat(MAX_QR_TEXT + 1)).is_err());
    }

    #[test]
    fn qr_rows_are_a_square_with_the_finder_pattern_in_its_corner() {
        let rows = qr_rows("ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@203.0.113.9:11111#Test").unwrap();
        assert!(rows.len() >= 21);
        assert!(rows.iter().all(|row| row.chars().count() == rows.len()));
        assert!(rows
            .iter()
            .all(|row| row.chars().all(|c| c == '#' || c == '.')));
        // The finder pattern: a dark bar of seven, then a light module.
        assert!(rows[0].starts_with("#######."));
        assert!(rows[1].starts_with("#.....#."));
        assert!(qr_rows("").is_err());
        assert!(qr_rows(&"x".repeat(MAX_QR_ROWS_TEXT + 1)).is_err());
    }
}
