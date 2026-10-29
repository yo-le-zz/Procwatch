# Architecture

`procwatch` is a library crate plus a thin binary. Everything that does not touch the operating system is portable and covered by tests; only the event collection is Windows-specific.

```text
 ETW session (Kernel-Process)  ─┐
 IP Helper notifications       ─┼─► EventSink (bounded channel) ─► Session ─► terminal
                                │                                    │   └──► log file
                                │                                    ▼
                                │                         TimedBuffer + ProcessTable
                                │                                    │
                                └─ Inspector (sysinfo) ◄── command line / initial snapshot
```

## Modules

| Module | Role |
| --- | --- |
| `platform::windows` | ETW session (`ferrisetw`), elevation check, network change notifications, `wevtutil` query. Only compiled on Windows. |
| `platform::unsupported` | Same API for other systems: refuses to start. Keeps the rest of the code buildable and testable anywhere. |
| `platform` (mod) | `EventSink`: bounded channel to the session; events are dropped and counted rather than growing memory when the consumer lags. |
| `event` | Event model (`Start`, `Stop`, `Network`) and `Record` (an event with its resolved parent name). |
| `session` | Core loop: filters, parent resolution, buffering, report building. Outputs and command line lookup are injected, so it is tested with synthetic events. |
| `matcher` | Case-insensitive name patterns with wildcards. |
| `table` | PID to name table, with a bounded memory of exited processes so a parent can still be named after it exits. |
| `buffer` | Rolling buffer bounded by time and by size. Pruning is relative to event time, which keeps tests deterministic. |
| `exit_code` | Classifies and describes exit codes (clean, error, crash, Ctrl+C). |
| `verdict` | Decides between crash, forced kill and normal exit; looks for a killing tool in the recent events. |
| `report` / `render` | Report layout; colored and plain rendering of the same lines. |
| `inspector` | `sysinfo`: initial process list and command line of a new process. |
| `config` | TOML configuration with validation and atomic saves. |
| `cli`, `menu`, `main` (binary) | Arguments, interactive menu, wiring and watch loop. |

## Event flow

1. `start_process_trace` opens a user-mode ETW session named `procwatch-<pid>` and enables `Microsoft-Windows-Kernel-Process` with the process keyword (`0x10`), keeping event IDs 1 (start) and 2 (stop). The callback runs on the ETW thread, extracts `ProcessID`, `ParentProcessID`, `ImageName`, `ExitCode`, stamps the time of reception and pushes to the sink.
2. The binary starts the session **before** taking the initial process snapshot, so a process launched in between is seen twice (harmless) rather than never.
3. `Session::handle` normalizes the image name, resolves the parent name from the table, hides ignored processes (counting them), reads the command line of visible new processes, prints the line and keeps it in the buffer.
4. When a target stops, the session freezes the context (events of the last `context_seconds`, before the stop itself), asks `verdict::analyze` for a verdict and prints the report.
5. After a crash, the binary waits five seconds (Windows Error Reporting writes its event late) and then queries the Application log.

## Design decisions

- **Reception time instead of ETW time**: the ETW timestamp clock depends on the session configuration; the reception delay is far below the precision needed here.
- **No shell interpolation**: `wevtutil` receives arguments built only from a number; the process name is only used to filter its output.
- **The log file is best-effort**: a failing log file never stops the watch; a failing terminal does (there is nothing left to display to).
- **Unsafe code** is limited to two spots in `platform::windows` (token elevation query, network notification callback), each with a `SAFETY` comment.
