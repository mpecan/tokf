<#
.SYNOPSIS
    Provision a fresh Windows VM for building and testing tokf.

.DESCRIPTION
    Windows evaluation images expire after 90 days, so the VM is disposable by
    design. This script exists so recreating one is a single command rather
    than an afternoon: run it in a clean Windows 11 VM and it installs
    everything needed to build the workspace and run the Windows test suite.

    Installs: Git, rustup (honouring rust-toolchain.toml), Visual Studio Build
    Tools with the C++ workload, CMake and NASM. The last four are not
    optional — libsqlite3-sys, mlua-sys (Luau) and aws-lc-sys all compile C or
    C++ and need a real MSVC toolchain plus a linker.

.PARAMETER SkipBuildTools
    Skip the Visual Studio Build Tools install. Use when the VM already has
    them — it is by far the slowest step (several GB).

.PARAMETER RepoUrl
    Repository to clone. Defaults to the public tokf repo.

.EXAMPLE
    # In an elevated PowerShell in the fresh VM:
    iwr -useb https://raw.githubusercontent.com/mpecan/tokf/main/scripts/provision-windows-dev.ps1 | iex

.NOTES
    Run as Administrator. Written for Windows 11 (x64 or ARM64) with winget
    available, which is the default on current images.
#>

[CmdletBinding()]
param(
    [switch]$SkipBuildTools,
    [string]$RepoUrl = 'https://github.com/mpecan/tokf.git'
)

$ErrorActionPreference = 'Stop'

function Write-Step {
    param([string]$Message)
    Write-Host "`n=== $Message ===" -ForegroundColor Cyan
}

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-Admin)) {
    throw 'Run this in an elevated PowerShell — the Build Tools install needs Administrator.'
}

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw 'winget not found. Install "App Installer" from the Microsoft Store, then re-run.'
}

# ARM64 hosts need the ARM64 compilers; x64 needs NASM for aws-lc-sys assembly.
$arch = $env:PROCESSOR_ARCHITECTURE
Write-Step "Detected architecture: $arch"

function Install-Package {
    param([string]$Id, [string]$Label)
    Write-Host "Installing $Label ($Id)..."
    # --accept-* keeps this non-interactive; a package already present is not an error.
    winget install --id $Id --exact --silent --disable-interactivity `
        --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        throw "$Label failed to install (winget exit $LASTEXITCODE)"
    }
}

Write-Step 'Core tools'
Install-Package -Id 'Git.Git'        -Label 'Git'
Install-Package -Id 'Kitware.CMake'  -Label 'CMake'      # aws-lc-sys
Install-Package -Id 'Rustlang.Rustup' -Label 'rustup'

if ($arch -eq 'AMD64') {
    # aws-lc-sys assembles x86-64 sources with NASM. Not used on ARM64.
    Install-Package -Id 'NASM.NASM' -Label 'NASM'
    $nasm = 'C:\Program Files\NASM'
    if (Test-Path $nasm) {
        $env:PATH = "$nasm;$env:PATH"
        [Environment]::SetEnvironmentVariable('PATH', "$nasm;$([Environment]::GetEnvironmentVariable('PATH','Machine'))", 'Machine')
    }
}

if (-not $SkipBuildTools) {
    Write-Step 'Visual Studio Build Tools (slow — several GB)'
    # The C++ workload brings MSVC and the Windows SDK. On ARM64 the ARM64
    # compilers are a separate component and are NOT in --includeRecommended.
    $components = @(
        '--add', 'Microsoft.VisualStudio.Workload.VCTools',
        '--includeRecommended'
    )
    if ($arch -eq 'ARM64') {
        $components += @('--add', 'Microsoft.VisualStudio.Component.VC.Tools.ARM64')
    }
    $override = ($components + @('--wait', '--quiet', '--norestart')) -join ' '

    winget install --id 'Microsoft.VisualStudio.2022.BuildTools' --exact --silent `
        --disable-interactivity --accept-package-agreements --accept-source-agreements `
        --override $override
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        throw "Build Tools failed to install (winget exit $LASTEXITCODE)"
    }
}

# winget updates the machine PATH but not this session's.
Write-Step 'Refreshing PATH for this session'
$env:PATH = [Environment]::GetEnvironmentVariable('PATH', 'Machine') + ';' +
            [Environment]::GetEnvironmentVariable('PATH', 'User')

Write-Step 'Cloning the repository'
$repoDir = Join-Path $HOME 'tokf'
if (Test-Path $repoDir) {
    Write-Host "$repoDir already exists — pulling instead of cloning."
    git -C $repoDir pull --ff-only
} else {
    git clone $RepoUrl $repoDir
}
Set-Location $repoDir

# rustup reads rust-toolchain.toml here and installs the pinned toolchain,
# so the VM matches CI without the version being duplicated in this script.
Write-Step 'Installing the pinned Rust toolchain'
rustup show

Write-Step 'Done'
Write-Host @"
Provisioned. To run the same checks the Windows CI job runs:

    cd $repoDir
    cargo clippy --locked -p tokf --all-targets -- -D warnings
    cargo test   --locked -p tokf --lib
    cargo test   --locked -p tokf --bin tokf

The first build compiles Luau, SQLite and aws-lc from source and will take a
while. Snapshot the VM once this succeeds — that is the point you want to
return to after the evaluation licence expires.
"@ -ForegroundColor Green
