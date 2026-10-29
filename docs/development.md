# Development

## Build and checks

```powershell
cargo build
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The portable logic builds and is tested on Linux and macOS too; only `procwatch::platform::windows` needs Windows. The `replay` example runs anywhere and needs no admin rights:

```powershell
cargo run --example replay
```

## Tests

- Unit tests live next to the code (matching, exit codes, buffers, verdicts, rendering, configuration, CLI parsing).
- `tests/session.rs` runs end-to-end scenarios of the session with synthetic events: forced kill attributed to its tool, crash, clean exit, noise filtering, network bursts, color only on the terminal, failing log file, and so on.
- The interactive menu and the live ETW capture are not covered by automated tests; they need a terminal and an elevated Windows session.

## Verification status of version 1.0.0

Checked from a Linux environment:

- formatting, `clippy -D warnings` and the whole test suite pass on Linux;
- the Windows code (ETW session, elevation check, network notifications, event log query) type-checks, passes `clippy -D warnings` and **links** into `procwatch.exe` for the `x86_64-pc-windows-gnu` target.

Not checked: running the binary on a live Windows machine, so the real ETW payloads (field names `ProcessID`, `ParentProcessID`, `ImageName`, `ExitCode` of `Microsoft-Windows-Kernel-Process`) and the interactive menu have not been exercised there. The CI workflow builds and tests on `windows-latest` with the MSVC toolchain. If a field cannot be read on your Windows build, the corresponding event is dropped rather than guessed (a crash is never shown as a clean exit) and is counted as `abandonné(s)` in the final summary. A first run that shows no events, or a summary with many dropped events, means this needs attention; please open an issue with your Windows version.

## Releasing

1. Update the version in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. Tag the commit `vX.Y.Z` and push the tag. The release workflow checks that the tag matches `Cargo.toml`, runs format, lint and tests, builds, packages `procwatch-vX.Y.Z-windows-x64.zip` and publishes a GitHub release with the changelog section as notes.

## Conventions

- Code comments are in French, documentation in English.
- Keep the portable logic free of Windows calls; anything platform specific goes through `procwatch::platform`.
