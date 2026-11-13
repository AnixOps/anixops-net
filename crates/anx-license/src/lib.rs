//! This device's ANX license (anx spec §2.4.1, panel side in anix-control `internal/entitlement`).
//!
//! Only the ANX protocol is licensed; nothing else in the client depends on this crate's answers. A panel
//! that licenses ANX announces its license CA on every subscription response
//! (`X-AnixOps-Anx-License-Ca`). Only then does the client name its device key on the next request
//! (`X-AnixOps-Device: ed25519:<hex>`), so no other subscription server ever learns a stable device
//! identifier. The panel answers with the license (`X-AnixOps-Anx-License`, hex) and a status; the client
//! writes the license to a file that every ANX outbound reads when it connects
//! (`node_import::AnxLicense::license_cert_path`).
//!
//! Files in the license directory (the client's private state directory):
//!
//! * `device.key`: the Ed25519 private key, PKCS#8 (v1: a software key; mode 0600 on Unix).
//! * `license.cert`: the license in hex, as the header carried it.
//! * `origins`: the subscription origins (`scheme://host[:port]`) that announced a license CA, one per line.
//! * `status`: the panel's last status word.
//!
//! The node checks the license; what the client reads from it here only drives the status line.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

/// Request header naming the device key.
pub const HEADER_DEVICE: &str = "X-AnixOps-Device";
/// Response header with the license, hex.
pub const HEADER_LICENSE: &str = "X-AnixOps-Anx-License";
/// Response header with the license CA's public key, hex; its presence means the panel licenses ANX.
pub const HEADER_LICENSE_CA: &str = "X-AnixOps-Anx-License-Ca";
/// Response header with the panel's status word (`ok`, `not_entitled`, `no_seat`, `revoked`, ...).
pub const HEADER_LICENSE_STATUS: &str = "X-AnixOps-Anx-License-Status";

/// `not_before` of a license is the issue time minus this (anx spec §2.4).
const BACKDATE_SECS: u64 = 300;
const MAX_ORIGINS: usize = 32;

/// The device key, as the ANX outbound and the subscription request need it.
#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    pkcs8: Vec<u8>,
    public_key: Vec<u8>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("public_key", &hex(&self.public_key))
            .finish_non_exhaustive()
    }
}

impl Device {
    /// The value of [`HEADER_DEVICE`].
    pub fn header(&self) -> String {
        format!("ed25519:{}", hex(&self.public_key))
    }

    /// The private key as the ANX outbound's `device_key` setting takes it: PKCS#8 in hex.
    pub fn private_key_hex(&self) -> String {
        hex(&self.pkcs8)
    }

    /// The ANX outbound's `device_alg` setting.
    pub fn alg(&self) -> &'static str {
        "ed25519"
    }
}

/// What the license says, for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseState {
    /// No subscription announced a license CA: the status line stays hidden.
    NotOffered,
    /// A license valid until `not_after` (Unix seconds).
    Valid { not_after: u64 },
    /// The license ran out; the next subscription refresh renews it.
    Expired { not_after: u64 },
    /// No license; `status` is the panel's last word, if any (`not_entitled`, `no_seat`, `revoked`, ...).
    Missing { status: Option<String> },
}

/// The license directory.
#[derive(Debug, Clone)]
pub struct LicenseStore {
    dir: PathBuf,
}

