import AVFoundation
import CoreImage
import CoreImage.CIFilterBuiltins
import PhotosUI
import SwiftUI
import UIKit

/// QR codes for node and subscription links: reading them from the camera or a saved picture, and drawing one to
/// share. Everything stays on the device; a scanned link goes through the same import preview as pasted text.
enum QRCode {
    /// The text of the first QR code in a picture, such as a screenshot of a provider's page.
    static func message(in image: UIImage) -> String? {
        guard let ciImage = image.ciImage ?? image.cgImage.map({ CIImage(cgImage: $0) }),
              let detector = CIDetector(
                  ofType: CIDetectorTypeQRCode,
                  context: nil,
                  options: [CIDetectorAccuracy: CIDetectorAccuracyHigh]
              )
        else {
            return nil
        }
        return detector.features(in: ciImage)
            .compactMap { ($0 as? CIQRCodeFeature)?.messageString }
            .first { !$0.isEmpty }
    }

    /// A sharp QR code for `text`, or nil when it is too long for one code.
    static func image(for text: String, scale: CGFloat = 10) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: scale, y: scale)),
              let cgImage = CIContext().createCGImage(output, from: output.extent)
        else {
            return nil
        }
        return UIImage(cgImage: cgImage)
    }
}

/// The full-screen scanner sheet: the camera with a viewfinder, a button to read a code from Photos instead, and a
/// note when the camera is not allowed.
struct QRScannerSheet: View {
    let found: (String) -> Void
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var access = AVCaptureDevice.authorizationStatus(for: .video)
    @State private var photo: PhotosPickerItem?
    @State private var error: String?

    var body: some View {
        NavigationStack {
            ZStack {
                Color.black.ignoresSafeArea()
                switch access {
                case .authorized:
                    QRCameraView { text in
                        finish(text)
                    }
                    .ignoresSafeArea()
                    RoundedRectangle(cornerRadius: 24, style: .continuous)
                        .strokeBorder(Color.white.opacity(0.85), lineWidth: 3)
                        .frame(width: 240, height: 240)
                        .accessibilityHidden(true)
                case .notDetermined:
                    ProgressView()
                        .tint(.white)
                default:
                    VStack(spacing: 12) {
                        Image(systemName: "camera.fill")
                            .font(.largeTitle)
                        Text("没有相机权限")
                            .font(.headline)
                        Text("可以在“设置”里允许 AnixOps Net 使用相机，或者从相册里的截图识别二维码。")
                            .font(.subheadline)
                            .multilineTextAlignment(.center)
                        if let settings = URL(string: UIApplication.openSettingsURLString) {
                            Link("打开设置", destination: settings)
                                .font(.body.weight(.semibold))
                        }
                    }
                    .foregroundStyle(.white)
                    .padding(32)
                }
                VStack {
                    Spacer()
                    if let error {
                        Text(error)
                            .font(.subheadline)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(.black.opacity(0.6), in: Capsule())
                    }
                    PhotosPicker(selection: $photo, matching: .images) {
                        Label("从相册识别", systemImage: "photo")
                            .font(.body.weight(.semibold))
                            .padding(.horizontal, 20)
                            .frame(height: 44)
                            .background(.ultraThinMaterial, in: Capsule())
                    }
                    .padding(.bottom, 32)
                }
            }
            .navigationTitle("扫描二维码")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarColorScheme(.dark, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
            }
            .task {
                if access == .notDetermined {
                    _ = await AVCaptureDevice.requestAccess(for: .video)
                    access = AVCaptureDevice.authorizationStatus(for: .video)
                }
            }
            .onChange(of: photo) { _, item in
                guard let item else {
                    return
                }
                Task {
                    await read(item)
                }
            }
        }
    }

    private func read(_ item: PhotosPickerItem) async {
        photo = nil
        guard let data = try? await item.loadTransferable(type: Data.self),
              let image = UIImage(data: data)
        else {
            error = String(localized: "无法读取这张图片")
            return
        }
        guard let text = QRCode.message(in: image) else {
            error = String(localized: "图片里没有找到二维码")
            return
        }
        finish(text)
    }

