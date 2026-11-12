// anixops-appcast: makes and checks the signed update feed of the macOS app (docs/macos-updates.md).
//
// Built only together with the app's own verifier, so it signs with exactly the code the app verifies with:
//
//   swiftc -O -o anixops-appcast \
//     apps/ios/Sources/NetworkCoreBridge/UpdateFeed.swift apps/ios/Tools/appcast/main.swift
//
// Subcommands (exit codes: 0 ok, 1 verification failed, 64 usage, 65 bad input data, 66 missing input file):
//
//   keygen [--out FILE]
//       New Ed25519 key. Writes the private key as PKCS#8 PEM (as `anixsign keygen` does) to FILE (mode 0600), or to
//       stdout when no FILE is given; prints the public key (for ANIXOPS_UPDATE_PUBLIC_KEYS) and its key id to stderr.
//       Private keys may also be given as a base64 raw seed; public keys as base64 raw or PEM SPKI.
//   item --product P --version V --build N --min-system 14.0 --url URL --file ARCHIVE [--notes-file F]
//       Prints the signed feed item of an archive as JSON.
//   feed --product P --channel C --sequence N --items ITEMS.json [--out FILE]
//       Signs a feed made of the items (a JSON array of items) and prints or writes the document to serve.
//   verify --public-keys K1[,K2] --product P --feed FILE [--last-sequence N] [--archive FILE --build N]
//       Checks a feed, and optionally an archive against the feed's item of that build, like the app does.
//
// `item` and `feed` read the private key from --key-file FILE or the ANIXOPS_UPDATE_SIGNING_KEY environment variable.

import Foundation

enum ExitCode: Int32 {
    case ok = 0
    case verificationFailed = 1
    case usage = 64
    case dataError = 65
    case noInput = 66
}

struct ToolError: Error {
    let code: ExitCode
    let message: String
}

func fail(_ code: ExitCode, _ message: String) -> Never {
    FileHandle.standardError.write(Data("anixops-appcast: \(message)\n".utf8))
    exit(code.rawValue)
}

struct Options {
    private var values: [String: String] = [:]

    init(_ arguments: ArraySlice<String>) throws {
        var iterator = arguments.makeIterator()
        while let flag = iterator.next() {
            guard flag.hasPrefix("--"), flag.count > 2 else {
                throw ToolError(code: .usage, message: "unexpected argument \(flag)")
            }
            guard let value = iterator.next() else {
                throw ToolError(code: .usage, message: "\(flag) needs a value")
            }
            values[String(flag.dropFirst(2))] = value
        }
    }

    func optional(_ name: String) -> String? {
        values[name]
    }

    func required(_ name: String) throws -> String {
        guard let value = values[name], !value.isEmpty else {
            throw ToolError(code: .usage, message: "missing --\(name)")
        }
        return value
    }

    func int(_ name: String) throws -> Int64 {
        guard let value = Int64(try required(name)) else {
            throw ToolError(code: .usage, message: "--\(name) must be a whole number")
        }
        return value
    }
}

func read(_ path: String) throws -> Data {
    guard let data = FileManager.default.contents(atPath: path) else {
        throw ToolError(code: .noInput, message: "cannot read \(path)")
    }
    return data
}

func makeSigner(_ options: Options) throws -> UpdateSigner {
    let text: String
    if let path = options.optional("key-file") {
        text = String(decoding: try read(path), as: UTF8.self)
    } else if let value = ProcessInfo.processInfo.environment["ANIXOPS_UPDATE_SIGNING_KEY"], !value.isEmpty {
        text = value
    } else {
        throw ToolError(code: .usage, message: "give --key-file or set ANIXOPS_UPDATE_SIGNING_KEY")
    }
    do {
        return try UpdateSigner(privateKey: text)
    } catch {
        throw ToolError(code: .dataError, message: "the signing key is not an Ed25519 PKCS#8 PEM key or base64 seed")
    }
}

func output(_ data: Data, to path: String?) throws {
    if let path {
        guard FileManager.default.createFile(atPath: path, contents: data) else {
            throw ToolError(code: .dataError, message: "cannot write \(path)")
        }
    } else {
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
    }
}

