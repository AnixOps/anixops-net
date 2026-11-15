import CryptoKit
@testable import NetworkCoreBridge
import XCTest

final class MitmCertificateImportTests: XCTestCase {
    private func pem(_ der: Data, label: String) -> String {
        "-----BEGIN \(label)-----\n" + der.base64EncodedString(options: .lineLength64Characters) + "\n-----END \(label)-----\n"
    }

    private func keyPEM(_ authority: MitmCertificateAuthority) throws -> String {
        try P256.Signing.PrivateKey(x963Representation: authority.privateKeyRepresentation()).pemRepresentation
    }

    func testOwnRootImportsWithPkcs8Key() throws {
        let team = try MitmCertificateAuthority.generate(commonName: "Team Debug CA")

        let imported = try MitmCertificateAuthority.importing(
            certificatePEM: pem(team.certificateDER, label: "CERTIFICATE"),
            privateKeyPEM: try keyPEM(team)
        )

        XCTAssertEqual(imported.sha256Fingerprint, team.sha256Fingerprint)
        XCTAssertEqual(imported.commonName, "Team Debug CA")
        XCTAssertEqual(try imported.privateKeyRepresentation(), try team.privateKeyRepresentation())
        // Leaves issued from the imported root verify against the team's certificate.
        XCTAssertEqual(imported.trustState(anchors: [team.certificate]), .trusted)
    }

    func testKeyAfterParametersBlockImports() throws {
        let team = try MitmCertificateAuthority.generate()
        let pkcs8PEM = try keyPEM(team)
        // `openssl ecparam -genkey` output: an EC PARAMETERS block, then the key.
        let parameters = pem(DER.oid(DER.OID.prime256v1), label: "EC PARAMETERS")

        XCTAssertNoThrow(try MitmCertificateAuthority.importing(
            certificatePEM: pem(team.certificateDER, label: "CERTIFICATE"),
            privateKeyPEM: parameters + pkcs8PEM
        ))
    }

    func testRefusesKeyOfAnotherRoot() throws {
        let team = try MitmCertificateAuthority.generate()
        let other = try MitmCertificateAuthority.generate()

        XCTAssertThrowsError(try MitmCertificateAuthority.importing(
            certificatePEM: pem(team.certificateDER, label: "CERTIFICATE"),
            privateKeyPEM: try keyPEM(other)
        )) { error in
            XCTAssertEqual(error as? MitmCertificateAuthority.ImportFailure, .keyMismatch)
        }
    }

    func testRefusesLeafCertificate() throws {
        let team = try MitmCertificateAuthority.generate()
        let leaf = try team.issueLeaf(host: "example.com")
        let leafKey = try XCTUnwrap(SecKeyCopyExternalRepresentation(leaf.privateKey, nil) as Data?)

        XCTAssertThrowsError(try MitmCertificateAuthority.importing(
            certificatePEM: pem(leaf.certificateDER, label: "CERTIFICATE"),
            privateKeyPEM: try P256.Signing.PrivateKey(x963Representation: leafKey).pemRepresentation
        )) { error in
            XCTAssertEqual(error as? MitmCertificateAuthority.ImportFailure, .notCertificateAuthority)
        }
        XCTAssertEqual(try DER.isCertificateAuthority(team.certificateDER), true)
        XCTAssertEqual(try DER.isCertificateAuthority(leaf.certificateDER), false)
    }

    func testRefusesTextThatIsNotPem() throws {
        let team = try MitmCertificateAuthority.generate()

        XCTAssertThrowsError(try MitmCertificateAuthority.importing(certificatePEM: "hello", privateKeyPEM: try keyPEM(team))) { error in
            XCTAssertEqual(error as? MitmCertificateAuthority.ImportFailure, .notCertificate)
        }
        XCTAssertThrowsError(try MitmCertificateAuthority.importing(
            certificatePEM: pem(team.certificateDER, label: "CERTIFICATE"),
            privateKeyPEM: "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----"
        )) { error in
            XCTAssertEqual(error as? MitmCertificateAuthority.ImportFailure, .unsupportedKey)
        }
    }
}
