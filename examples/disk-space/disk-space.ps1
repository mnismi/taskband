# Taskband module: disk usage for every fixed drive.
#
# Prints two lines with all drives side by side, bars over the detail:
#   C: [========--]  D: [===-------]
#      61% 290/476G     30% 280/931G
# (the real output uses the same bar glyphs as the memory module)
#
# Drives at 1 TB or larger show T with one decimal, smaller drives show
# whole G. Needs a monospace font, or the detail line will not sit under
# the bars.
#
# config.json (plain output):
#   "disk": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\disk-space\\disk-space.ps1\"",
#       "interval": 30,
#       "css": { "font-family": "Consolas", "text-align": "left" }
#   }
#
# For styled output, add "output": "html". Only the bars are colored, each
# by its own usage level (green below 50%, yellow from 50%, orange from 75%,
# red from 90%); everything else renders in the module's normal color:
#   "disk": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\disk-space\\disk-space.ps1\" -Styled",
#       "interval": 30,
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

# Same glyphs as examples/memory/memory.ps1, written as codepoints so that
# re-encoding this file cannot corrupt them. Every glyph must exist in
# Consolas and Cascadia Mono with the standard monospace advance, or a bar's
# width changes with its fill level.
$open = '['
$close = ']'
$filled = [char]0x25AA
$empty = [char]0x00B7

$SEGMENTS = 10
$SEP = '  '

# DriveType 3 = local fixed disk; skips removable, network, and optical
# drives, whose free space is meaningless or absent on the taskbar.
$disks = Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' |
    Where-Object { $_.Size -gt 0 } | Sort-Object DeviceID

if (-not $disks) {
    'DISK: no fixed drives'
    exit
}

$tops = @()
$bottoms = @()

foreach ($d in $disks) {
    $total = [double]$d.Size
    $used = $total - [double]$d.FreeSpace
    $pct = [math]::Round($used / $total * 100)

    # One segment per 10%, clamped so a rounding surprise cannot produce a
    # negative repeat count.
    $n = [math]::Round($pct / 100 * $SEGMENTS)
    $n = [math]::Max(0, [math]::Min($SEGMENTS, $n))
    $bar = $open + ($filled.ToString() * $n) + ($empty.ToString() * ($SEGMENTS - $n)) + $close

    if ($total -ge 1TB) {
        $detail = '{0}% {1}/{2}T' -f $pct, [math]::Round($used / 1TB, 1), [math]::Round($total / 1TB, 1)
    }
    else {
        $detail = '{0}% {1}/{2}G' -f $pct, [math]::Round($used / 1GB), [math]::Round($total / 1GB)
    }

    $label = "$($d.DeviceID) "
    $top = $label + $bar
    # Visible width, before any markup, so styled and plain output align the
    # same way.
    $topWidth = $top.Length

    if ($Styled) {
        $level = if ($pct -ge 90) { 'red' }
            elseif ($pct -ge 75) { 'orange' }
            elseif ($pct -ge 50) { 'yellow' }
            else { 'green' }
        $top = $label + "<span class='$level'>$bar</span>"
    }

    $bottom = (' ' * $label.Length) + $detail

    # Pad both cells to the wider of the two so every column stays aligned
    # when the detail text outgrows the bar.
    $width = [math]::Max($topWidth, $bottom.Length)
    $top += ' ' * ($width - $topWidth)
    $bottom += ' ' * ($width - $bottom.Length)

    $tops += $top
    $bottoms += $bottom
}

($tops -join $SEP).TrimEnd()
($bottoms -join $SEP).TrimEnd()
