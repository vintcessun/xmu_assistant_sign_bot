param(
  [string]$SecureLinkDataDir = "",
  [switch]$NoBuild,
  [int]$ReadyTimeoutSec = 150,
  # Host port the container's SOCKS5 is published on. Change (or set
  # SOCKS_HOST_PORT) when something else, e.g. a VS Code port forward, already
  # listens on host 1080 — a foreign listener there hijacks the probes.
  [int]$SocksPort = $(if ($env:SOCKS_HOST_PORT) { [int]$env:SOCKS_HOST_PORT } else { 1080 })
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$ComposeFile = Join-Path $RepoRoot "docker/docker-compose.yml"
$StepLog = Join-Path $RepoRoot ".run_socks5_vpn_check.log"

function Write-Step {
  param([Parameter(Mandatory = $true)][string]$Message)
  $line = "[{0}] {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $Message
  Write-Host $line
  Add-Content -LiteralPath $StepLog -Value $line
}

function Convert-ToDockerPath {
  param([Parameter(Mandatory = $true)][string]$Path)
  return ($Path -replace "\\", "/")
}

function Resolve-DataDir {
  param([string]$InputPath)

  if (-not [string]::IsNullOrWhiteSpace($InputPath)) {
    return (Resolve-Path -LiteralPath $InputPath).Path
  }

  if (-not [string]::IsNullOrWhiteSpace($env:SECURELINK_DATA_DIR)) {
    return (Resolve-Path -LiteralPath $env:SECURELINK_DATA_DIR).Path
  }

  $default = Join-Path $RepoRoot "../data/securelink"
  return (Resolve-Path -LiteralPath $default).Path
}

function Invoke-Compose {
  param([Parameter(Mandatory = $true)][string[]]$ComposeArgs)
  & docker compose -f $ComposeFile @ComposeArgs
  if ($LASTEXITCODE -ne 0) {
    throw "docker compose failed: $($ComposeArgs -join ' ')"
  }
}

function Wait-Ready {
  param([int]$TimeoutSec)

  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    $state = (& docker inspect xmu-securelink-socks --format "{{.State.Status}}" 2>$null | Out-String).Trim()
    if ($state -eq "running") {
      $logs = & docker logs --tail 120 xmu-securelink-socks 2>&1 | Out-String
      # "watching session files" is printed only after the tun is up AND the
      # XMU /32 routes are pinned, so it marks full readiness (microsocks now
      # starts BEFORE the VPN, so "starting microsocks" alone is too early).
      if ($logs -match "watching session files") {
        return
      }
      if ($logs -match "xmu_secure_link exited early" -or $logs -match "no callback URL entered") {
        throw "xmu_secure_link entered interactive SSO or exited early. Refresh the saved session first."
      }
      if ($logs -match "session data not found") {
        throw "session data was not mounted correctly. Check SECURELINK_DATA_DIR."
      }
    }
    Start-Sleep -Seconds 2
  }

  $tail = & docker logs --tail 160 xmu-securelink-socks 2>&1 | Out-String
  throw "container did not become ready within $TimeoutSec seconds.`n$tail"
}

function Assert-RouteViaTun {
  param([Parameter(Mandatory = $true)][string]$Target)

  # The VPN client reconnects on transient server drops; the tun (and our /32
  # routes) vanish for a few seconds until the entrypoint's watch loop re-pins
  # them. Retry instead of failing on that window.
  $attempts = 12
  for ($i = 1; $i -le $attempts; $i++) {
    $route = & docker exec xmu-securelink-socks ip route get $Target 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0 -and $route -match "\bdev\s+(tun|tap|ovpn)") {
      Write-Host "[ok] route $Target -> $($route.Trim())"
      return
    }
    if ($i -lt $attempts) {
      Write-Host "[retry $i/$attempts] route $Target not on tun yet (client reconnecting?)"
      Start-Sleep -Seconds 5
    }
  }
  throw "route check failed for ${Target}: $route"
}

