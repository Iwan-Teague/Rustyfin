#Requires -Version 5.1
<#
.SYNOPSIS
    Create a shallow zip archive of the repository at a given git ref.
.DESCRIPTION
    Usage:
      .\make_shallow_zip.ps1              # archives HEAD to <repo>-<sha>.zip
      .\make_shallow_zip.ps1 <ref>        # archives <ref> (e.g. main, HEAD, v1.2.3)
      .\make_shallow_zip.ps1 <ref> <out.zip>
    Env:
      SHALLOW_ZIP_EXTRA_EXCLUDES="path1,path2"  # optional comma-separated extra excludes
.PARAMETER Ref
    Git ref to archive (default: HEAD).
.PARAMETER Out
    Output zip path (default: <repo>-<short-sha>.zip in repo root).
#>
param(
    [string]$Ref = "HEAD",
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "Error: git is not installed or not in PATH." -ForegroundColor Red
    Write-Host "git is required to create a shallow zip archive." -ForegroundColor Yellow
    $yn = Read-Host "Install git now via winget? [y/N]"
    if ($yn -match '^[Yy]') {
        winget install Microsoft.Git
        Write-Host "git installation complete. Please restart this script." -ForegroundColor Cyan
    }
    exit 1
}

# Resolve repo root via git
$root = git rev-parse --show-toplevel 2>$null
if ($LASTEXITCODE -ne 0 -or -not $root) {
    Write-Host "Error: not inside a git repository." -ForegroundColor Red
    exit 1
}
$root = $root.Trim()

$repoName = Split-Path -Leaf $root
Set-Location $root

if (-not $Out) {
    $shortSha = (git rev-parse --short $Ref 2>$null).Trim()
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: could not resolve ref '$Ref'." -ForegroundColor Red
        exit 1
    }
    $Out = "${repoName}-${shortSha}.zip"
}

$outPath = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $root $Out }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outPath) | Out-Null

$stageDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rustyfin_zip_" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null

try {
    # Use git archive --format=zip, then expand so we can strip exclude paths before re-zipping
    $archiveZip = Join-Path $stageDir "archive.zip"
    git archive --format=zip --prefix="${repoName}/" -o $archiveZip $Ref
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: git archive failed." -ForegroundColor Red
        exit 1
    }

    Expand-Archive -Path $archiveZip -DestinationPath $stageDir -Force
    Remove-Item $archiveZip -Force

    # Keep export lightweight by removing local/test/bootstrap artifacts if present.
    $excludePaths = @(
        ".tmp",
        ".npm-cache",
        ".playwright-browsers",
        "node_modules",
        "ui/node_modules",
        "ui/.next",
        "tests/node_modules",
        "tests/_runs",
        "scripts/rustfin.db",
        "scripts/rustfin.db-shm",
        "scripts/rustfin.db-wal"
    )

    if ($env:SHALLOW_ZIP_EXTRA_EXCLUDES) {
        $extra = $env:SHALLOW_ZIP_EXTRA_EXCLUDES -split ','
        $excludePaths += $extra | Where-Object { $_.Trim() -ne "" }
    }

    foreach ($rel in $excludePaths) {
        if (-not $rel) { continue }
        # Normalise separators so both forward and back slash work
        $relNorm = $rel.Replace('/', '\')
        $target  = Join-Path $stageDir "${repoName}\${relNorm}"
        if (Test-Path $target) {
            Remove-Item $target -Recurse -Force
        }
    }

    # Re-zip everything under $stageDir\$repoName\ into the output path
    $sourceFolder = Join-Path $stageDir $repoName
    Compress-Archive -Path "$sourceFolder\*" -DestinationPath $outPath -Force

    # Compress-Archive puts files at root; we need them under $repoName/ like the bash version.
    # Re-create with the folder itself so the zip contains $repoName/...
    Remove-Item $outPath -Force
    Push-Location $stageDir
    Compress-Archive -Path $repoName -DestinationPath $outPath
    Pop-Location

    Write-Host "Created: $outPath"
} finally {
    Remove-Item $stageDir -Recurse -Force -ErrorAction SilentlyContinue
}
