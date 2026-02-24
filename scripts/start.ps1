#Requires -Version 5.1
<#
.SYNOPSIS
    Start the full Rustyfin Docker stack in a fresh clone or existing workspace.
.DESCRIPTION
    Safe defaults:
    - auto-creates a local media directory if none is provided
    - can auto-pick free host ports when defaults are occupied
.PARAMETER Build
    Rebuild images (cached, default behavior).
.PARAMETER FullRebuild
    Rebuild without cache (slowest, strictest).
.PARAMETER NoBuild
    Skip image rebuild step.
.PARAMETER Foreground
    Run compose in foreground (default is detached).
.PARAMETER NoHealthCheck
    Skip backend health wait loop.
.PARAMETER File
    Compose file path (default: docker-compose.yml).
.PARAMETER Help
    Show this help.
#>
param(
    [switch]$Build,
    [switch]$FullRebuild,
    [switch]$CachedBuild,
    [switch]$NoBuild,
    [switch]$Foreground,
    [switch]$NoHealthCheck,
    [string]$File = "",
    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Write-Info    { param([string]$Msg) Write-Host "[start] $Msg" -ForegroundColor Cyan }
function Write-Success { param([string]$Msg) Write-Host "[start] $Msg" -ForegroundColor Green }
function Write-Warn    { param([string]$Msg) Write-Host "[start] $Msg" -ForegroundColor Yellow }
function Write-Die     { param([string]$Msg) Write-Host "[start] ERROR: $Msg" -ForegroundColor Red; exit 1 }

function Show-Usage {
    Write-Host @"
Usage:
  .\scripts\start.ps1 [-Build] [-FullRebuild] [-NoBuild] [-Foreground] [-NoHealthCheck] [-File <path>]

Options:
  -Build            Rebuild images (cached, default behavior).
  -FullRebuild      Rebuild without cache (slowest, strictest).
  -CachedBuild      Alias for -Build.
  -NoBuild          Skip image rebuild step.
  -Foreground       Run compose in foreground (default is detached).
  -NoHealthCheck    Skip backend health wait loop.
  -File             Compose file path (default: docker-compose.yml).
  -Help             Show this help.
"@
}

if ($Help) { Show-Usage; exit 0 }

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent $ScriptDir

# Resolve flags (later flags win, matching bash precedence)
$DoBuild      = $true
$NoCacheBuild = $false
$Detach       = $true
$HealthCheck  = $true

if ($NoBuild)                { $DoBuild = $false; $NoCacheBuild = $false }
if ($Build -or $CachedBuild) { $DoBuild = $true;  $NoCacheBuild = $false }
if ($FullRebuild)            { $DoBuild = $true;  $NoCacheBuild = $true  }
if ($Foreground)             { $Detach      = $false }
if ($NoHealthCheck)          { $HealthCheck = $false }

$ComposeFile = if ($File) { $File } else { Join-Path $RepoRoot "docker-compose.yml" }
if (-not [System.IO.Path]::IsPathRooted($ComposeFile)) {
    $ComposeFile = Join-Path $RepoRoot $ComposeFile
}

Set-Location $RepoRoot

if (-not (Test-Path $ComposeFile))                          { Write-Die "docker-compose.yml not found at $ComposeFile" }
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Write-Die "docker is not installed or not in PATH" }
docker compose version 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Die "docker compose is not available" }

$RuntimeEnvFile = Join-Path $RepoRoot ".rustyfin.runtime.env"

$SafeTmpDir = if ($env:RUSTFIN_TMPDIR) { $env:RUSTFIN_TMPDIR } else { Join-Path $RepoRoot ".tmp" }
New-Item -ItemType Directory -Force -Path $SafeTmpDir | Out-Null
if (-not (Test-Path $SafeTmpDir)) { Write-Die "Failed to create temp dir: $SafeTmpDir" }

# Load prior runtime settings so repeated runs stay stable.
$userBackendPort          = $env:RUSTFIN_BACKEND_PORT
$userUiPort               = $env:RUSTFIN_UI_PORT
$userMediaPath            = $env:RUSTFIN_MEDIA_PATH
$userBrowserBackendOrigin = $env:RUSTYFIN_BROWSER_BACKEND_ORIGIN
$userWsAllowedOrigins     = $env:RUSTFIN_WS_ALLOWED_ORIGINS

