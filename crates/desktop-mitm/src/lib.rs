//! HTTPS decryption on the desktop clients. The engine decrypts by itself (the same code as on iOS) once its
//! envelope has a `mitm` section; this crate makes that section from a local root and the plugins the user
//! imported, and creates the root where the platform has none of its own.

pub mod capture;
pub mod mappings;
pub mod phone;
pub mod plugins;
pub mod replay;
pub mod sessions;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use serde_json::{Map, Value};

/// The name browsers and trust stores show for a root made by [`Root::ensure`].
pub const ROOT_COMMON_NAME: &str = "AnixOps Net HTTPS Decryption Root";
const ROOT_CERTIFICATE_FILE: &str = "root-ca.pem";
const ROOT_KEY_FILE: &str = "root-ca-key.pem";

/// A root certificate and its key on disk, both PEM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

impl Root {
    /// Where the root of `directory` is kept, whether or not it exists yet.
    pub fn at(directory: &Path) -> Self {
        Self {
            certificate_path: directory.join(ROOT_CERTIFICATE_FILE),
            private_key_path: directory.join(ROOT_KEY_FILE),
        }
    }

    pub fn exists(&self) -> bool {
        self.certificate_path.is_file() && self.private_key_path.is_file()
    }

    /// The root in `directory`, generated on first use. The key is written owner-only.
    pub fn ensure(directory: &Path) -> Result<Self, String> {
        let root = Self::at(directory);
        if root.exists() {
            return Ok(root);
        }
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, ROOT_COMMON_NAME);
        name.push(DnType::OrganizationName, "AnixOps Net");
        let mut params = CertificateParams::default();
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let key = KeyPair::generate().map_err(|error| error.to_string())?;
        let certificate = params
            .self_signed(&key)
            .map_err(|error| error.to_string())?;
        write_private(&root.private_key_path, key.serialize_pem().as_bytes())?;
        fs::write(&root.certificate_path, certificate.pem()).map_err(|error| error.to_string())?;
        Ok(root)
    }

    /// Replaces the root in `directory` with the user's own CA (a team root, say), both PEM: the certificate must
    /// be a CA certificate and the key its PKCS#8 private key. Nothing is written when either is refused. The
    /// engine and the trust stores pick it up at the next start of HTTPS decryption.
    pub fn import(
        directory: &Path,
        certificate_pem: &str,
        private_key_pem: &str,
    ) -> Result<Self, String> {
        let certificate_der = certificate_der(certificate_pem)?;
        let is_ca = x509_parser::parse_x509_certificate(&certificate_der)
            .ok()
            .and_then(|(_, certificate)| {
                certificate
                    .basic_constraints()
                    .ok()
                    .flatten()
                    .map(|constraints| constraints.value.ca)
            })
            .unwrap_or(false);
        if !is_ca {
            return Err("the certificate is not a CA certificate".to_string());
        }
        let key = KeyPair::from_pem(private_key_pem)
            .map_err(|_| "the private key is not a PKCS#8 PEM key (BEGIN PRIVATE KEY)")?;
        let public_key = key.public_key_raw();
        if public_key.is_empty()
            || !certificate_der
                .windows(public_key.len())
                .any(|window| window == public_key)
        {
            return Err("the private key does not belong to the certificate".to_string());
        }
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let root = Self::at(directory);
        write_private(&root.private_key_path, key.serialize_pem().as_bytes())?;
        fs::write(
            &root.certificate_path,
            certificate_pem.trim().to_string() + "\n",
        )
        .map_err(|error| error.to_string())?;
        Ok(root)
    }

    /// SHA-256 of the root certificate, upper-case hex pairs joined by `:` as the phone setup page shows it.
    pub fn fingerprint(&self) -> Result<String, String> {
        let pem = fs::read_to_string(&self.certificate_path).map_err(|error| {
            format!("HTTPS decryption root certificate could not be read: {error}")
        })?;
        Ok(fingerprint(&certificate_der(&pem)?))
    }

    /// `envelope` with a `mitm` section made of this root, `policy` and the enabled plugins in `plugins`.
    pub fn envelope(
        &self,
        envelope: &[u8],
        policy: &str,
        plugins: Option<&Path>,
    ) -> Result<Vec<u8>, String> {
        let certificate = fs::read_to_string(&self.certificate_path).map_err(|error| {
            format!("HTTPS decryption root certificate could not be read: {error}")
        })?;
        let private_key = fs::read_to_string(&self.private_key_path)
            .map_err(|error| format!("HTTPS decryption root key could not be read: {error}"))?;
        envelope_with_mitm(envelope, &certificate, &private_key, policy, plugins)
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| error.to_string())
}