    private func finish(_ text: String) {
        found(text)
        dismiss()
    }
}

/// The camera preview that reports the first QR code it sees, once.
private struct QRCameraView: UIViewControllerRepresentable {
    let found: (String) -> Void

    func makeUIViewController(context: Context) -> Controller {
        Controller(found: found)
    }

    func updateUIViewController(_ controller: Controller, context: Context) {}

    final class Controller: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
        private let session = AVCaptureSession()
        private let found: (String) -> Void
        private var reported = false
        private var preview: AVCaptureVideoPreviewLayer?

        init(found: @escaping (String) -> Void) {
            self.found = found
            super.init(nibName: nil, bundle: nil)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("init(coder:) is not used")
        }

        override func viewDidLoad() {
            super.viewDidLoad()
            view.backgroundColor = .black
            guard let camera = AVCaptureDevice.default(for: .video),
                  let input = try? AVCaptureDeviceInput(device: camera),
                  session.canAddInput(input)
            else {
                return
            }
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            guard session.canAddOutput(output) else {
                return
            }
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
            let layer = AVCaptureVideoPreviewLayer(session: session)
            layer.videoGravity = .resizeAspectFill
            view.layer.addSublayer(layer)
            preview = layer
        }

        override func viewDidLayoutSubviews() {
            super.viewDidLayoutSubviews()
            preview?.frame = view.bounds
        }

        override func viewWillAppear(_ animated: Bool) {
            super.viewWillAppear(animated)
            let session = session
            DispatchQueue.global(qos: .userInitiated).async {
                session.startRunning()
            }
        }

        override func viewWillDisappear(_ animated: Bool) {
            super.viewWillDisappear(animated)
            let session = session
            DispatchQueue.global(qos: .userInitiated).async {
                session.stopRunning()
            }
        }

        // The delegate queue is `.main`, so the callback already runs on the main actor.
        nonisolated func metadataOutput(
            _ output: AVCaptureMetadataOutput,
            didOutput metadataObjects: [AVMetadataObject],
            from connection: AVCaptureConnection
        ) {
            let text = metadataObjects
                .compactMap { ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }
                .first { !$0.isEmpty }
            MainActor.assumeIsolated {
                report(text)
            }
        }

        private func report(_ text: String?) {
            guard !reported, let text else {
                return
            }
            reported = true
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            found(text)
        }
    }
}

/// A link shown as a QR code to scan on another device, with the system share sheet for the link itself. The link
/// carries the node's password, so the sheet says so and the code is only drawn when the user opens it.
struct QRShareSheet: View {
    let title: String
    let link: String
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var copied = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 20) {
                    if let image = QRCode.image(for: link) {
                        Image(uiImage: image)
                            .interpolation(.none)
                            .resizable()
                            .scaledToFit()
                            .padding(16)
                            .background(Color.white, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                            .frame(maxWidth: 300)
                            .accessibilityLabel("二维码")
                    } else {
                        Label("链接太长，无法生成二维码，请复制或分享链接。", systemImage: "exclamationmark.triangle")
                            .font(.subheadline)
                            .foregroundStyle(brand.text2)
                    }
                    Text(title)
                        .font(.headline)
                        .foregroundStyle(brand.text)
                    Label("链接里含有节点密码，只分享给信任的人。", systemImage: "lock")
                        .font(.footnote)
                        .foregroundStyle(brand.text2)
                    HStack(spacing: 12) {
                        Button {
                            UIPasteboard.general.string = link
                            copied = true
                        } label: {
                            Label(copied ? "已复制" : "复制链接", systemImage: copied ? "checkmark" : "doc.on.doc")
                                .frame(maxWidth: .infinity)
                                .frame(height: 44)
                        }
                        .buttonStyle(.bordered)
                        ShareLink(item: link) {
                            Label("分享", systemImage: "square.and.arrow.up")
                                .frame(maxWidth: .infinity)
                                .frame(height: 44)
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                .padding(24)
            }
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("导出")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("完成") {
                        dismiss()
                    }
                }
            }
        }
    }
}
