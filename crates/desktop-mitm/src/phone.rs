//! Phone debugging through this computer: the phone sets the computer as its Wi-Fi proxy and opens the setup
//! page of the engine's LAN share (`engine_embedded::lan_share`), which the window offers as a QR code.
//!
//! The share asks phones for a proxy username and password by default ([`LanShareAuth`]): anyone else on the
//! same network (a café or office Wi-Fi, a CGNAT pool, a mesh VPN) would otherwise use this computer's nodes and
//! end up in its capture. The backends make the pair on first use, keep it in the platform's key store and put it
//! into every `start_lan_share` the window sends ([`authorized_start`]); the user may turn it off.

use std::fmt;

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The share's control commands the window may send to the engine.
pub const CONTROL_COMMANDS: [&str; 3] = ["lan_share", "start_lan_share", "stop_lan_share"];

/// The command that starts (or restarts) the share; the backends add the credentials to it.
pub const START_COMMAND: &str = "start_lan_share";

/// Characters of the made-up username and password: lower-case letters and digits without the ones that are
/// easy to mistake for each other (`0 o 1 l i`), since they are typed on a phone by hand.
const ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";
/// Random characters of the password: 20 of 31 is about 99 bits.
pub const PASSWORD_LEN: usize = 20;
/// Random characters after [`USERNAME_PREFIX`].
const USERNAME_RANDOM_LEN: usize = 6;
const USERNAME_PREFIX: &str = "anx-";
/// SOCKS5 carries each of them in at most 255 bytes.
const MAX_CREDENTIAL_LEN: usize = 255;

/// The proxy credentials of the LAN share and whether phones must send them. Kept by the platform's key store;
/// the window shows them so the user can type them on the phone. Has no derived `Debug`: the password never
/// reaches a log line.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanShareAuth {
    /// Phones must send the credentials; on unless the user turned it off.
    #[serde(default = "required_by_default")]
    pub required: bool,
    pub username: String,
    pub password: String,
}

fn required_by_default() -> bool {
    true
}

impl fmt::Debug for LanShareAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LanShareAuth")
            .field("required", &self.required)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl LanShareAuth {
    /// A new random username and password, required.
    pub fn generate() -> Result<Self, String> {
        Ok(Self {
            required: true,
            username: format!("{USERNAME_PREFIX}{}", random_text(USERNAME_RANDOM_LEN)?),
            password: random_text(PASSWORD_LEN)?,
        })
    }

    /// A new username and password; whether they are required stays as it was.
    pub fn regenerated(&self) -> Result<Self, String> {
        Ok(Self {
            required: self.required,
            ..Self::generate()?
        })
    }

    /// Whether the engine takes these credentials: both set, short enough for SOCKS5, and a username without
    /// the `:` that HTTP Basic splits on.
    pub fn is_valid(&self) -> bool {
        let fits = |text: &str| !text.is_empty() && text.len() <= MAX_CREDENTIAL_LEN;
        fits(&self.username)
            && fits(&self.password)
            && !self.username.contains(':')
            && !self
                .username
                .chars()
                .chain(self.password.chars())
                .any(char::is_control)
    }

    /// The stored form, read back by [`LanShareAuth::from_bytes`].
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|error| error.to_string())
    }

    /// The stored form, or `None` when it is damaged or holds credentials the engine would refuse.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice::<Self>(bytes)
            .ok()
            .filter(Self::is_valid)
    }
}

/// `len` characters of [`ALPHABET`] from the system's CSPRNG, without modulo bias.
fn random_text(len: usize) -> Result<String, String> {
    // The largest multiple of the alphabet's size below 256: bytes at or above it are drawn again.
    const LIMIT: u8 = (256 / ALPHABET.len() * ALPHABET.len()) as u8;
    let random = SystemRandom::new();
    let mut text = String::with_capacity(len);
    let mut buffer = [0u8; 32];
    while text.len() < len {
        random
            .fill(&mut buffer)
            .map_err(|_| "the LAN share password could not be generated".to_string())?;
        for byte in buffer {
            if text.len() == len {
                break;
            }
            if byte < LIMIT {
                text.push(char::from(ALPHABET[usize::from(byte) % ALPHABET.len()]));
            }
        }
    }
    Ok(text)
}

/// Whether `request` starts the share.
pub fn is_start(request: &Value) -> bool {
    request.get("cmd").and_then(Value::as_str) == Some(START_COMMAND)
}

