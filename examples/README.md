# Example modules

Ready-to-use Taskband modules. Each folder is self-contained: point a module's
`exec` at the script inside it.

| Module                          | Shows                                    | Needs                           |
| ------------------------------- | ---------------------------------------- | ------------------------------- |
| [claude-usage](claude-usage/)   | Claude 5-hour and 7-day usage, with bars | Node.js 18+, a claude.ai cookie |
| [memory](memory/)               | Physical memory in use                   | nothing                         |
| [network-speed](network-speed/) | Download and upload throughput           | nothing                         |

Modules with setup worth explaining have their own README. The rest carry their
config snippet in a comment at the top of the script.

## Wiring one up

Add a module definition to your Taskband `config.json` and list its name in
`modules`:

```json5
{
    "modules": ["memory", "clock"],

    "memory": {
        "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\taskband\\examples\\memory\\memory.ps1\"",
        "interval": 5
    }
}
```

## Things that catch people out

**Use absolute paths.** Taskband runs each `exec` through `cmd /C`, which
inherits whatever working directory `Taskband.exe` was started from. A relative
path works until you enable start-at-login, then silently stops.

**Escape the path in JSON.** Backslashes double up (`C:\\path\\to`) and inner
quotes need a backslash (`\"`). JSON5 comments and trailing commas are fine, but
these two rules are not relaxed.

**Modules share one worker thread.** Taskband runs due modules one after another
on a single thread, so a slow module delays every other module for as long as it
takes. `network-speed` costs about a second per run by design, and
`claude-usage` waits up to five seconds for the network. Give slow modules a
generous `interval`, and keep in mind that a one-second `clock` will not tick
smoothly while a slow module is running.

**Use a monospace font for anything with bars or columns.** In a proportional
font, stacked bar lines will not align.

**A module is arbitrary shell code.** It runs as you, every interval, and
Taskband reloads `config.json` the moment it changes. Read a module before you
point Taskband at it.
