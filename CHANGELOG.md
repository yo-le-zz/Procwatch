# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-10-29

### Added
- Real-time capture of process starts and stops through an ETW session on `Microsoft-Windows-Kernel-Process`.
- Interactive menu (inquire) to choose the target processes, the ignore list and the options, with a picker over the running processes and persistent configuration.
- Colored terminal output: one color per kind of ending (clean exit, error, crash), dedicated tags for target events, millisecond timestamps and parent process on every start.
- Exit code interpretation that tells a real crash (`0xC0000005`, ...) from a forced kill (`taskkill /F`, code `1`).
- End-of-target report: lifetime, verdict, the tool that killed the process when it can be identified, and the events of the last seconds (rolling buffer).
- Noise filter (ignore list with `*` wildcards) with a per-report summary of what was hidden.
- Network interface change notifications (added, removed, modified).
- Delayed lookup of the Windows Application event log (events 1000/1001) after a crash.
- Plain-text log file, command line options, `NO_COLOR` support and an offline `replay` example.