/// The window's `start_lan_share` with the stored credentials in place of any it carried: with `auth.required`
/// the engine then asks every phone for them, otherwise it asks for none. Only the backend decides this, so a
/// window can never start an open share while the user wants a password.
pub fn authorized_start(request: &Value, auth: &LanShareAuth) -> Result<Value, String> {
    let mut request = request.clone();
    let fields = request
        .as_object_mut()
        .ok_or("the LAN share request is not an object")?;
    fields.remove("username");
    fields.remove("password");
    if auth.required {
        if !auth.is_valid() {
            return Err("the LAN share credentials are not usable; make new ones".to_string());
        }
        fields.insert("username".to_string(), Value::from(auth.username.clone()));
        fields.insert("password".to_string(), Value::from(auth.password.clone()));
    }
    Ok(request)
}

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
    use serde_json::json;

    #[test]
    fn generated_credentials_are_random_typeable_and_required() {
        let auth = LanShareAuth::generate().unwrap();
        assert!(auth.required);
        assert!(auth.is_valid());
        assert!(auth.username.starts_with(USERNAME_PREFIX));
        assert_eq!(
            auth.username.len(),
            USERNAME_PREFIX.len() + USERNAME_RANDOM_LEN
        );
        assert_eq!(auth.password.len(), PASSWORD_LEN);
        assert!(auth.password.bytes().all(|byte| ALPHABET.contains(&byte)));
        let other = LanShareAuth::generate().unwrap();
        assert_ne!(auth.password, other.password);
        assert_ne!(auth.username, other.username);
        // Every character of the alphabet turns up and nothing else does.
        let many: String = (0..64).map(|_| random_text(64).unwrap()).collect();
        assert!(ALPHABET.iter().all(|byte| many.as_bytes().contains(byte)));
        assert!(!many.contains(['0', 'o', '1', 'l', 'i']));
    }

    #[test]
    fn regenerating_keeps_the_switch_and_changes_the_pair() {
        let mut auth = LanShareAuth::generate().unwrap();
        auth.required = false;
        let fresh = auth.regenerated().unwrap();
        assert!(!fresh.required);
        assert_ne!(fresh.password, auth.password);
    }

    #[test]
    fn the_stored_form_reads_back_and_old_or_damaged_forms_are_refused() {
        let auth = LanShareAuth::generate().unwrap();
        assert_eq!(
            LanShareAuth::from_bytes(&auth.to_bytes().unwrap()),
            Some(auth)
        );
        // Without the switch it is required.
        let read =
            LanShareAuth::from_bytes(br#"{"username":"anx-abc","password":"secret"}"#).unwrap();
        assert!(read.required);
        assert_eq!(LanShareAuth::from_bytes(b"not json"), None);
        assert_eq!(
            LanShareAuth::from_bytes(br#"{"username":"a:b","password":"secret"}"#),
            None
        );
        assert_eq!(
            LanShareAuth::from_bytes(br#"{"username":"","password":"secret"}"#),
            None
        );
    }

    #[test]
    fn the_password_never_reaches_debug_output() {
        let auth = LanShareAuth::generate().unwrap();
        let text = format!("{auth:?}");
        assert!(text.contains(&auth.username));
        assert!(!text.contains(&auth.password));
    }

    #[test]
    fn starts_carry_the_stored_credentials_and_nothing_the_window_sent() {
        let auth = LanShareAuth {
            required: true,
            username: "anx-abcdef".to_string(),
            password: "p".repeat(PASSWORD_LEN),
        };
        let window = json!({"cmd": "start_lan_share", "port": 8899});
        assert!(is_start(&window));
        assert!(!is_start(&json!({"cmd": "lan_share"})));
        assert_eq!(
            authorized_start(&window, &auth).unwrap(),
            json!({"cmd": "start_lan_share", "port": 8899, "username": "anx-abcdef", "password": "p".repeat(PASSWORD_LEN)})
        );
        // A window cannot pick its own pair, nor start an open share while one is required.
        let forged =
            json!({"cmd": "start_lan_share", "port": 8899, "username": "x", "password": "y"});
        assert_eq!(
            authorized_start(&forged, &auth).unwrap()["username"],
            "anx-abcdef"
        );
        // Turned off by the user: no credentials at all, also none the window sent.
        let open = LanShareAuth {
            required: false,
            ..auth.clone()
        };
        assert_eq!(
            authorized_start(&forged, &open).unwrap(),
            json!({"cmd": "start_lan_share", "port": 8899})
        );
        let broken = LanShareAuth {
            password: String::new(),
            ..auth
        };
        assert!(authorized_start(&window, &broken).is_err());
        assert!(authorized_start(&json!("start_lan_share"), &open).is_err());
    }

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