if (Test-Path $RuntimeEnvFile) {
    Get-Content $RuntimeEnvFile | ForEach-Object {
        if ($_ -match '^\s*#' -or $_ -notmatch '=') { return }
        $parts = $_ -split '=', 2
        $key   = $parts[0].Trim()
        $val   = $parts[1].Trim().Trim("'").Trim('"')
        [System.Environment]::SetEnvironmentVariable($key, $val, "Process")
    }
}

# Explicit shell/env values always win over runtime file values.
if ($userBackendPort)          { $env:RUSTFIN_BACKEND_PORT           = $userBackendPort }
if ($userUiPort)               { $env:RUSTFIN_UI_PORT                = $userUiPort }
if ($userMediaPath)            { $env:RUSTFIN_MEDIA_PATH             = $userMediaPath }
if ($userBrowserBackendOrigin) { $env:RUSTYFIN_BROWSER_BACKEND_ORIGIN = $userBrowserBackendOrigin }
if ($userWsAllowedOrigins)     { $env:RUSTFIN_WS_ALLOWED_ORIGINS     = $userWsAllowedOrigins }

# Migrate legacy repo-local default media root.
$legacyMediaRoot = Join-Path $RepoRoot "media"
if (-not $userMediaPath -and $env:RUSTFIN_MEDIA_PATH -eq $legacyMediaRoot) {
    $env:RUSTFIN_MEDIA_PATH = $env:USERPROFILE
}

$backendLocked = [bool]$userBackendPort
$uiLocked      = [bool]$userUiPort

# Default media path for first-time setup.
$mediaPath = if ($env:RUSTFIN_MEDIA_PATH) { $env:RUSTFIN_MEDIA_PATH } `
             elseif ($env:USERPROFILE)    { $env:USERPROFILE } `
             else                         { Join-Path $RepoRoot "media" }

New-Item -ItemType Directory -Force -Path $mediaPath | Out-Null
if (-not (Test-Path $mediaPath)) { Write-Die "Failed to create media path: $mediaPath" }
$mediaPath = (Resolve-Path $mediaPath).Path
if (-not (Test-Path $mediaPath -PathType Container)) { Write-Die "Resolved media path is not a directory: $mediaPath" }
$env:RUSTFIN_MEDIA_PATH = $mediaPath

$PickerHelperPort    = if ($env:RUSTFIN_PICKER_HELPER_PORT) { $env:RUSTFIN_PICKER_HELPER_PORT } else { "43110" }
$PickerHelperHost    = if ($env:RUSTFIN_PICKER_HELPER_HOST) { $env:RUSTFIN_PICKER_HELPER_HOST } else { "0.0.0.0" }
$PickerHelperPidFile = Join-Path $SafeTmpDir "directory-picker-helper.pid"
$PickerHelperLogFile = Join-Path $SafeTmpDir "directory-picker-helper.log"
$PickerHelperScript  = Join-Path $SafeTmpDir "directory-picker-helper.py"

