# Troubleshooting

## "droits administrateur requis"

ETW sessions need elevation. Open *Terminal* or *PowerShell* with **Run as administrator** and start `procwatch` from there.

## "démarrage de la capture ETW impossible"

- Another tool may hold an ETW session with a clashing name; names are `procwatch-<pid>` and unique per run.
- After a hard kill (the process was terminated without Ctrl+C) a session can stay alive. List and stop it:

  ```powershell
  logman query -ets
  logman stop procwatch-<pid> -ets
  ```

## Nothing is displayed

- Check that the target name matches: `procwatch --no-menu -t steam* --show-ignored`.
- A target is matched on the file name of the executable, not on the window title.
- If the process starts and stops before `procwatch` is running, there is nothing to report: start `procwatch` first.

## The report names no culprit

See *Limits you should know about* in the README: ETW does not identify the caller of `TerminateProcess`. Killing from the Task Manager shows as code `1` with no culprit. If you suspect a specific tool, run `procwatch --show-ignored --context 120` and look at what started just before.

## Garbled colors

Windows Terminal and recent consoles are fine. In a console without ANSI support, use `--no-color` or set `NO_COLOR=1`. Output sent to a file or a pipe is never colored.

## Accented characters look wrong in the event log excerpt

The excerpt comes from `wevtutil`, which writes in the console code page. The text is still readable; use `wevtutil` directly if you need the exact text.

## "la configuration ... invalide"

The configuration file has a syntax error or an unknown key; the message names the file and the key. Fix it, or delete the file to go back to the defaults.
