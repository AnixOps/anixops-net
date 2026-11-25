import SwiftUI

/// 组网 on the Mac (docs/mesh-ui.md): the same page as on iOS, in the window's detail column, with members, subnets
/// and paths, and the network's 更多 menu. The Mac's own 本机 segment (relay, host mode, advertising) comes with
/// kernel 0.4.
struct MacMeshPage: View {
    var tunnel: TunnelController
    @State private var model = MeshModel()

    var body: some View {
        NavigationStack {
            MeshPage(tunnel: tunnel, model: model)
        }
    }
}