function Start-DirectoryPickerHelper {
    $enabled = if ($env:RUSTFIN_ENABLE_PICKER_HELPER) { $env:RUSTFIN_ENABLE_PICKER_HELPER } else { "1" }
    if ($enabled -eq "0") {
        Write-Warn "Directory picker helper disabled (RUSTFIN_ENABLE_PICKER_HELPER=0)."
        return
    }

    $pyBin = $null
    if (Get-Command python3 -ErrorAction SilentlyContinue) { $pyBin = "python3" }
    elseif (Get-Command python -ErrorAction SilentlyContinue) { $pyBin = "python" }
    if (-not $pyBin) {
        Write-Warn "Python not found; native host directory picker helper not started."
        return
    }

    try {
        $r = Invoke-WebRequest "http://127.0.0.1:${PickerHelperPort}/health" -UseBasicParsing -TimeoutSec 1 -ErrorAction Stop
        if ($r.StatusCode -eq 200) {
            Write-Info "Directory picker helper already running on port ${PickerHelperPort}."
            return
        }
    } catch {}

    if (Test-Path $PickerHelperPidFile) {
        $existingPid = Get-Content $PickerHelperPidFile -ErrorAction SilentlyContinue
        if ($existingPid) {
            $proc = Get-Process -Id ([int]$existingPid) -ErrorAction SilentlyContinue
            if ($proc) {
                Write-Info "Directory picker helper already running (pid $existingPid)."
                return
            }
        }
        Remove-Item $PickerHelperPidFile -Force -ErrorAction SilentlyContinue
    }

    # Write the Python helper script (identical to the bash version)
    $pyScript = @'
#!/usr/bin/env python3
import json
import os
import platform
import shutil
import subprocess
from http.server import BaseHTTPRequestHandler, HTTPServer

HOST = os.environ.get("RUSTFIN_PICKER_HELPER_HOST", "0.0.0.0")
PORT = int(os.environ.get("RUSTFIN_PICKER_HELPER_PORT", "43110"))

def pick_directory():
    system = platform.system()
    if system == "Darwin":
        script = 'set chosenFolder to choose folder with prompt "Select a media directory for Rustyfin"\nPOSIX path of chosenFolder'
        out = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
        if out.returncode == 0:
            return out.stdout.strip()
        err = (out.stderr or "").strip()
        if "User canceled" in err or "(-128)" in err:
            return ""
        raise RuntimeError(err or "folder picker failed")

    if system == "Linux":
        if shutil.which("zenity"):
            out = subprocess.run(
                ["zenity", "--file-selection", "--directory", "--title=Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "zenity folder picker failed")
        if shutil.which("kdialog"):
            out = subprocess.run(
                ["kdialog", "--getexistingdirectory", ".", "Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "kdialog folder picker failed")
        raise RuntimeError("no supported Linux picker found (install zenity or kdialog)")

    if system == "Windows":
        ps_script = r"""
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a media directory for Rustyfin'
$result = $dialog.ShowDialog()
if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
  Write-Output $dialog.SelectedPath
}
"""
        out = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps_script],
            capture_output=True,
            text=True,
        )
        if out.returncode == 0:
            return (out.stdout or "").strip()
        raise RuntimeError((out.stderr or "").strip() or "PowerShell folder picker failed")

    raise RuntimeError(f"unsupported host OS for picker helper: {system}")

class Handler(BaseHTTPRequestHandler):
    def _write_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._write_json(200, {"ok": True})
        else:
            self._write_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/pick":
            self._write_json(404, {"error": "not found"})
            return
        try:
            selected = pick_directory()
            if not selected:
                self._write_json(400, {"error": "directory selection cancelled"})
                return
            self._write_json(200, {"path": selected})
        except Exception as exc:
            self._write_json(500, {"error": str(exc)})

    def log_message(self, format, *args):
        return

def main():
    server = HTTPServer((HOST, PORT), Handler)
    server.serve_forever()

if __name__ == "__main__":
    main()
'@

    Set-Content -Path $PickerHelperScript -Value $pyScript -Encoding UTF8

    $env:RUSTFIN_PICKER_HELPER_PORT = $PickerHelperPort
    $env:RUSTFIN_PICKER_HELPER_HOST = $PickerHelperHost

    $helperProc = Start-Process `
        -FilePath $pyBin `
        -ArgumentList "`"$PickerHelperScript`"" `
        -WindowStyle Hidden `
        -PassThru `
        -RedirectStandardOutput $PickerHelperLogFile `
        -RedirectStandardError  "$PickerHelperLogFile.err"

    Set-Content -Path $PickerHelperPidFile -Value $helperProc.Id

    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 200
        try {
            $r = Invoke-WebRequest "http://127.0.0.1:${PickerHelperPort}/health" -UseBasicParsing -TimeoutSec 1 -ErrorAction Stop
            if ($r.StatusCode -eq 200) {
                Write-Info "Directory picker helper started on http://127.0.0.1:${PickerHelperPort} (pid $($helperProc.Id))"
                return
            }
        } catch {}
    }
    Write-Warn "Directory picker helper did not report healthy; check: $PickerHelperLogFile"
}

Start-DirectoryPickerHelper

$env:RUSTFIN_PICKER_HELPER_PORT          = $PickerHelperPort
$env:RUSTFIN_DIRECTORY_PICKER_HELPER_URL = if ($env:RUSTFIN_DIRECTORY_PICKER_HELPER_URL) { $env:RUSTFIN_DIRECTORY_PICKER_HELPER_URL } else { "http://host.docker.internal:${PickerHelperPort}/pick" }
$env:RUSTFIN_MEDIA_HOST_PATH             = if ($env:RUSTFIN_MEDIA_HOST_PATH)             { $env:RUSTFIN_MEDIA_HOST_PATH }             else { $env:RUSTFIN_MEDIA_PATH }
$env:RUSTFIN_MEDIA_CONTAINER_ROOT        = if ($env:RUSTFIN_MEDIA_CONTAINER_ROOT)        { $env:RUSTFIN_MEDIA_CONTAINER_ROOT }        else { $env:RUSTFIN_MEDIA_PATH }

function Test-PortInUse {
    param([int]$Port)
    $listeners = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()
    return ($listeners | Where-Object { $_.Port -eq $Port }).Count -gt 0
}

function Get-FreePort {
    param([int]$Preferred, [int]$MaxHops = 200)
    $p    = $Preferred
    $hops = 0
    while (Test-PortInUse $p) {
        $p++
        $hops++
        if ($hops -gt $MaxHops) { Write-Die "Unable to find a free port near $Preferred" }
    }
    return $p
}

function Get-PrimaryLanIPv4 {
    try {
        $route = Get-NetRoute -DestinationPrefix "0.0.0.0/0" -ErrorAction SilentlyContinue |
            Sort-Object RouteMetric | Select-Object -First 1
        if ($route) {
            $addr = Get-NetIPAddress -InterfaceIndex $route.InterfaceIndex `
                -AddressFamily IPv4 -ErrorAction SilentlyContinue |
                Where-Object { $_.IPAddress -ne "127.0.0.1" } |
                Select-Object -First 1
            if ($addr) { return $addr.IPAddress }
        }
    } catch {}
    return $null
}

