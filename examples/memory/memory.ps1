# Taskband module: physical memory in use.
#
# Prints two lines, a bar over the detail:
#   MEM [========--]
#   56% . 17.8G / 31.7G
# (the real output uses the same fullwidth bar glyphs as the claude-usage module)
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
# For styled output (bold title, green bar, percentage that turns yellow at
# 75% and red at 90%), add "output": "html" and define the classes:
#   "classes": {
#       "warning":  { "color": "#f5c542" },
#       "critical": { "color": "#ff5555", "font-weight": "bold" }
#   },
#   "memory": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\memory\\memory.ps1\" -Styled",
#       "interval": 5,
#       "output": "html",
#       "css": { "font-family": "Consolas", "text-align": "left" },
#       "classes": {
#           "title": { "font-weight": "bold", "color": "#ffffff" },
#           "bar":   { "color": "#7fdbb0" }
#       }
#   }

param([switch]$Styled)

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Same glyphs as examples/claude-usage/claude-usage.js, written as codepoints so
# that re-encoding this file cannot corrupt them.
$open = [char]0xFF3B
$close = [char]0xFF3D
$filled = [char]0xFFED
$empty = [char]0xFF65
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

# Styled variant: span markup, printed as-is. Needs "output": "html".
$state = $null
if ($pct -ge 90) { $state = 'critical' } elseif ($pct -ge 75) { $state = 'warning' }

$pctText = if ($state) { "<span class='$state'>$pct%</span>" } else { "$pct%" }
"<span class='title'>MEM </span><span class='bar'>$bar</span>"
"$pctText $dot ${usedGb}G / ${totalGb}G"
