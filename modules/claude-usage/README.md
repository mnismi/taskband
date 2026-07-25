# claude-usage

Shows your Claude usage on the taskbar as two progress bars: the 5-hour session
window and the 7-day window, each with the percentage used and a countdown to
when it resets.

<img src="claude-usage-preview.png" alt="The module on the taskbar: 5H and 7D progress bars with percentages and reset countdowns">

```
5H [▪▪········]  20% · 3h 46m
7D [▪▪▪▪▪▪····]  61% · 2d 2h
```

A window with no active countdown shows `idle` in place of the reset time. The
API reports this with a null `resets_at`, typically for the 5-hour window when
no session is running.

## This uses an unofficial endpoint

This module does not use Anthropic's official API. It calls claude.ai's
internal web endpoint (`/api/organizations/<orgId>/usage`) with your own
browser session cookie, the same request the claude.ai page makes for itself.
Automating claude.ai outside its own interfaces may conflict with Anthropic's
consumer terms of service. The module is read-only, touches only your own
account, and makes one request per interval, but use it at your own risk: the
endpoint can change or disappear without notice, and the plausible worst case
is your session being invalidated or your account flagged.

## Requirements

Node.js 18 or newer, for the built-in `fetch`. Nothing else: no dependencies, no
install step.

## Setup

1. Copy the template:

   ```powershell
   Copy-Item config.example.json config.json
   ```

2. Get your credentials from a logged-in `claude.ai` tab:
   - Open `https://claude.ai`, then DevTools (F12) and the Network tab.
   - Reload the page, then click the request to
     `.../organizations/<orgId>/usage`.
   - `orgId` is the id in that URL.
   - Under Request Headers, copy the whole `cookie` value (it must contain
     `sessionKey=` and `cf_clearance=`) and the `user-agent` value.

3. Paste all three into `config.json`.

`config.json` is ignored by this folder's `.gitignore`. Never commit it: the
cookie is a live credential for your Claude account.

## Wiring it into Taskband

Add to your `config.json` (the Taskband one, beside `Taskband.exe`):

```json5
{
    "modules": ["claude", "clock"],

    "claude": {
        "exec": "node \"C:\\path\\to\\taskband\\modules\\claude-usage\\claude-usage.js\"",
        "interval": 60,
        "css": { "font-family": "Consolas", "font-size": "11px", "text-align": "left" }
    }
}
```

Three things matter here:

- **The path must be absolute.** Taskband runs modules through `cmd /C`, which
  inherits whatever working directory the exe was started from.
- **The font must be monospace.** In a proportional font the two bars will not
  line up under each other. Consolas and Cascadia Mono both work.
- **Keep `interval` at 60 or higher.** Each run is one request to claude.ai, and
  the countdown only advances a minute at a time anyway.

## What it shows when something is wrong

The module never goes blank. It prints one line instead:

| Line                   | Meaning                                                  |
| ---------------------- | -------------------------------------------------------- |
| `Claude: no config`    | `config.json` is missing, malformed, or has an empty key  |
| `Claude: auth expired` | The cookie is no longer valid; capture a fresh one        |
| `Claude: HTTP 500`     | claude.ai returned an unexpected status                   |
| `Claude: offline`      | No network, or the request passed its 5 second timeout    |
| `Claude: bad response` | The response was not the JSON this module expects         |

Run a debug build of Taskband to see the detailed reason on stderr, or run the
script directly:

```powershell
node claude-usage.js
```

`Claude: auth expired` is the one you should expect to see periodically. Claude
session cookies do not last forever; when it appears, repeat step 2 above.

## A note on timing

Taskband runs every module sequentially on a single worker thread, so a slow
module holds up the rest of the bar. This one waits up to 5 seconds for
claude.ai before giving up, which is why the timeout is short and the interval
is long.

## Tests

```powershell
node --test
```

The tests inject a fake sender, so they cover every HTTP and parsing branch
without touching the network or needing a `config.json`.
