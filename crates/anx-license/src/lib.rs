//! This device's ANX licenses (anx spec §2.4.1, panel side in anix-control `internal/entitlement`).
//!
//! Only the ANX protocol is licensed; nothing else in the client depends on this crate's answers. A panel
//! that licenses ANX announces its license CA on every subscription response
//! (`X-AnixOps-Anx-License-Ca`). Only then does the client name a device key on the next request to that
//! origin (`X-AnixOps-Device: ed25519:<hex>`), so no other subscription server ever learns a device
//! identifier. The panel answers with the license (`X-AnixOps-Anx-License`, hex) and a status; the client
//! writes the license to a file that the ANX outbounds of that origin's nodes read when they connect
//! (`node_import::AnxLicense::license_cert_path`).
//!
//! Every origin gets its own device key (user, 2026-11-19): two panels never see the same key, so they
//! cannot tell from it that they serve the same device, and one panel's license never replaces another's.
//!
//! Files in the license directory (the client's private state directory):
//!
//! * `origins`: the subscription origins (`scheme://host[:port]`) that announced a license CA, newest
//!   first, one per line.
//! * `o-<16 hex>/` per origin (the first 8 bytes of the origin's SHA-256):
//!   * `device.key`: the Ed25519 private key, PKCS#8 (v1: a software key; mode 0600 on Unix).
//!   * `license.cert`: the license in hex, as the header carried it.
//!   * `status`: the panel's last status word.
//!
//! Before 2026-11-19 one `device.key`, `license.cert` and `status` sat in the directory itself for every
//! origin. On first use they move to the newest origin, which keeps its seat; any other origin gets a key of
//! its own at its next refresh.
//!
//! The node checks the license; what the client reads from it here only drives the status line.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ring::digest::{digest, SHA256};
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

const DEVICE_FILE: &str = "device.key";
const LICENSE_FILE: &str = "license.cert";
const STATUS_FILE: &str = "status";

/// A device key, as the ANX outbound and the subscription request need it.
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