impl LicenseStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The license file every ANX outbound reads.
    pub fn license_path(&self) -> PathBuf {
        self.dir.join("license.cert")
    }

    /// The device key, created on first use.
    pub fn device(&self) -> io::Result<Device> {
        let path = self.dir.join("device.key");
        let pkcs8 = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                    .map_err(|_| io::Error::other("the device key could not be generated"))?;
                write_private(&self.dir, "device.key", doc.as_ref())?;
                doc.as_ref().to_vec()
            }
            Err(error) => return Err(error),
        };
        let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the device key is damaged"))?;
        Ok(Device {
            public_key: pair.public_key().as_ref().to_vec(),
            pkcs8,
        })
    }

    /// The [`HEADER_DEVICE`] value to send to `url`: only to an origin that announced a license CA.
    pub fn request_header(&self, url: &str) -> io::Result<Option<String>> {
        let Some(origin) = origin(url) else {
            return Ok(None);
        };
        if !self.origins().contains(&origin) {
            return Ok(None);
        }
        self.device().map(|device| Some(device.header()))
    }

    /// Records a subscription response from `url`. Returns `true` when the origin announced a license CA
    /// for the first time, so the caller should fetch once more with [`request_header`](Self::request_header)
    /// to get the license now rather than at the next refresh.
    pub fn record(&self, url: &str, header: impl Fn(&str) -> Option<String>) -> io::Result<bool> {
        let Some(origin) = origin(url) else {
            return Ok(false);
        };
        let offered = header(HEADER_LICENSE_CA).is_some_and(|ca| is_hex(ca.trim(), 64));
        let mut origins = self.origins();
        let known = origins.contains(&origin);
        if offered != known {
            origins.retain(|o| *o != origin);
            if offered {
                origins.insert(0, origin);
                origins.truncate(MAX_ORIGINS);
            }
            write_private(&self.dir, "origins", origins.join("\n").as_bytes())?;
        }
        if !offered {
            return Ok(false);
        }
        if let Some(status) = header(HEADER_LICENSE_STATUS) {
            let status: String = status
                .trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(32)
                .collect();
            write_private(&self.dir, "status", status.as_bytes())?;
        }
        if let Some(license) = header(HEADER_LICENSE) {
            let license = license.trim();
            if cert_not_after(license).is_some() {
                write_private(&self.dir, "license.cert", license.as_bytes())?;
            }
        }
        Ok(!known)
    }

    /// The license as the status line shows it. `now` is the system time; a clock set back does not
    /// revive an expired license, because the license's own issue time bounds it from below.
    pub fn state(&self, now: u64) -> LicenseState {
        if self.origins().is_empty() {
            return LicenseState::NotOffered;
        }
        let status = fs::read_to_string(self.dir.join("status"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let cert = fs::read_to_string(self.license_path()).ok();
        match cert.as_deref().and_then(|c| cert_times(c.trim())) {
            Some((not_before, not_after)) => {
                let now = now.max(not_before.saturating_add(BACKDATE_SECS));
                if now > not_after {
                    LicenseState::Expired { not_after }
                } else {
                    LicenseState::Valid { not_after }
                }
            }
            None => LicenseState::Missing { status },
        }
    }

    fn origins(&self) -> Vec<String> {
        fs::read_to_string(self.dir.join("origins"))
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `scheme://host[:port]` of an http(s) URL, lowercased.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// `(not_before, not_after)` of a license in hex (anx spec §2.4 layout).
fn cert_times(hex_text: &str) -> Option<(u64, u64)> {
    let bytes = unhex(hex_text)?;
    if bytes.len() < 19 || bytes[0] != 1 {
        return None;
    }
    let pk_len = bytes[18] as usize;
    let at = 19 + pk_len;
    if bytes.len() != at + 8 + 64 {
        return None;
    }
    let word =
        |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as u64;
    Some((word(at), word(at + 4)))
}

fn cert_not_after(hex_text: &str) -> Option<u64> {
    cert_times(hex_text).map(|(_, not_after)| not_after)
}

fn write_private(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(name);
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0xf)] as char);
    }
    out
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The license anix-control's `TestANXCertVector` pins: valid 1800000000–1800604800.
    const CERT: &str = "01c00f77ef73c16e3608c8dfab4273795e02203b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da296b49d2006b530c80e4f028a2f3c2add9a1cff3660629e2babb6053ed7e9d9e72476f8b87eb826a5932b570004a19d5225dd38a172800e384e3a6a32ba8e2b71c63819ea894ebc005";
    const CA: &str = "c83c22218d9d871e9eb6196777b1ee9715a34420d42646c09e6bbedf3687de26";
    const URL: &str = "https://Panel.Example:8443/s/token?flag=anixops";

    fn store(name: &str) -> LicenseStore {
        let mut dir = std::env::temp_dir();
        dir.push(format!("anx-license-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        LicenseStore::new(dir)
    }

    fn headers(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
            .collect();
        move |name: &str| map.get(&name.to_ascii_lowercase()).cloned()
    }

    #[test]
    fn device_key_is_created_once() {
        let store = store("device");
        let first = store.device().unwrap();
        let again = store.device().unwrap();
        assert_eq!(first, again);
        assert!(first.header().starts_with("ed25519:"));
        assert_eq!(first.header().len(), "ed25519:".len() + 64);
        assert!(!format!("{first:?}").contains(&first.private_key_hex()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.dir().join("device.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn the_device_is_named_only_to_origins_that_license_anx() {
        let store = store("origins");
        assert_eq!(store.request_header(URL).unwrap(), None);
        assert_eq!(store.state(1_800_000_000), LicenseState::NotOffered);
        // A plain subscription server: nothing is recorded.
        assert!(!store.record(URL, headers(&[])).unwrap());
        assert_eq!(store.request_header(URL).unwrap(), None);

        // The panel announces its CA: fetch again with the device header.
        assert!(store
            .record(URL, headers(&[(HEADER_LICENSE_CA, CA)]))
            .unwrap());
        let header = store.request_header(URL).unwrap().expect("named now");
        assert_eq!(header, store.device().unwrap().header());
        assert_eq!(
            store
                .request_header("https://panel.example:8443/other")
                .unwrap(),
            Some(header)
        );
        assert_eq!(
            store
                .request_header("https://elsewhere.example/s/x")
                .unwrap(),
            None
        );
        assert_eq!(
            store.state(1_800_000_000),
            LicenseState::Missing { status: None }
        );

        // The license arrives.
        assert!(!store
            .record(
                URL,
                headers(&[
                    (HEADER_LICENSE_CA, CA),
                    (HEADER_LICENSE, CERT),
                    (HEADER_LICENSE_STATUS, "ok")
                ])
            )
            .unwrap());
        assert_eq!(fs::read_to_string(store.license_path()).unwrap(), CERT);
        assert_eq!(
            store.state(1_800_000_100),
            LicenseState::Valid {
                not_after: 1_800_604_800
            }
        );
        assert_eq!(
            store.state(1_800_604_801),
            LicenseState::Expired {
                not_after: 1_800_604_800
            }
        );
        // A clock set back does not revive it... nor make a fresh one look unborn.
        assert_eq!(
            store.state(1_000),
            LicenseState::Valid {
                not_after: 1_800_604_800
            }
        );

        // A malformed license is ignored; the panel's word is kept.
        store
            .record(
                URL,
                headers(&[
                    (HEADER_LICENSE_CA, CA),
                    (HEADER_LICENSE, "zz"),
                    (HEADER_LICENSE_STATUS, "no_seat"),
                ]),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(store.license_path()).unwrap(), CERT);
        assert_eq!(
            fs::read_to_string(store.dir().join("status")).unwrap(),
            "no_seat"
        );

        // The panel turns licensing off: the origin is forgotten.
        store.record(URL, headers(&[])).unwrap();
        assert_eq!(store.request_header(URL).unwrap(), None);
        assert_eq!(store.state(1_800_000_000), LicenseState::NotOffered);
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn origins() {
        assert_eq!(
            origin("https://u:p@A.example/s/x").as_deref(),
            Some("https://a.example")
        );
        assert_eq!(
            origin("http://a.example:80?x").as_deref(),
            Some("http://a.example:80")
        );
        assert_eq!(origin("ss://abc@host:1"), None);
        assert_eq!(origin("https://"), None);
        assert_eq!(cert_not_after(CERT), Some(1_800_604_800));
        assert_eq!(cert_not_after(&CERT[2..]), None);
    }
}
