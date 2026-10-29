# Configuration reference

`procwatch` reads a TOML file. The default location is:

- Windows: `%APPDATA%\procwatch\config.toml`
- Elsewhere (development only): `$XDG_CONFIG_HOME/procwatch/config.toml` or `~/.config/procwatch/config.toml`

Use `--config <FILE>` to choose another file. A missing file means "all defaults". A file that cannot be parsed is an **error** (it is never silently replaced by defaults). Unknown keys are rejected so that typos are noticed. The menu entry *Enregistrer la configuration* writes the file atomically (temporary file, then rename).

Command line options are applied on top of the file; see the README.

## Keys

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `targets` | list of strings | `[]` | Process names to watch. At least one is required to start. |
| `ignore` | list of strings | see below | Process names to hide. A target is never hidden, even if it matches. |
| `context_seconds` | integer, 1 to 600 | `30` | History printed when a target ends, and window used to look for the culprit. |
| `buffer_capacity` | integer, 100 to 1,000,000 | `5000` | Maximum number of events kept in memory for that history. |
| `show_ignored` | boolean | `false` | Show the ignored processes too. |
| `show_network` | boolean | `true` | Show network interface changes. Repeated identical notifications within one second are merged. |
| `query_event_log` | boolean | `true` | After a crash, look up the Windows Application event log (events 1000 and 1001). |
| `log_file` | string path | none | Append a plain-text copy (no colors) of the output to this file. If writing fails, the file log is disabled with a warning and the watch continues. |

Default `ignore` list: `NetCfgNotifyObjectHost.exe`, `conhost.exe`, `RuntimeBroker.exe`, `backgroundTaskHost.exe`, `taskhostw.exe`, `WmiPrvSE.exe`.

## Name patterns

Used by `targets` and `ignore`:

- case-insensitive;
- a path is reduced to its file name and a trailing `.exe` is optional (`steam`, `Steam.exe` and `C:\...\steam.exe` are the same);
- `*` at the start and/or the end is a wildcard: `steam*` (prefix), `*helper` (suffix), `*node*` (contains);
- empty patterns, or a lone `*`, are ignored: a pattern that matched everything would never be what you want.

## Example

See [`config.example.toml`](../config.example.toml); a test checks that this file stays valid.
