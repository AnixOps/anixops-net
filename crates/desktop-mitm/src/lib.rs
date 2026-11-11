//! HTTPS decryption on the desktop clients. The engine decrypts by itself (the same code as on iOS) once its
//! envelope has a `mitm` section; this crate makes that section from a local root and the plugins the user
//! imported, and creates the root where the platform has none of its own.

pub mod plugins;

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
        let _ = fs::remove_dir_all(&directory);
    }
}