function Test-IsIPv4 {
    param([string]$Value)
    return $Value -match '^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$'
}

function Find-OpenSSL {
    $cmd = Get-Command openssl -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @(
        "C:\Program Files\Git\usr\bin\openssl.exe",
        "C:\Program Files (x86)\Git\usr\bin\openssl.exe",
        "C:\Program Files\OpenSSL-Win64\bin\openssl.exe",
        "C:\Program Files\OpenSSL\bin\openssl.exe",
        "C:\OpenSSL-Win64\bin\openssl.exe",
        "C:\OpenSSL\bin\openssl.exe"
    )
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd) {
        $gitBinDir  = Split-Path -Parent $gitCmd.Source
        $gitRoot    = Split-Path -Parent $gitBinDir
        $candidates += (Join-Path $gitRoot "usr\bin\openssl.exe")
    }
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    return $null
}

function New-TlsCertViaDotNet {
    param([string]$HostName, [string]$CertPath, [string]$KeyPath)
    # Requires .NET 5+ (PowerShell 7+). Returns $false on older runtimes.
    try {
        $rsa = [System.Security.Cryptography.RSA]::Create(2048)
        $req = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
            "CN=$HostName", $rsa,
            [System.Security.Cryptography.HashAlgorithmName]::SHA256,
            [System.Security.Cryptography.RSASignaturePadding]::Pkcs1
        )
        $san = [System.Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
        $san.AddDnsName("localhost")
        $san.AddIpAddress([System.Net.IPAddress]::Parse("127.0.0.1"))
        if (Test-IsIPv4 $HostName) {
            $san.AddIpAddress([System.Net.IPAddress]::Parse($HostName))
        } elseif ($HostName -ne "localhost") {
            $san.AddDnsName($HostName)
        }
        $req.CertificateExtensions.Add($san.Build())
        $now  = [System.DateTimeOffset]::UtcNow
        $cert = $req.CreateSelfSigned($now, $now.AddDays(365))

        $certBytes = $cert.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert)
        $certB64   = [System.Convert]::ToBase64String($certBytes, [System.Base64FormattingOptions]::InsertLineBreaks)
        [System.IO.File]::WriteAllText($CertPath, "-----BEGIN CERTIFICATE-----`n$certB64`n-----END CERTIFICATE-----`n")

        $keyBytes = $rsa.ExportRSAPrivateKey()
        $keyB64   = [System.Convert]::ToBase64String($keyBytes, [System.Base64FormattingOptions]::InsertLineBreaks)
        [System.IO.File]::WriteAllText($KeyPath, "-----BEGIN RSA PRIVATE KEY-----`n$keyB64`n-----END RSA PRIVATE KEY-----`n")

        $rsa.Dispose()
        return $true
    } catch {
        return $false
    }
}

