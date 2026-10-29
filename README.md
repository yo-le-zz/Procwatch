# procwatch

Real-time Windows process watcher built on ETW (Event Tracing for Windows). It tells you **what started around the moment a program died, and whether that program crashed, was killed by another tool, or exited normally**.

It was written to track down a program that kept disappearing a few minutes after launch: the useful clue turned out to be a `taskkill /F /PID …` started by another application just before.

> The interface (menu and messages) is in French. The documentation is in English.

## Features

- **Live feed of every process start and stop**, with millisecond timestamps, the parent process and the command line.
- **Interactive menu** ([inquire](https://crates.io/crates/inquire)): choose the target(s), the processes to hide, the options, or pick them from the running processes. The configuration is saved to a TOML file.
- **Colors by outcome**: green for starts, cyan for a clean exit, yellow for an error exit, red for a crash, magenta for your targets.
- **Crash or kill?** Exit codes are interpreted (`0xC0000005` access violation, `0xC0000409` fail-fast, `1` forced kill…).
- **End-of-target report**: lifetime, verdict, the tool that killed it when it can be identified, and everything that happened in the last seconds (rolling buffer, 30 s by default).
- **Noise filter**: an ignore list with `*` wildcards; the report tells you how many events were hidden.
- **Network changes** (adapter added, removed or modified), useful when VPNs and virtual adapters are involved.
- **Windows event log lookup** a few seconds after a crash (Application events 1000/1001).
- Plain-text log file, `NO_COLOR` support, non-interactive mode for scripts.

## Example

`cargo run --example replay` replays a recorded scenario without admin rights. Output:

```text
20:21:13.604  START    Fleasion.exe (pid 200)  parent=explorer.exe(100)
20:21:15.604  CIBLE>   RobloxPlayerBeta.exe (pid 4432)  parent=Fleasion.exe(200)
20:23:18.604  START    taskkill.exe (pid 20112)  parent=Fleasion.exe(200)  taskkill /F /PID 4432
20:23:19.204  CIBLE<   RobloxPlayerBeta.exe (pid 4432)  0x1 (erreur générique ou arrêt forcé : taskkill /F, TerminateProcess)
RAPPORT  ===== Fin de la cible RobloxPlayerBeta.exe (pid 4432) =====
         Durée de vie : 2 min 03 s
         Verdict      : ARRÊT FORCÉ par taskkill.exe (pid 20112, lancé par Fleasion.exe) : 0x1 (...)
         Commande     : taskkill /F /PID 4432
         Contexte     : 1 événement(s) sur les 30 s précédentes ; 40 masqué(s) : NetCfgNotifyObjectHost.exe x40
| 20:23:18.604  START    taskkill.exe (pid 20112)  parent=Fleasion.exe(200)  taskkill /F /PID 4432
RAPPORT  ===== Fin du rapport =====
```

`CIBLE>` and `CIBLE<` mark the start and the end of a target.

## Requirements

- Windows 10 or 11, 64-bit.
- **Administrator rights**: opening an ETW kernel-process session requires them (running as a member of the *Performance Log Users* group may also work, but is not covered here).
- Rust 1.95 or newer to build from source.

## Installation

From a release: download the asset from the Releases page, extract it, and run `procwatch.exe` from an **elevated** terminal.

From source:

```powershell
cargo build --release
# binary: target\release\procwatch.exe
```

## Usage

Run from an elevated terminal. Without arguments the menu starts:

```powershell
procwatch
```

Or start directly:

```powershell
procwatch --no-menu --target RobloxPlayerBeta --log watch.log
procwatch --no-menu -t "roblox*,steam" -i discord.exe --context 60
```

| Option | Meaning |
| --- | --- |
| `-t, --target <NAME>` | Process to watch. Repeatable or comma-separated; replaces the configured targets. |
| `-i, --ignore <NAME>` | Extra process to hide; added to the configured ignore list. |
| `--config <FILE>` | Configuration file (default `%APPDATA%\procwatch\config.toml`). |
| `--no-menu` | Skip the menu (also skipped when the input or output is not a terminal). |
| `--log <FILE>` | Append a plain-text copy of the output to this file. |
| `--context <SECONDS>` | History shown in the report (1 to 600). |
| `--no-network` | Do not show network interface changes. |
| `--no-event-log` | Do not query the Windows event log after a crash. |
| `--show-ignored` | Also show the processes of the ignore list. |
| `--no-color` | Disable colors (`NO_COLOR` is honored too). |

Press `Ctrl+C` to stop; a summary of the session is printed.

Names are case-insensitive, the `.exe` suffix is optional and `*` can start and/or end a pattern (`steam*`, `*helper`, `*node*`).

## Reading the verdict

| Verdict | What it means |
| --- | --- |
| `PLANTAGE` | The exit code is an unhandled exception (`0xC…`, `0xE…`). A real crash. |
| `ARRÊT FORCÉ par <tool>` | Exit code other than 0 and a tool such as `taskkill` was started just before with the target's PID or name on its command line. |
| `Fin anormale sans plantage identifié` | Non-zero code, no crash signature, no killer found. Code `1` is typically a forced kill or a generic error. |
| `Fermeture demandée par <tool>` | A closing tool targeted the process and it exited with 0. |
| `Sortie normale` | Exit code 0 and nothing suspicious. |

### Limits you should know about

- ETW does not say **who** called `TerminateProcess`. A culprit is named only when a helper such as `taskkill`, `pskill`, `wmic` or PowerShell was started with the target's PID or name on its command line. Killing from the Task Manager, or from a program calling the API directly, shows up as a code `1` with no culprit.
- The command line is read right after the start event. A process that lives only a few milliseconds may already be gone, so it shows no command line (and cannot be named as a culprit).
- Timestamps are the moment `procwatch` received the event, a fraction of a millisecond after the real one.
- If the event queue saturates, extra events are dropped and counted in the final summary instead of exhausting memory.
- Windows only: on other systems the tool refuses to start (the portable part is only there for development and tests).

## Documentation

- [Configuration reference](docs/configuration.md)
- [Architecture](docs/architecture.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Development and verification status](docs/development.md)
- [Changelog](CHANGELOG.md)

## License

[MIT](LICENSE), copyright 2026 yolezz.
