# Privacy Manifest Data Inventory

Status: draft for the NetworkCore iOS app and tunnel extension. App Privacy answers still need manual confirmation before submission.

| Component | Collected data types | Tracking | Tracking domains |
| --- | --- | --- | --- |
| NetworkCoreApp | none | false | none |
| NetworkCorePacketTunnel | none | false | none |

No analytics, crash reporting, advertising or third-party SDK is linked. Any new data collection must update both `PrivacyInfo.xcprivacy` files and this inventory in the same commit.

The import sheet uses the camera (and a picture the user picks in Photos) only to read a node or subscription QR code
on the device. No frame or picture is saved or uploaded, so this is not data collection; `NSCameraUsageDescription`
in `project.yml` says so to the user.

The optional connection-quality report (`ClientReporter`, `ClientReportStore`; docs/network-behavior.md "连接质量报告")
is off by default. When the user turns it on, the app sends per-node dial counts of ANX nodes, the network type and
the app version about once an hour to the panel of the subscription that supplied the nodes, linked to that
subscription's token. It goes to the user's subscription provider, not to the developer, and is not used for tracking.
Whether this has to be declared (`NSPrivacyCollectedDataTypeOtherDiagnosticData`, linked, not tracking, app
functionality) in both `PrivacyInfo.xcprivacy` files and in the App Store Connect App Privacy answers is a pending
manual decision recorded in docs/manual-intervention.md ("连接质量报告"); until it is made, the manifests are unchanged.
