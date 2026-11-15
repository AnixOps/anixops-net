import CryptoKit
import Foundation
import Security

/// A team's own CA in place of the generated root, as the Windows and Linux clients offer (`desktop_mitm::Root::
/// import`): a PEM CA certificate and its PEM private key. The engine issues leaves with ECDSA P-256 only, so the key
/// must be a P-256 key (`BEGIN PRIVATE KEY` PKCS#8 or `BEGIN EC PRIVATE KEY`).
extension MitmCertificateAuthority {
    public enum ImportFailure: Error, Equatable {
        /// No `BEGIN CERTIFICATE` block, or one the Security framework cannot read.
        case notCertificate
        /// The certificate's basic constraints do not say it is a CA.
        case notCertificateAuthority
        /// The key is not a P-256 private key in PEM.
        case unsupportedKey
        /// The key does not belong to the certificate.
        case keyMismatch
    }

    /// Checks the pair and returns it as an authority; nothing is stored. Save it with `MitmCertificateStore.save`.
    public static func importing(certificatePEM: String, privateKeyPEM: String) throws -> MitmCertificateAuthority {
        guard let der = pemBlock(in: certificatePEM, labels: ["CERTIFICATE"])?.der,
              let certificate = SecCertificateCreateWithData(nil, der as CFData)
        else {
            throw ImportFailure.notCertificate
        }
        guard (try? DER.isCertificateAuthority(der)) == true else {
            throw ImportFailure.notCertificateAuthority
        }
        // `openssl ecparam -genkey` writes an EC PARAMETERS block first; only the key block is handed on.
        guard let block = pemBlock(in: privateKeyPEM, labels: ["PRIVATE KEY", "EC PRIVATE KEY"]),
              let signing = try? P256.Signing.PrivateKey(pemRepresentation: block.text)
        else {
            throw ImportFailure.unsupportedKey
        }
        guard let publicKey = SecCertificateCopyKey(certificate),
              let publicPoint = SecKeyCopyExternalRepresentation(publicKey, nil) as Data?,
              publicPoint == signing.publicKey.x963Representation
        else {
            throw ImportFailure.keyMismatch
        }
        let key = try privateKey(fromRepresentation: signing.x963Representation)
        return try MitmCertificateAuthority(certificateDER: der, privateKey: key)
    }

    /// The first PEM block with one of `labels`: its text from BEGIN to END and its decoded bytes.
    static func pemBlock(in pem: String, labels: [String]) -> (text: String, der: Data)? {
        for label in labels {
            let begin = "-----BEGIN \(label)-----"
            let end = "-----END \(label)-----"
            guard let start = pem.range(of: begin), let stop = pem.range(of: end, range: start.upperBound ..< pem.endIndex) else {
                continue
            }
            let body = pem[start.upperBound ..< stop.lowerBound]
            guard let der = Data(base64Encoded: String(body), options: .ignoreUnknownCharacters), !der.isEmpty else {
                return nil
            }
            return (String(pem[start.lowerBound ..< stop.upperBound]), der)
        }
        return nil
    }
}

extension DER {
    /// Whether the certificate's basic constraints extension says `cA` is true.
    static func isCertificateAuthority(_ der: Data) throws -> Bool {
        var certificate = Reader(der)
        var tbs = try Reader(certificate.read(expecting: 0x30).content)
        var fields = try Reader(tbs.read(expecting: 0x30).content)
        if fields.peekTag == 0xA0 {
            _ = try fields.read(expecting: 0xA0)
        }
        _ = try fields.read(expecting: 0x02) // serial
        _ = try fields.read(expecting: 0x30) // signature algorithm
        _ = try fields.read(expecting: 0x30) // issuer
        _ = try fields.read(expecting: 0x30) // validity
        _ = try fields.read(expecting: 0x30) // subject
        _ = try fields.read(expecting: 0x30) // subject public key info
        while let tag = fields.peekTag {
            let element = try fields.read(expecting: tag)
            guard tag == 0xA3 else {
                continue // issuer and subject unique identifiers
            }
            var outer = Reader(element.content)
            var extensions = try Reader(outer.read(expecting: 0x30).content)
            let basicConstraints = oid(OID.basicConstraints)
            while extensions.peekTag != nil {
                var fields = try Reader(extensions.read(expecting: 0x30).content)
                let identifier = try fields.read(expecting: 0x06)
                guard identifier.encoded == basicConstraints else {
                    continue
                }
                if fields.peekTag == 0x01 {
                    _ = try fields.read(expecting: 0x01) // critical
                }
                var value = try Reader(fields.read(expecting: 0x04).content)
                var constraints = try Reader(value.read(expecting: 0x30).content)
                guard constraints.peekTag == 0x01 else {
                    return false // cA defaults to false
                }
                return try constraints.read(expecting: 0x01).content.first.map { $0 != 0 } ?? false
            }
        }
        return false
    }
}
