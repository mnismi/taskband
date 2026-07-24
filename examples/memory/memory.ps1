# Taskband module: physical memory in use.
#
# Prints two lines, a bar over the detail:
#   MEM [========--]
#   56% . 17.8G / 31.7G
# (the real output uses the same bar glyphs as the claude-usage module)
#
# Needs a monospace font, or the bar and the figures below it will not line up.
#
# config.json (plain output):
#   "memory": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\memory\\memory.ps1\"",
#       "interval": 5,
#       "css": { "font-family": "Consolas", "text-align": "left" }
#   }
#
# For styled output, add "output": "html". Only the bar is colored, by usage
# level (green below 50%, yellow from 50%, orange from 75%, red from 90%);
# everything else renders in the module's normal color:
#   "memory": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\memory\\memory.ps1\" -Styled",
#       "interval": 5,
#       "output": "html",
#       "css": { "font-family": "Consolas", "text-align": "left" },
#       "classes": {
#           "green":  { "color": "#7fdbb0" },
#           "yellow": { "color": "#f5c542" },
#           "orange": { "color": "#ff9f43" },
#           "red":    { "color": "#ff5555" }
#       }
#   }

param([switch]$Styled)

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Same glyphs as examples/claude-usage/claude-usage.js, written as codepoints so
# that re-encoding this file cannot corrupt them. Every glyph must exist in
# Consolas and Cascadia Mono with the standard monospace advance, or the bar's
# width changes with its fill level (the old fullwidth glyphs came from
# fallback fonts and did exactly that).
$open = '['
$close = ']'
$filled = [char]0x25AA
$empty = [char]0x00B7
$dot = [char]0x00B7

$SEGMENTS = 10

$os = Get-CimInstance Win32_OperatingSystem
$totalKb = [double]$os.TotalVisibleMemorySize
$usedKb = $totalKb - [double]$os.FreePhysicalMemory

$pct = [math]::Round($usedKb / $totalKb * 100)

# One segment per 10%, clamped so a rounding surprise cannot produce a negative
# repeat count.
$n = [math]::Round($pct / 100 * $SEGMENTS)
$n = [math]::Max(0, [math]::Min($SEGMENTS, $n))
$bar = $open + ($filled.ToString() * $n) + ($empty.ToString() * ($SEGMENTS - $n)) + $close

$usedGb = [math]::Round($usedKb / 1MB, 1)
$totalGb = [math]::Round($totalKb / 1MB, 1)

if (-not $Styled) {
    "MEM $bar"
    "$pct% $dot ${usedGb}G / ${totalGb}G"
    exit
}

# Styled variant: only the bar carries a color class, picked by usage level.
# Everything else is plain text. Needs "output": "html".
$level = if ($pct -ge 90) { 'red' }
    elseif ($pct -ge 75) { 'orange' }
    elseif ($pct -ge 50) { 'yellow' }
    else { 'green' }

"MEM <span class='$level'>$bar</span>"
"$pct% $dot ${usedGb}G / ${totalGb}G"