function Read-Exact {
  param(
    [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
    [Parameter(Mandatory = $true)][byte[]]$Buffer,
    [Parameter(Mandatory = $true)][int]$Count
  )

  $offset = 0
  while ($offset -lt $Count) {
    $read = $Stream.Read($Buffer, $offset, $Count - $offset)
    if ($read -le 0) {
      throw "unexpected EOF while reading SOCKS5 response"
    }
    $offset += $read
  }
}

function Test-SocksBanner {
  param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$TargetHost,
    [Parameter(Mandatory = $true)][int]$TargetPort,
    [Parameter(Mandatory = $true)][string]$ExpectedPattern
  )

  # Short timeout on purpose: when the VPN client is mid-reconnect the SYN
  # black-holes; fail fast and let the retry wrapper land inside a connected
  # window instead of burning 15s per attempt.
  $client = [System.Net.Sockets.TcpClient]::new()
  $client.ReceiveTimeout = 3000
  $client.SendTimeout = 3000

  try {
    $client.Connect("127.0.0.1", $script:SocksPort)
    $stream = $client.GetStream()

    [byte[]]$hello = @([byte]0x05, [byte]0x01, [byte]0x00)
    $stream.Write($hello, 0, $hello.Length)
    [byte[]]$method = 0, 0
    Read-Exact $stream $method 2
    if ($method[0] -ne 0x05 -or $method[1] -ne 0x00) {
      throw "SOCKS5 method negotiation failed: $($method -join ',')"
    }

    [byte[]]$addr = [System.Net.IPAddress]::Parse($TargetHost).GetAddressBytes()
    [byte[]]$portBytes = @([byte](($TargetPort -shr 8) -band 0xff), [byte]($TargetPort -band 0xff))
    [byte[]]$request = @([byte]0x05, [byte]0x01, [byte]0x00, [byte]0x01) + $addr + $portBytes
    $stream.Write($request, 0, $request.Length)

    [byte[]]$head = 0, 0, 0, 0
    Read-Exact $stream $head 4
    if ($head[0] -ne 0x05 -or $head[1] -ne 0x00) {
      throw "SOCKS5 connect failed for ${TargetHost}:${TargetPort}; reply=$($head -join ',')"
    }

    $skip = switch ($head[3]) {
      0x01 { 4 + 2 }
      0x03 {
        [byte[]]$len = 0
        Read-Exact $stream $len 1
        [int]$len[0] + 2
      }
      0x04 { 16 + 2 }
      default { throw "unknown SOCKS5 address type in reply: $($head[3])" }
    }
    [byte[]]$discard = New-Object byte[] $skip
    Read-Exact $stream $discard $skip

    [byte[]]$buf = New-Object byte[] 256
    $read = $stream.Read($buf, 0, $buf.Length)
    if ($read -le 0) {
      throw "no banner received from ${TargetHost}:${TargetPort}"
    }
    $banner = ([System.Text.Encoding]::ASCII.GetString($buf, 0, $read)).Trim()
    if ($banner -notmatch $ExpectedPattern) {
      throw "$Name probe failed. Expected pattern '$ExpectedPattern'. Banner: $banner"
    }
    Write-Host "[ok] $Name banner: $banner"
  }
  finally {
    $client.Close()
  }
}

# Wait until the target's route inside the container is on the tun (= the VPN
# is in a connected window). Probing blind can phase-lock with a periodic
# server-side disconnect cycle and miss every window; a real consumer's retries
# land in a window the same way this does.
function Wait-TunWindow {
  param([Parameter(Mandatory = $true)][string]$Target, [int]$TimeoutSec = 40)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    $r = & docker exec xmu-securelink-socks ip route get $Target 2>$null | Out-String
    if ($r -match "\bdev\s+(tun|tap|ovpn)") { return $true }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

# Retry wrapper for the banner probes: a probe hitting the few-second window of
# a VPN reconnect fails spuriously; the entrypoint self-heals within one watch
# tick, so retrying is the correct acceptance semantics.
function Test-SocksBannerWithRetry {
  param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$TargetHost,
    [Parameter(Mandatory = $true)][int]$TargetPort,
    [Parameter(Mandatory = $true)][string]$ExpectedPattern,
    [int]$Attempts = 20
  )
  for ($i = 1; $i -le $Attempts; $i++) {
    try {
      if (-not (Wait-TunWindow -Target $TargetHost)) {
        throw "no connected VPN window within 40s (route never on tun)"
      }
      Test-SocksBanner -Name $Name -TargetHost $TargetHost -TargetPort $TargetPort -ExpectedPattern $ExpectedPattern
      return
    } catch {
      if ($i -eq $Attempts) { throw }
      Write-Host "[retry $i/$Attempts] $Name probe failed: $($_.Exception.Message)"
      Start-Sleep -Milliseconds 700
    }
  }
}

$dataDir = Resolve-DataDir $SecureLinkDataDir
if (-not (Test-Path -LiteralPath (Join-Path $dataDir "session.json"))) {
  throw "session.json not found in $dataDir"
}
if (-not (Test-Path -LiteralPath (Join-Path $dataDir "device_id"))) {
  throw "device_id not found in $dataDir"
}

$env:SECURELINK_DATA_DIR = Convert-ToDockerPath $dataDir
$env:SOCKS_HOST_PORT = "$SocksPort"   # compose reads this for the host port mapping

Remove-Item -LiteralPath $StepLog -Force -ErrorAction SilentlyContinue
Write-Step "SECURELINK_DATA_DIR=$env:SECURELINK_DATA_DIR"
Write-Step "compose=$ComposeFile"

try {
  if ($NoBuild) {
    Write-Step "docker compose up -d --force-recreate"
    Invoke-Compose -ComposeArgs @("up", "-d", "--force-recreate")
  } else {
    Write-Step "docker compose up -d --build --force-recreate"
    Invoke-Compose -ComposeArgs @("up", "-d", "--build", "--force-recreate")
  }

  Write-Step "waiting for VPN and microsocks"
  Wait-Ready -TimeoutSec $ReadyTimeoutSec

  Write-Step "checking routes"
  Assert-RouteViaTun "121.192.180.236"
  Assert-RouteViaTun "59.77.5.59"
  Assert-RouteViaTun "219.229.81.200"

  Write-Step "checking FTP banner"
  Test-SocksBannerWithRetry "FTP" "121.192.180.236" 21 "^220 "
  Write-Step "checking SSH banner"
  Test-SocksBannerWithRetry "SSH" "59.77.5.59" 2222 "^SSH-2\.0-"

  Write-Step "SOCKS5 -> Docker -> VPN validation passed"
}
finally {
  Write-Step "stopping docker compose"
  # 30s: entrypoint waits up to ~10s for the VPN client to disconnect cleanly.
  # A SIGKILL here leaves a half-open session server-side and later connections
  # get kicked into a reconnect loop, so give the graceful path room.
  & docker compose -f $ComposeFile down --timeout 30
  Write-Step "docker compose stopped"
}