/// `envelope` with a `mitm` section made of the PEM root, `policy` and the enabled plugins of the library in
/// `plugins`.
pub fn envelope_with_mitm(
    envelope: &[u8],
    certificate_pem: &str,
    private_key_pem: &str,
    policy: &str,
    plugins: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let mut envelope: Value = serde_json::from_slice(envelope)
        .map_err(|_| "embedded engine configuration is not valid JSON".to_string())?;
    let sections = envelope
        .as_object_mut()
        .ok_or("embedded engine configuration is not a JSON object")?;
    let certificate = pem_body(certificate_pem)
        .ok_or("HTTPS decryption root certificate is not a PEM certificate")?;
    let private_key =
        pem_body(private_key_pem).ok_or("HTTPS decryption root key is not a PEM private key")?;
    let mut mitm = Map::new();
    mitm.insert("policy".to_string(), policy.into());
    mitm.insert("ca_certificate".to_string(), certificate.into());
    mitm.insert("ca_private_key".to_string(), private_key.into());
    if let Some(plugins) = plugins {
        plugins::apply(plugins, &mut mitm);
    }
    sections.insert("mitm".to_string(), mitm.into());
    serde_json::to_vec(&envelope).map_err(|error| error.to_string())
}

/// The DER of the first PEM block of `pem`.
fn certificate_der(pem: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    if !pem.contains("-----BEGIN CERTIFICATE-----") {
        return Err("the certificate is not a PEM certificate (BEGIN CERTIFICATE)".to_string());
    }
    pem_body(pem)
        .and_then(|body| base64::engine::general_purpose::STANDARD.decode(body).ok())
        .ok_or_else(|| "the certificate is not a PEM certificate (BEGIN CERTIFICATE)".to_string())
}

/// SHA-256 of `der`, upper-case hex pairs joined by `:`.
pub fn fingerprint(der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, der)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The base64 body of the first PEM block: the DER the engine expects.
pub fn pem_body(pem: &str) -> Option<String> {
    let body = pem
        .lines()
        .map(str::trim)
        .skip_while(|line| !line.starts_with("-----BEGIN "))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END "))
        .collect::<String>();
    (!body.is_empty()).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_bodies_are_the_base64_between_the_markers() {
        let pem = "-----BEGIN CERTIFICATE-----\r\nQUJD\r\nREVG\r\n-----END CERTIFICATE-----\r\n";
        assert_eq!(pem_body(pem).as_deref(), Some("QUJDREVG"));
        assert_eq!(pem_body("QUJD"), None);
        assert_eq!(
            pem_body("-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n"),
            None
        );
    }

    #[test]
    fn the_root_is_made_once_and_fills_the_mitm_section() {
        let directory =
            std::env::temp_dir().join(format!("desktop-mitm-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        assert!(!Root::at(&directory).exists());
        let root = Root::ensure(&directory).unwrap();
        let certificate = fs::read_to_string(&root.certificate_path).unwrap();
        assert_eq!(Root::ensure(&directory).unwrap(), root);
        assert_eq!(
            fs::read_to_string(&root.certificate_path).unwrap(),
            certificate
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&root.private_key_path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }

        let envelope = root.envelope(br#"{"schema_version":1}"#, "", None).unwrap();
        let envelope: Value = serde_json::from_slice(&envelope).unwrap();
        assert_eq!(
            envelope["mitm"]["ca_certificate"].as_str(),
            pem_body(&certificate).as_deref()
        );
        assert!(envelope["mitm"]["ca_private_key"].as_str().is_some());
        assert!(root.envelope(b"[]", "", None).is_err());
        assert_eq!(root.fingerprint().unwrap().len(), 32 * 3 - 1);
        let _ = fs::remove_dir_all(&directory);
    }

    fn own_ca(common_name: &str) -> (String, String) {
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, common_name);
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let key = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        (certificate.pem(), key.serialize_pem())
    }

    #[test]
    fn an_own_ca_replaces_the_root_only_when_it_is_valid() {
        let directory =
            std::env::temp_dir().join(format!("desktop-mitm-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let generated = Root::ensure(&directory).unwrap();
        let before = fs::read_to_string(&generated.certificate_path).unwrap();

        let (certificate, key) = own_ca("Team Debug CA");
        let (_, other_key) = own_ca("Someone Else");
        assert!(Root::import(&directory, &certificate, &other_key).is_err());
        assert!(Root::import(&directory, "not a certificate", &key).is_err());
        assert!(Root::import(&directory, &certificate, "not a key").is_err());
        // A leaf is not a CA.
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["example.com".to_string()])
            .unwrap()
            .self_signed(&leaf_key)
            .unwrap();
        assert!(Root::import(&directory, &leaf.pem(), &leaf_key.serialize_pem()).is_err());
        assert_eq!(
            fs::read_to_string(&generated.certificate_path).unwrap(),
            before
        );

        let root = Root::import(&directory, &certificate, &key).unwrap();
        assert_eq!(root, generated);
        assert_eq!(
            fs::read_to_string(&root.certificate_path).unwrap().trim(),
            certificate.trim()
        );
        let envelope = root.envelope(br#"{"schema_version":1}"#, "", None).unwrap();
        let envelope: Value = serde_json::from_slice(&envelope).unwrap();
        assert_eq!(
            envelope["mitm"]["ca_certificate"].as_str(),
            pem_body(&certificate).as_deref()
        );
        let _ = fs::remove_dir_all(&directory);
    }
}
