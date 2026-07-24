<p align="center">
  <img src="assets/taskband-logo-1024.png" width="128" alt="Taskband logo">
</p>

<h1 align="center">Taskband</h1>

<p align="center">Highly customizable Windows taskbar with custom plugins.</p>

<p align="center">
  <a href="https://github.com/mnismi/taskband/actions/workflows/ci.yml"><img src="https://github.com/mnismi/taskband/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
</p>

Taskband puts your own status widgets right on the Windows taskbar: CPU
usage, a clock, or the output of any command you choose. You describe each
widget in a simple config file, and Taskband keeps it updated on the bar. If
you've used [Waybar](https://github.com/Alexays/Waybar) on Linux and miss it
on Windows, this is for you.

## Features

- **Modules on the real taskbar**: no floating overlay window; bars embed
  into the taskbar itself, on every monitor that has one
- **Anything is a module**: a module is just a command (`exec`) run on an
  `interval`; its output is rendered on the bar, multi-line output included
- **Per-monitor routing**: send different modules to different monitors
- **CSS-like styling**: global defaults plus per-module overrides for color,
  background, font, padding, margin, and text alignment; named classes let a
  module restyle itself, or parts of a line, based on what it reports
- **JSON5 config with live reload**: comments and trailing commas allowed;
  edits apply instantly, no restart
- **System tray**: reload config, edit config, toggle start-at-login, quit
- **Single self-contained `.exe`**: a default config is baked in, so the
  binary runs on its own

## Installation

Download `Taskband.exe` from the
[latest release](https://github.com/mnismi/taskband/releases/latest) and run it.
An icon appears in the system tray; right-click it to manage Taskband.

The binary is not code-signed, so Windows SmartScreen shows a "Windows
protected your PC" warning the first time you run it. Choose **More info**
then **Run anyway**. If you would rather not trust a prebuilt binary, build
it yourself with the steps below.

Or build from source:

```
git clone https://github.com/mnismi/taskband.git
cd taskband
cargo build --release
```

The binary lands at `target/release/Taskband.exe`.

## Configuration

Taskband looks for `config.json` next to `Taskband.exe` first, then in the
current working directory. If neither exists it uses the built-in default;
the tray's **Edit config** writes that default out beside the exe so you can
customize it. The file is watched; saving it reloads the bar live.

The format is [JSON5](https://json5.org/), so comments and trailing commas
are fine:

```json5
{
    // Module order, left to right (rendered at the right end of the taskbar).
    "modules": ["cpu", "clock"],

    // Global style defaults, inherited by every module.
    "css": {
        "font-family": "Segoe UI",
        "font-size": "12px",
        "color": "#d0d0d0",
        "padding": "0 8px"
    },

    // Each remaining top-level key defines a module.
    "cpu": {
        "exec": "powershell -NoProfile -Command \"'CPU ' + (Get-CimInstance Win32_Processor).LoadPercentage + '%'\"",
        "interval": 2, // seconds between runs (default: 5)
        "css": { "color": "#7fdbb0", "font-weight": "bold" }
    },
    "clock": {
        // Each output line becomes a line on the bar.
        "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('ddd dd MMM'); (Get-Date).ToString('HH:mm:ss')\"",
        "interval": 1,
        "css": { "color": "#ffffff", "font-size": "14px" }
    }
}
```

### Modules

| Key        | Type   | Default | Description                                    |
| ---------- | ------ | ------- | ---------------------------------------------- |
| `exec`     | string | (required) | Command to run; stdout becomes the module text |
| `interval` | number | `5`     | Seconds between runs                           |
| `css`      | object | `{}`    | Style overrides for this module                |
| `output`   | string | `"text"` | `"html"` enables span markup in stdout (see Dynamic styles) |
| `classes`  | object | `{}`    | Module-only named style fragments              |

### Styling

Supported CSS properties, in the global `css` block or per module:

`color`, `background-color`, `font-family`, `font-size` (px),
`font-weight` (`normal`, `bold`, or a number), `padding`, `margin`
(1-4 edge values, px), `text-align` (`left`, `center`, `right`).

### Dynamic styles

A module can change its look based on what it reports. Define named classes,
opt the module into HTML output, and let the script wrap parts of its output
in spans that reference them:

```json5
{
    // Shared classes, available to every module.
    "classes": {
        "warning":  { "color": "#f5c542" },
        "critical": { "color": "#ff5555", "font-weight": "bold" }
    },
    "memory": {
        "exec": "powershell ... memory.ps1",
        "output": "html",
        // Module-only classes; a name defined in both places merges,
        // property by property, and the module's keys win.
        "classes": { "title": { "font-weight": "bold" } }
    }
}
```

With `"output": "html"`, the module prints its text with markup inline, and
the tags never appear on the bar:

```
<span class='title'>MEM </span><span class='warning'>76%</span> used
```

Only the `span` tag with a `class` attribute is recognized. Spans nest, and
classes accumulate outer to inner. Each output line is a line on the bar,
exactly like plain text. Escape literal characters as `&lt;`, `&gt;`,
`&amp;`, `&quot;`, `&apos;`. Span classes may only change text-level
properties: `color`, `background-color`, `font-family`, `font-size`, and
`font-weight`. Box properties (`padding`, `margin`, `text-align`) stay
module-level and are ignored in a span with a warning.

Malformed markup is shown as plain text with a warning on stderr, so a
broken module stays visible. Plain-text modules (no `output` key) are never
parsed: their angle brackets and ampersands display literally, and they keep
working unchanged.

### Multiple monitors

By default all modules appear on the primary taskbar. To route modules per
monitor, add a `monitors` map keyed by monitor index (shown in the console
output of a debug build):

```json5
{
    "modules": ["cpu", "clock"], // fallback for monitors not listed below
    "monitors": {
        "0": { "modules": ["cpu", "clock"] },
        "1": { "modules": ["clock"] }
    }
}
```

Secondary taskbars need **Settings → Personalization → Taskbar → Show my
taskbar on all displays** enabled. Windows 11 paints its own clock on
secondary taskbars; `"secondary-clock-reserve"` (default `100`) reserves that
many pixels at the right edge so modules don't overlap it.

## Examples

The [`examples/`](examples/) folder has ready-to-use modules you can point your
config at: Claude usage with progress bars, memory in use, and network
throughput. Each one is a single folder you can copy anywhere.

## Building from source

Requires [Rust](https://rustup.rs/) stable on Windows.

```
cargo run             # debug build; keeps a console for diagnostics
cargo build --release # console-less background app
```

## Security

A module is a shell command, so `config.json` is executable content: anything
in an `exec` value runs as you, and the file is reloaded automatically when it
changes. Treat it like a script rather than a settings file. Only run configs
you wrote or have read, and keep the file somewhere other users on the machine
cannot write to.

## License

[MIT](LICENSE)