/// One origin's device key and license file, for its nodes' ANX outbounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginLicense {
    /// `scheme://host[:port]`, as [`origin`] writes it.
    pub origin: String,
    pub device: Device,
    /// The license file, which may not exist yet.
    pub license_path: PathBuf,
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

    /// The directory of one origin's key, license and status.
    pub fn origin_dir(&self, origin: &str) -> PathBuf {
        let sum = digest(&SHA256, origin.as_bytes());
        self.dir.join(format!("o-{}", hex(&sum.as_ref()[..8])))
    }

    /// The license file of `origin`'s ANX outbounds.
    pub fn license_path(&self, origin: &str) -> PathBuf {
        self.origin_dir(origin).join(LICENSE_FILE)
    }

    /// `origin`'s device key, created on first use.
    pub fn device(&self, origin: &str) -> io::Result<Device> {
        self.migrate()?;
        let dir = self.origin_dir(origin);
        let pkcs8 = match fs::read(dir.join(DEVICE_FILE)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                    .map_err(|_| io::Error::other("the device key could not be generated"))?;
                write_private(&dir, DEVICE_FILE, doc.as_ref())?;
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

    /// Every origin that announced a license CA, with its device key, for the ANX outbounds
    /// (`node_import::set_anx_licenses`).
    pub fn licenses(&self) -> io::Result<Vec<OriginLicense>> {
        self.migrate()?;
        self.origins()
            .into_iter()
            .map(|origin| {
                Ok(OriginLicense {
                    device: self.device(&origin)?,
                    license_path: self.license_path(&origin),
                    origin,
                })
            })
            .collect()
    }

    /// The [`HEADER_DEVICE`] value to send to `url`: only to an origin that announced a license CA, and only
    /// that origin's own key.
    pub fn request_header(&self, url: &str) -> io::Result<Option<String>> {
        self.migrate()?;
        let Some(origin) = origin(url) else {
            return Ok(None);
        };
        if !self.origins().contains(&origin) {
            return Ok(None);
        }
        self.device(&origin).map(|device| Some(device.header()))
    }

    /// Records a subscription response from `url`. Returns `true` when the origin announced a license CA
    /// for the first time, so the caller should fetch once more with [`request_header`](Self::request_header)
    /// to get the license now rather than at the next refresh. An origin that stops announcing one is
    /// forgotten with its key and license.
    pub fn record(&self, url: &str, header: impl Fn(&str) -> Option<String>) -> io::Result<bool> {
        self.migrate()?;
        let Some(origin) = origin(url) else {
            return Ok(false);
        };
        let offered = header(HEADER_LICENSE_CA).is_some_and(|ca| is_hex(ca.trim(), 64));
        let mut origins = self.origins();
        let known = origins.contains(&origin);
        if offered != known {
            origins.retain(|o| *o != origin);
            if offered {
                origins.insert(0, origin.clone());
                for dropped in origins.split_off(MAX_ORIGINS.min(origins.len())) {
                    remove_dir(&self.origin_dir(&dropped))?;
                }
            } else {
                remove_dir(&self.origin_dir(&origin))?;
            }
            write_private(&self.dir, "origins", origins.join("\n").as_bytes())?;
        }
        if !offered {
            return Ok(false);
        }
        let dir = self.origin_dir(&origin);
        if let Some(status) = header(HEADER_LICENSE_STATUS) {
            let status: String = status
                .trim()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(32)
                .collect();
            write_private(&dir, STATUS_FILE, status.as_bytes())?;
        }
        if let Some(license) = header(HEADER_LICENSE) {
            let license = license.trim();
            if cert_not_after(license).is_some() {
                write_private(&dir, LICENSE_FILE, license.as_bytes())?;
            }
        }
        Ok(!known)
    }

    /// The licenses as the status line shows them: the best of every origin (a valid one before an expired
    /// one before none). `now` is the system time; a clock set back does not revive an expired license,
    /// because the license's own issue time bounds it from below.
    pub fn state(&self, now: u64) -> LicenseState {
        let _ = self.migrate();
        let states: Vec<LicenseState> = self
            .origins()
            .iter()
            .map(|origin| self.origin_state(origin, now))
            .collect();
        let latest = |valid: bool| {
            states
                .iter()
                .filter_map(|state| match state {
                    LicenseState::Valid { not_after } if valid => Some(*not_after),
                    LicenseState::Expired { not_after } if !valid => Some(*not_after),
                    _ => None,
                })
                .max()
        };
        if let Some(not_after) = latest(true) {
            LicenseState::Valid { not_after }
        } else if let Some(not_after) = latest(false) {
            LicenseState::Expired { not_after }
        } else {
            states
                .into_iter()
                .next()
                .unwrap_or(LicenseState::NotOffered)
        }
    }

    /// One origin's license as the status line shows it.
    pub fn origin_state(&self, origin: &str, now: u64) -> LicenseState {
        if !self.origins().iter().any(|known| known == origin) {
            return LicenseState::NotOffered;
        }
        let dir = self.origin_dir(origin);
        let status = fs::read_to_string(dir.join(STATUS_FILE))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let cert = fs::read_to_string(dir.join(LICENSE_FILE)).ok();
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

    /// Moves the single key, license and status of clients before 2026-11-19 to the newest origin (see the
    /// crate documentation). Without an origin they are removed: nothing uses them.
    fn migrate(&self) -> io::Result<()> {
        let legacy = self.dir.join(DEVICE_FILE);
        if !legacy.exists() {
            return Ok(());
        }
        match self.origins().first() {
            Some(newest) => {
                let dir = self.origin_dir(newest);
                fs::create_dir_all(&dir)?;
                for name in [LICENSE_FILE, STATUS_FILE, DEVICE_FILE] {
                    let from = self.dir.join(name);
                    if from.exists() && !dir.join(name).exists() {
                        fs::rename(&from, dir.join(name))?;
                    } else if from.exists() {
                        fs::remove_file(&from)?;
                    }
                }
            }
            None => {
                for name in [LICENSE_FILE, STATUS_FILE, DEVICE_FILE] {
                    match fs::remove_file(self.dir.join(name)) {
                        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}

/// `scheme://host[:port]` of an http(s) URL, lowercased: the key of an origin's license, and the value the
/// client puts in a node's `anx.license_origin`.
pub fn origin(url: &str) -> Option<String> {
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

fn remove_dir(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
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
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
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
    fn device_key_is_created_once_per_origin() {
        let store = store("device");
        let first = store.device("https://a.example").unwrap();
        let again = store.device("https://a.example").unwrap();
        assert_eq!(first, again);
        assert!(first.header().starts_with("ed25519:"));
        assert_eq!(first.header().len(), "ed25519:".len() + 64);
        assert!(!format!("{first:?}").contains(&first.private_key_hex()));
        // Another origin never gets the same key.
        assert_ne!(store.device("https://b.example").unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.origin_dir("https://a.example").join(DEVICE_FILE))
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
        let origin = "https://panel.example:8443";
        assert_eq!(store.request_header(URL).unwrap(), None);
        assert_eq!(store.state(1_800_000_000), LicenseState::NotOffered);
        // A plain subscription server: nothing is recorded.
        assert!(!store.record(URL, headers(&[])).unwrap());
        assert_eq!(store.request_header(URL).unwrap(), None);
        assert!(store.licenses().unwrap().is_empty());

        // The panel announces its CA: fetch again with the device header.
        assert!(store
            .record(URL, headers(&[(HEADER_LICENSE_CA, CA)]))
            .unwrap());
        let header = store.request_header(URL).unwrap().expect("named now");
        assert_eq!(header, store.device(origin).unwrap().header());
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
        assert_eq!(
            fs::read_to_string(store.license_path(origin)).unwrap(),
            CERT
        );
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
        assert_eq!(
            fs::read_to_string(store.license_path(origin)).unwrap(),
            CERT
        );
        assert_eq!(
            fs::read_to_string(store.origin_dir(origin).join(STATUS_FILE)).unwrap(),
            "no_seat"
        );

        // The panel turns licensing off: the origin is forgotten with its key and license.
        store.record(URL, headers(&[])).unwrap();
        assert_eq!(store.request_header(URL).unwrap(), None);
        assert_eq!(store.state(1_800_000_000), LicenseState::NotOffered);
        assert!(!store.origin_dir(origin).exists());
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn each_panel_gets_its_own_key_and_license() {
        let store = store("panels");
        let a = "https://a.example/s/1";
        let b = "https://b.example/s/2";
        store
            .record(
                a,
                headers(&[(HEADER_LICENSE_CA, CA), (HEADER_LICENSE, CERT)]),
            )
            .unwrap();
        store
            .record(
                b,
                headers(&[(HEADER_LICENSE_CA, CA), (HEADER_LICENSE_STATUS, "no_seat")]),
            )
            .unwrap();
        let header_a = store.request_header(a).unwrap().unwrap();
        let header_b = store.request_header(b).unwrap().unwrap();
        assert_ne!(header_a, header_b);

        // b's answer did not replace a's license.
        let licenses = store.licenses().unwrap();
        assert_eq!(licenses.len(), 2);
        let of = |origin: &str| licenses.iter().find(|l| l.origin == origin).unwrap();
        assert_eq!(of("https://a.example").device.header(), header_a);
        assert_eq!(of("https://b.example").device.header(), header_b);
        assert_eq!(
            fs::read_to_string(&of("https://a.example").license_path).unwrap(),
            CERT
        );
        assert!(!of("https://b.example").license_path.exists());
        assert_eq!(
            store.origin_state("https://b.example", 1_800_000_100),
            LicenseState::Missing {
                status: Some("no_seat".into())
            }
        );
        // The status line shows the best of them.
        assert_eq!(
            store.state(1_800_000_100),
            LicenseState::Valid {
                not_after: 1_800_604_800
            }
        );
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn the_single_key_of_older_clients_moves_to_the_newest_origin() {
        let store = store("legacy");
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        write_private(store.dir(), DEVICE_FILE, doc.as_ref()).unwrap();
        write_private(store.dir(), LICENSE_FILE, CERT.as_bytes()).unwrap();
        write_private(store.dir(), STATUS_FILE, b"ok").unwrap();
        write_private(
            store.dir(),
            "origins",
            b"https://new.example\nhttps://old.example",
        )
        .unwrap();
        let legacy = Ed25519KeyPair::from_pkcs8_maybe_unchecked(doc.as_ref())
            .unwrap()
            .public_key()
            .as_ref()
            .to_vec();

        let licenses = store.licenses().unwrap();
        let of = |origin: &str| licenses.iter().find(|l| l.origin == origin).unwrap();
        assert_eq!(
            of("https://new.example").device.header(),
            format!("ed25519:{}", hex(&legacy))
        );
        assert_eq!(
            fs::read_to_string(&of("https://new.example").license_path).unwrap(),
            CERT
        );
        assert_ne!(
            of("https://old.example").device,
            of("https://new.example").device
        );
        for name in [DEVICE_FILE, LICENSE_FILE, STATUS_FILE] {
            assert!(!store.dir().join(name).exists(), "{name} left behind");
        }
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
