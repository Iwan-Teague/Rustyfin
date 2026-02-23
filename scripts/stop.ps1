#Requires -Version 5.1
<#
.SYNOPSIS
    Stop and remove the Rustyfin Docker stack (containers + network).
.DESCRIPTION
    Does not remove persistent volumes.
.PARAMETER File
    Compose file path (default: docker-compose.yml).
.PARAMETER Help
    Show this help.
#>
param(
    [string]$File = "",
    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Write-Info    { param([string]$Msg) Write-Host "[stop] $Msg" -ForegroundColor Cyan }
function Write-Success { param([string]$Msg) Write-Host "[stop] $Msg" -ForegroundColor Green }
function Write-Die     { param([string]$Msg) Write-Host "[stop] ERROR: $Msg" -ForegroundColor Red; exit 1 }

function Show-Usage {
    Write-Host "Usage: .\scripts\stop.ps1 [-File <compose-file>]"
}

if ($Help) { Show-Usage; exit 0 }

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot    = Split-Path -Parent $ScriptDir
$ComposeFile = if ($File) { $File } else { Join-Path $RepoRoot "docker-compose.yml" }

if (-not [System.IO.Path]::IsPathRooted($ComposeFile)) {
    $ComposeFile = Join-Path $RepoRoot $ComposeFile
}

Set-Location $RepoRoot

if (-not (Test-Path $ComposeFile))                          { Write-Die "docker-compose.yml not found at $ComposeFile" }
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Write-Die "docker is not installed or not in PATH" }
docker compose version 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Die "docker compose is not available" }

$SafeTmpDir = if ($env:RUSTFIN_TMPDIR) { $env:RUSTFIN_TMPDIR } else { Join-Path $RepoRoot ".tmp" }
New-Item -ItemType Directory -Force -Path $SafeTmpDir | Out-Null
if (-not (Test-Path $SafeTmpDir)) { Write-Die "Failed to create temp dir: $SafeTmpDir" }

Write-Info "Stopping Rustyfin stack..."
docker compose -f $ComposeFile down --remove-orphans

# Stop picker helper by PID file
$PickerHelperPidFile = Join-Path $SafeTmpDir "directory-picker-helper.pid"
if (Test-Path $PickerHelperPidFile) {
    $helperPid = Get-Content $PickerHelperPidFile -ErrorAction SilentlyContinue
    if ($helperPid) {
        $proc = Get-Process -Id ([int]$helperPid) -ErrorAction SilentlyContinue
        if ($proc) {
            Write-Info "Stopping directory picker helper (pid $helperPid)..."
            Stop-Process -Id ([int]$helperPid) -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-Item $PickerHelperPidFile -Force -ErrorAction SilentlyContinue
}

# Stop any remaining picker helper listeners on the port
$PickerHelperPort = if ($env:RUSTFIN_PICKER_HELPER_PORT) { [int]$env:RUSTFIN_PICKER_HELPER_PORT } else { 43110 }
$listeners = Get-NetTCPConnection -LocalPort $PickerHelperPort -State Listen -ErrorAction SilentlyContinue
if ($listeners) {
    Write-Info "Stopping picker helper listener(s) on port ${PickerHelperPort}..."
    foreach ($conn in $listeners) {
        Stop-Process -Id $conn.OwningProcess -Force -ErrorAction SilentlyContinue
    }
}

Write-Success "Rustyfin stack stopped."
