# Taskband module: download and upload throughput on physical adapters.
#
# Prints two lines, for example:
#   v 4.2 MB/s
#   ^ 118 KB/s
# (the real output uses the down and up arrow glyphs)
#
# Each run samples the byte counters twice one second apart, so it takes about
# a second longer than other modules. Taskband runs modules sequentially on one
# thread, so keep "interval" at 5 or more.
#
# config.json:
#   "network-speed": {
#       "exec": "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\path\\to\\examples\\network-speed\\network-speed.ps1\"",
#       "interval": 5,
#       "css": { "text-align": "left" }
#   }
#
# Note on the data source: Get-NetAdapterStatistics is deliberately not used.
# Many drivers publish no statistics object for their adapter, in which case it
# reports nothing and this module would silently show 0 B/s forever. The
# Win32_PerfRawData_* class names are also not localized, unlike Get-Counter's.

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Perf-counter instance names substitute brackets for parentheses and
# underscores for '#', '/' and '\'. For example
# "Intel(R) Ethernet Connection (17) I219-V" becomes
# "Intel[R] Ethernet Connection [17] I219-V".
function Get-PerfName($desc) {
    $out = $desc.Replace('(', '[').Replace(')', ']').Replace('#', '_')
    return ($out -replace '[/\\]', '_')
}

# Physical, connected adapters only, so WSL, Hyper-V, VirtualBox and VPN
# adapters do not get counted as internet traffic.
$wanted = @(Get-NetAdapter |
    Where-Object { -not $_.Virtual -and $_.Status -eq 'Up' } |
    ForEach-Object { Get-PerfName $_.InterfaceDescription })

function Read-Total {
    $rows = @(Get-CimInstance Win32_PerfRawData_Tcpip_NetworkInterface -ErrorAction SilentlyContinue |
        Where-Object { $wanted -contains $_.Name })
    [pscustomobject]@{
        Rx = [double](($rows | Measure-Object -Property BytesReceivedPersec -Sum).Sum)
        Tx = [double](($rows | Measure-Object -Property BytesSentPersec -Sum).Sum)
    }
}

function Format-Rate([double]$bps) {
    if ($bps -ge 1MB) { return ('{0:N1} MB/s' -f ($bps / 1MB)) }
    if ($bps -ge 1KB) { return ('{0:N0} KB/s' -f ($bps / 1KB)) }
    return ('{0:N0} B/s' -f $bps)
}

$down = [char]0x2193
$up = [char]0x2191

if ($wanted.Count -eq 0) {
    "$down --"
    "$up --"
    exit
}

$a = Read-Total
$sw = [System.Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Milliseconds 1000
$sw.Stop()
$b = Read-Total
$secs = [math]::Max(0.001, $sw.Elapsed.TotalSeconds)

"$down $(Format-Rate ([math]::Max(0, $b.Rx - $a.Rx) / $secs))"
"$up $(Format-Rate ([math]::Max(0, $b.Tx - $a.Tx) / $secs))"