function Ensure-EdgeTlsCert {
    param([string]$HostName)
    $certDir  = Join-Path $SafeTmpDir "edge-tls"
    $certPath = Join-Path $certDir "tls.crt"
    $keyPath  = Join-Path $certDir "tls.key"
    $metaPath = Join-Path $certDir "meta.host"

    New-Item -ItemType Directory -Force -Path $certDir | Out-Null

    $needRegen = $false
    if (-not (Test-Path $certPath) -or -not (Test-Path $keyPath)) {
        $needRegen = $true
    } elseif (-not (Test-Path $metaPath) -or (Get-Content $metaPath -Raw -ErrorAction SilentlyContinue).Trim() -ne $HostName) {
        $needRegen = $true
    }

    if (-not $needRegen) {
        $env:RUSTFIN_EDGE_TLS_CERT = $certPath
        $env:RUSTFIN_EDGE_TLS_KEY  = $keyPath
        return
    }

    if (Test-Path $certPath) { Remove-Item $certPath -Force }
    if (Test-Path $keyPath)  { Remove-Item $keyPath  -Force }

    $opensslBin = Find-OpenSSL
    if ($opensslBin) {
        $san = "DNS:localhost,IP:127.0.0.1"
        if (Test-IsIPv4 $HostName) { $san += ",IP:$HostName" } else { $san += ",DNS:$HostName" }
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = "SilentlyContinue"
        & $opensslBin req -x509 -newkey rsa:2048 -sha256 -days 365 -nodes `
            -keyout $keyPath -out $certPath -subj "/CN=$HostName" `
            -addext "subjectAltName=$san" 2>$null
        $opensslExit = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP
        if ($opensslExit -ne 0) { Write-Die "Failed generating local TLS cert via openssl" }
    } elseif (New-TlsCertViaDotNet $HostName $certPath $keyPath) {
        Write-Info "Generated local TLS cert via .NET (openssl not found in PATH)"
    } else {
        Write-Die @"
openssl is required to generate local TLS certificates but was not found.
Install it via one of:
  winget install ShiningLight.OpenSSL
  choco install openssl
  scoop install openssl
If Git for Windows is installed, openssl may already exist at:
  C:\Program Files\Git\usr\bin\openssl.exe
"@
    }

    Set-Content -Path $metaPath -Value $HostName -NoNewline
    $env:RUSTFIN_EDGE_TLS_CERT = $certPath
    $env:RUSTFIN_EDGE_TLS_KEY  = $keyPath
}

$projectRunning = $false
$runningQ = docker compose -f $ComposeFile ps --status running -q 2>$null
if ($runningQ) { $projectRunning = $true }

$backendPort = if ($env:RUSTFIN_BACKEND_PORT) { $env:RUSTFIN_BACKEND_PORT } else { "8096" }
$uiPort      = if ($env:RUSTFIN_UI_PORT)      { $env:RUSTFIN_UI_PORT }      else { "3000" }

if (-not $backendLocked -and -not $projectRunning) {
    $backendSelected = Get-FreePort ([int]$backendPort)
    if ($backendSelected -ne [int]$backendPort) {
        Write-Warn "Port $backendPort is busy; using backend port $backendSelected"
    }
    $backendPort = "$backendSelected"
}

if (-not $uiLocked -and -not $projectRunning) {
    $uiSelected = Get-FreePort ([int]$uiPort)
    if ($uiSelected -ne [int]$uiPort) {
        Write-Warn "Port $uiPort is busy; using UI port $uiSelected"
    }
    $uiPort = "$uiSelected"
}

$env:RUSTFIN_BACKEND_PORT = $backendPort
$env:RUSTFIN_UI_PORT      = $uiPort

$publicHost = $env:RUSTFIN_PUBLIC_HOST
if (-not $publicHost) {
    $lanIp = Get-PrimaryLanIPv4
    $publicHost = if ($lanIp) { $lanIp } else { "localhost" }
}
$env:RUSTFIN_PUBLIC_HOST = $publicHost
Ensure-EdgeTlsCert $publicHost

if ($userBrowserBackendOrigin) {
    $env:RUSTYFIN_BROWSER_BACKEND_ORIGIN = $userBrowserBackendOrigin
} else {
    $env:RUSTYFIN_BROWSER_BACKEND_ORIGIN = "http://${publicHost}:${backendPort}"
}

if ($userWsAllowedOrigins) {
    $env:RUSTFIN_WS_ALLOWED_ORIGINS = $userWsAllowedOrigins
} else {
    $wsOrigins = @(
        "http://localhost:${uiPort}",
        "http://127.0.0.1:${uiPort}",
        "https://localhost:${uiPort}",
        "https://127.0.0.1:${uiPort}"
    )
    if ($publicHost -ne "localhost" -and $publicHost -ne "127.0.0.1") {
        $wsOrigins += "http://${publicHost}:${uiPort}"
        $wsOrigins += "https://${publicHost}:${uiPort}"
    }
    $env:RUSTFIN_WS_ALLOWED_ORIGINS = $wsOrigins -join ","
}

