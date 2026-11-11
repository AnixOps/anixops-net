import Foundation
import NetworkExtension

// Entry point of the macOS packet tunnel, which runs as a System Extension in Developer ID builds: macOS starts
// this process and creates the provider class named under NetworkExtension > NEProviderClasses in Info.plist.
autoreleasepool {
    NEProvider.startSystemExtensionMode()
}
dispatchMain()
