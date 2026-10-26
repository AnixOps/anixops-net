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