Write-Info "Using TMPDIR: $SafeTmpDir"
Write-Info "Using media path: $($env:RUSTFIN_MEDIA_PATH)"
Write-Info "Backend port: $($env:RUSTFIN_BACKEND_PORT)"
Write-Info "UI port: $($env:RUSTFIN_UI_PORT)"
Write-Info "Public host: $publicHost"
Write-Info "Browser backend origin: $($env:RUSTYFIN_BROWSER_BACKEND_ORIGIN)"
Write-Info "WebSocket allowed origins: $($env:RUSTFIN_WS_ALLOWED_ORIGINS)"
Write-Info "UI transport: HTTPS (secure context for microphone/WebRTC on LAN)"
Write-Info "Edge TLS cert: $($env:RUSTFIN_EDGE_TLS_CERT)"

if ($DoBuild) {
    if ($NoCacheBuild) {
        Write-Info "Build mode: full rebuild (no Docker cache)"
    } else {
        Write-Info "Build mode: rebuild (Docker cache enabled)"
    }
} else {
    Write-Warn "Build mode: skipped (-NoBuild)"
}

if ($env:RUSTFIN_TMDB_KEY) {
    Write-Info "TMDB metadata enrichment: enabled"
} else {
    Write-Warn "TMDB metadata enrichment disabled (set RUSTFIN_TMDB_KEY to fetch online posters/metadata)"
}

if ($DoBuild) {
    $buildArgs = @("build", "--pull")
    if ($NoCacheBuild) { $buildArgs += "--no-cache" }
    Write-Info "Rebuilding Docker images..."
    docker compose -f $ComposeFile @buildArgs
    if ($LASTEXITCODE -ne 0) {
        if ($NoCacheBuild) {
            Write-Warn "Full no-cache rebuild failed (likely transient network issue). Retrying once with Docker cache."
            docker compose -f $ComposeFile build --pull
            if ($LASTEXITCODE -ne 0) {
                Write-Die "Docker image rebuild failed after retry. Check your internet connection and retry."
            }
        } else {
            Write-Die "Docker image rebuild failed. Check your internet connection and retry."
        }
    }
}

$composeArgs = @("up", "--remove-orphans")
if ($Detach) { $composeArgs += "-d" }

docker compose -f $ComposeFile @composeArgs

@(
    "# Generated by scripts/start.ps1",
    "RUSTFIN_BACKEND_PORT=$($env:RUSTFIN_BACKEND_PORT)",
    "RUSTFIN_UI_PORT=$($env:RUSTFIN_UI_PORT)",
    "RUSTFIN_MEDIA_PATH=$($env:RUSTFIN_MEDIA_PATH)",
    "RUSTYFIN_BROWSER_BACKEND_ORIGIN=$($env:RUSTYFIN_BROWSER_BACKEND_ORIGIN)",
    "RUSTFIN_WS_ALLOWED_ORIGINS=$($env:RUSTFIN_WS_ALLOWED_ORIGINS)"
) | Set-Content -Path $RuntimeEnvFile -Encoding UTF8

if ($Detach -and $HealthCheck) {
    Write-Info "Waiting for backend health endpoint..."
    $ok = $false
    for ($i = 0; $i -lt 60; $i++) {
        try {
            $r = Invoke-WebRequest "http://127.0.0.1:${backendPort}/health" -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
            if ($r.StatusCode -eq 200) { $ok = $true; break }
        } catch {}
        Start-Sleep -Seconds 1
    }
    if (-not $ok) {
        Write-Warn "Backend health check did not pass within 60s."
        Write-Warn "Check logs with: docker compose -f `"$ComposeFile`" logs -f"
    }
}

Write-Success "Rustyfin stack is up."
Write-Host "  Backend: http://localhost:${backendPort}"
Write-Host "  UI:      https://localhost:${uiPort}"
if ($publicHost -ne "localhost" -and $publicHost -ne "127.0.0.1") {
    Write-Host "  Backend (LAN): http://${publicHost}:${backendPort}"
    Write-Host "  UI (LAN):      https://${publicHost}:${uiPort}"
}
Write-Host "  Note: if your browser warns about a local certificate, accept/trust it to enable microphone access."
