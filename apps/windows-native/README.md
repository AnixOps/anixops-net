# AnixOps Net for Windows (native)

The Windows client written with WinUI 3 and C# (`docs/architecture/desktop-native.md`). It runs without
administrator rights and talks to the system service `AnixOpsNetCore` (`apps/windows-service`) over a named
pipe.

- `src/AnixOps.Net.Ipc`: the window's end of the channel to the service. The wire format is fixed by the
  samples in `crates/client-ipc/fixtures`, which the tests here read too.
- `tests/`: unit tests, run by the "Windows native app" job in CI.

Like the rest of the repository, this is built and tested in GitHub Actions only.