func runKeygen(_ options: Options) throws {
    let (seed, publicKey) = UpdateSigner.generate()
    // PKCS#8 PEM, the same format as anix-kit's `anixsign keygen`.
    let privateKey = try UpdateSigner.pem(privateKey: seed)
    let keyID = try UpdateSigner(privateKey: seed).keyID
    if let path = options.optional("out") {
        guard FileManager.default.createFile(
            atPath: path,
            contents: Data(privateKey.utf8),
            attributes: [.posixPermissions: 0o600]
        ) else {
            throw ToolError(code: .dataError, message: "cannot write \(path)")
        }
    } else {
        print(privateKey, terminator: "")
    }
    FileHandle.standardError.write(Data("public key: \(publicKey)\nkey id: \(keyID)\n".utf8))
}

func runItem(_ options: Options) throws {
    let signer = try makeSigner(options)
    let product = try options.required("product")
    let build = Int(try options.int("build"))
    let archive = URL(fileURLWithPath: try options.required("file"))
    guard let url = URL(string: try options.required("url")) else {
        throw ToolError(code: .usage, message: "--url is not a URL")
    }
    let notes = try options.optional("notes-file").map { String(decoding: try read($0), as: UTF8.self) }
    let digest: (length: Int64, sha256: String)
    do {
        digest = try UpdateSigning.digest(of: archive)
    } catch {
        throw ToolError(code: .noInput, message: "cannot read \(archive.path)")
    }
    let item = UpdateItem(
        version: try options.required("version"),
        build: build,
        minimumSystemVersion: try options.required("min-system"),
        url: url,
        length: digest.length,
        sha256: digest.sha256,
        signature: try signer.archiveSignature(
            product: product, build: build, length: digest.length, sha256: digest.sha256
        ),
        notes: notes,
        publishedAt: Date()
    )
    try output(try UpdateSigning.encoder().encode(item), to: options.optional("out"))
}

func runFeed(_ options: Options) throws {
    let signer = try makeSigner(options)
    let items: [UpdateItem]
    do {
        items = try UpdateSigning.decoder().decode([UpdateItem].self, from: try read(options.required("items")))
    } catch let error as ToolError {
        throw error
    } catch {
        throw ToolError(code: .dataError, message: "--items must be a JSON array of feed items")
    }
    let feed = UpdateFeed(
        product: try options.required("product"),
        channel: try options.required("channel"),
        sequence: try options.int("sequence"),
        generatedAt: Date(),
        items: items
    )
    try output(try signer.sign(feed), to: options.optional("out"))
}

func runVerify(_ options: Options) throws {
    let keys = try options.required("public-keys").split(separator: ",").map(String.init)
    let verifier: UpdateVerifier
    do {
        verifier = try UpdateVerifier(product: try options.required("product"), publicKeys: keys)
    } catch let error as ToolError {
        throw error
    } catch {
        throw ToolError(code: .usage, message: "--public-keys must be base64 Ed25519 public keys")
    }
    var last: Int64?
    if options.optional("last-sequence") != nil {
        last = try options.int("last-sequence")
    }
    let feed: UpdateFeed
    do {
        feed = try verifier.verifyFeed(try read(options.required("feed")), lastAccepted: last)
    } catch let error as ToolError {
        throw error
    } catch {
        throw ToolError(code: .verificationFailed, message: "feed refused: \(error)")
    }
    print("feed ok: \(feed.product) \(feed.channel) sequence \(feed.sequence), \(feed.items.count) item(s); trusted key ids \(verifier.keyIDs.joined(separator: ","))")
    guard let archive = options.optional("archive") else {
        return
    }
    let build = Int(try options.int("build"))
    guard let item = feed.items.first(where: { $0.build == build }) else {
        throw ToolError(code: .verificationFailed, message: "the feed has no item for build \(build)")
    }
    do {
        try verifier.verifyArchive(at: URL(fileURLWithPath: archive), item: item)
    } catch {
        throw ToolError(code: .verificationFailed, message: "archive refused: \(error)")
    }
    print("archive ok: build \(build), \(item.length) bytes, sha256 \(item.sha256)")
}

let arguments = CommandLine.arguments.dropFirst()
guard let command = arguments.first else {
    fail(.usage, "usage: anixops-appcast keygen|item|feed|verify [options]; see the header of Tools/appcast/main.swift")
}
do {
    let options = try Options(arguments.dropFirst())
    switch command {
    case "keygen":
        try runKeygen(options)
    case "item":
        try runItem(options)
    case "feed":
        try runFeed(options)
    case "verify":
        try runVerify(options)
    default:
        throw ToolError(code: .usage, message: "unknown subcommand \(command)")
    }
} catch let error as ToolError {
    fail(error.code, error.message)
} catch {
    fail(.dataError, "\(error)")
}
