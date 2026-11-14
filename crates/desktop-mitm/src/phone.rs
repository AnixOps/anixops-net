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
}
