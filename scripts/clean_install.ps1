#Requires -Version 5.1
<#
.SYNOPSIS
    Wipe Rustyfin user-generated runtime data for a true "first run" state.
.DESCRIPTION
    After this script, running .\scripts\start.ps1 should require setup wizard again.
.PARAMETER Yes
    Skip interactive confirmation.
.PARAMETER File
    Compose file path (default: docker-compose.yml).
.PARAMETER Help
    Show this help.
#>
param(
    [switch]$Yes,
    [string]$File = "",
    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Write-Info    { param([string]$Msg) Write-Host "[clean-install] $Msg" -ForegroundColor Cyan }
function Write-Success { param([string]$Msg) Write-Host "[clean-install] $Msg" -ForegroundColor Green }
function Write-Warn    { param([string]$Msg) Write-Host "[clean-install] $Msg" -ForegroundColor Yellow }
function Write-Die     { param([string]$Msg) Write-Host "[clean-install] ERROR: $Msg" -ForegroundColor Red; exit 1 }

function Show-Usage {
    Write-Host @"
Usage:
  .\scripts\clean_install.ps1 [-Yes] [-File <compose-file>]

Options:
  -Yes         Skip interactive confirmation.
  -File        Compose file path (default: docker-compose.yml).
  -Help        Show this help.
"@
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

if (-not $Yes) {
    Write-Host ""
    Write-Warn "This will DELETE Rustyfin runtime/user data (DB, cache, transcode, volumes)."
    Write-Warn "After this, start.ps1 will boot as a first-time install."
    Write-Host ""
    $confirm = Read-Host "Type 'yes' to continue"
    if ($confirm -ne "yes") { Write-Info "Aborted."; exit 0 }
}

Write-Info "Stopping stack and removing compose volumes..."
docker compose -f $ComposeFile down --remove-orphans --volumes

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

# Local runtime/state paths (for non-docker or mixed usage)
function Remove-FileIfExists {
    param([string]$Path)
    if (Test-Path $Path -PathType Leaf) {
        Remove-Item $Path -Force
        Write-Info "Deleted file: $Path"
    }
}

function Remove-DirIfExists {
    param([string]$Path)
    if (Test-Path $Path -PathType Container) {
        Remove-Item $Path -Recurse -Force
        Write-Info "Deleted dir: $Path"
    }
}

Remove-FileIfExists (Join-Path $RepoRoot "rustfin.db")
Remove-FileIfExists (Join-Path $RepoRoot "rustfin.db-shm")
Remove-FileIfExists (Join-Path $RepoRoot "rustfin.db-wal")
Remove-FileIfExists (Join-Path $RepoRoot ".rustyfin.runtime.env")
Remove-FileIfExists (Join-Path $SafeTmpDir "directory-picker-helper.py")
Remove-FileIfExists (Join-Path $SafeTmpDir "directory-picker-helper.log")

Remove-FileIfExists (Join-Path $RepoRoot "scripts\rustfin.db")
Remove-FileIfExists (Join-Path $RepoRoot "scripts\rustfin.db-shm")
Remove-FileIfExists (Join-Path $RepoRoot "scripts\rustfin.db-wal")

$winTemp = [System.IO.Path]::GetTempPath().TrimEnd('\')
Remove-DirIfExists (Join-Path $winTemp "rustfin_cache")
Remove-DirIfExists (Join-Path $winTemp "rustfin_transcode")
Remove-DirIfExists (Join-Path $RepoRoot "tests\_runs")

Write-Success "Clean install reset complete."
Write-Host "Next step:"
Write-Host "  .\scripts\start.ps1"
