# R28 Core snapshot browser evidence

The `default-*` captures come from `r28-snapshots-core-0.3.0` and the `rc1-*` captures come from `r28-snapshots-core-0.3.1-rc.1` in [Core run 46096155351](https://github.com/AnixOps/AnixOps-core/actions/runs/46096155351). `SHA256SUMS` covers the exact downloaded status bytes and provenance. The Core harness passes synthetic Rust inputs to the production `snapshot()` serializer. It uses no live peers.

The Net workflow sends the snapshots unchanged to `client_view::invoke`. Chromium then loads the built desktop UI with those Rust view replies. Only unrelated application-shell commands use the standard UI demo backend. The expectations are separate from the inputs.

- `default-joined` and `rc1-joined` verify address-only member and local-device copy. Neither Core snapshot supplies a member domain or `duplicateOf`.
- `rc1-office-*` and `rc1-paris-*` contain real `names[]` with no matching peer cards. They verify that records do not create member rows or attach a domain to the local device. The `cam.lon` panel record is not fully answerable. It retains `duplicate: true` when `duplicateNetworks` disappears.
- `synthetic-members` supplies matching member rows to test full domain copy, a standalone duplicate bit, record collisions across regions within one network, and cross-network warnings. It is a client regression input and does not stand in for a Core output.

The projection does not identify a device from a panel alias or a bare address alone. Across-region warnings describe records and do not establish DNS short-name precedence, OS resolver behavior, or a live tunnel path.
