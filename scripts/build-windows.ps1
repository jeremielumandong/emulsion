#Requires -Version 5.1
<#
.SYNOPSIS
Build Emulsion on Windows (release by default).
.EXAMPLE
.\scripts\build-windows.ps1
.EXAMPLE
.\scripts\build-windows.ps1 -Configuration Debug
.EXAMPLE
.\scripts\build-windows.ps1 -Package
.EXAMPLE
.\scripts\build-windows.ps1 -Sign
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',
    # Like AgentOps, packaging always builds a release executable.
    [switch]$Package,
    # Sign the application, embedded uninstaller, and installer with Azure.
    [switch]$Sign
)

$ErrorActionPreference = 'Stop'

function Copy-Licenses {
    param([string]$Repository, [string]$Destination)

    $files = @(Get-Content -LiteralPath (Join-Path $Repository 'packaging/license-files.txt') |
        Where-Object { $_ -and -not $_.StartsWith('#') })
    foreach ($file in $files) {
        $source = Join-Path $Repository $file
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or
            (Get-Item -LiteralPath $source).Length -eq 0) {
            throw "Required license or notice is missing or empty: $file"
        }
    }
    foreach ($file in $files) {
        $target = Join-Path $Destination $file
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $Repository $file) -Destination $target -Force
    }
}

function Copy-VcRuntime {
    param([string]$Executable, [string]$Destination)

    $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($Executable))
    try {
        $reader.BaseStream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        $reader.BaseStream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Invalid PE executable: $Executable" }
        $architecture = switch ($reader.ReadUInt16()) {
            0x8664 { 'x64' }
            0x014c { 'x86' }
            0xaa64 { 'arm64' }
            default { throw 'Unsupported Windows executable architecture.' }
        }
    } finally { $reader.Dispose() }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $installations = @()
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $installations = @(& $vswhere -all -prerelease -products '*' -property installationPath)
    }
    if ($env:VSINSTALLDIR) { $installations += $env:VSINSTALLDIR }
    $candidates = @(
        foreach ($installation in ($installations | Select-Object -Unique)) {
            $redistRoot = Join-Path $installation 'VC/Redist/MSVC'
            foreach ($version in (Get-ChildItem -LiteralPath $redistRoot -Directory -ErrorAction SilentlyContinue)) {
                $architectureDir = Join-Path $version.FullName $architecture
                Get-ChildItem -LiteralPath $architectureDir -Directory -Filter 'Microsoft.VC*.CRT' -ErrorAction SilentlyContinue
            }
        }
    ) | Sort-Object { [version]$_.Parent.Parent.Name } -Descending
    $required = @('msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll')
    if ($architecture -ne 'x86') { $required += 'vcruntime140_1.dll' }
    $runtimeDir = $candidates | Where-Object {
        $candidate = $_.FullName
        @($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $candidate $_) -PathType Leaf) }).Count -eq 0
    } | Select-Object -First 1
    if (-not $runtimeDir) {
        throw "Visual C++ $architecture redistributable DLLs not found. Install the Visual Studio C++ build tools and redistributable component."
    }

    # Use Microsoft's redistributable directory, not the machine's System32.
    # Copy the complete CRT set so its own dependencies are packaged as well.
    foreach ($dll in (Get-ChildItem -LiteralPath $runtimeDir.FullName -Filter '*.dll' -File)) {
        if ($dll.Length -eq 0) { throw "Empty Visual C++ runtime: $($dll.FullName)" }
        Copy-Item -LiteralPath $dll.FullName -Destination $Destination -Force
    }
    foreach ($name in $required) {
        $staged = Get-Item -LiteralPath (Join-Path $Destination $name)
        if ($staged.Length -eq 0) { throw "Empty staged Visual C++ runtime: $name" }
    }
    Write-Host "Bundled Visual C++ $architecture runtime from $($runtimeDir.FullName)"
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This script must run on Windows.'
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Cargo is required. Install Rust with the MSVC toolchain and restart PowerShell.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$targetDir = Join-Path $repoRoot 'target'
Remove-Item Env:EMULSION_SIGN_ENABLED -ErrorAction SilentlyContinue
if ($Sign) {
    $Package = $true
    . (Join-Path $PSScriptRoot 'lib/trusted-signing.ps1')
    $signing = New-TrustedSigningContext
    $env:EMULSION_SIGN_SIGNTOOL = $signing.SignTool
    $env:EMULSION_SIGN_DLIB = $signing.Dlib
    $env:EMULSION_SIGN_METADATA = $signing.Metadata
}
if ($Package) {
    $Configuration = 'Release'
    if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) {
        throw 'NSIS packaging requires cargo-packager. Run: cargo install cargo-packager --version 0.11.8 --locked'
    }
}
$cargoArgs = @('build', '--locked', '-p', 'emulsion-app', '--bin', 'emulsion', '--target-dir', $targetDir)
if ($Configuration -eq 'Release') {
    $cargoArgs += '--release'
}
$expectedBinary = [IO.Path]::GetFullPath((Join-Path $targetDir ($Configuration.ToLowerInvariant() + '/emulsion.exe')))
$runningApp = Get-Process -Name emulsion -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $expectedBinary }
if ($runningApp) {
    throw 'Save your work and close the running Emulsion build before rebuilding; Windows locks its executable.'
}

# Run from the workspace so rustup and Cargo find its toolchain and configuration.
Push-Location -LiteralPath $repoRoot
try {
    Write-Host "Building Emulsion ($Configuration)..."
    if ($Package) {
        # Capture the actual artifact path so a custom Cargo target cannot
        # accidentally package an older native executable left in target/release.
        $buildMessages = & cargo @cargoArgs --message-format=json-render-diagnostics
    } else {
        & cargo @cargoArgs
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE."
    }
    Write-Host 'Emulsion build completed successfully. Output is under target/.'
    if ($Package) {
        $binaryDir = Join-Path $targetDir 'release'
        $binary = Join-Path $binaryDir 'emulsion.exe'
        $builtExecutables = @($buildMessages | ForEach-Object {
            $message = $_ | ConvertFrom-Json
            if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'emulsion' -and $message.executable) {
                [IO.Path]::GetFullPath($message.executable)
            }
        })
        if ($builtExecutables.Count -ne 1 -or $builtExecutables[0] -ne $binary) {
            throw 'NSIS packaging requires the native target/release build. Remove custom Cargo build.target or CARGO_BUILD_TARGET settings.'
        }
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
            throw "Missing $binary. Packaging expects the native Windows build without a custom Cargo build.target."
        }
        # ort supplies this runtime as a symlink; require readable bytes, not
        # FileInfo.Length (which is zero for a Windows symlink).
        $runtime = Join-Path $binaryDir 'DirectML.dll'
        if (-not (Test-Path -LiteralPath $runtime -PathType Leaf)) {
            throw "Missing runtime dependency: $runtime"
        }
        $runtimeStream = [IO.File]::OpenRead($runtime)
        try {
            if ($runtimeStream.Length -eq 0) { throw "Runtime dependency is empty: $runtime" }
        } finally { $runtimeStream.Dispose() }

        Copy-VcRuntime -Executable $binary -Destination $binaryDir
        Copy-Licenses -Repository $repoRoot -Destination (Join-Path $binaryDir 'licenses')

        $outDir = Join-Path $targetDir 'windows'
        if ($Sign) {
            Get-ChildItem -LiteralPath $outDir -Filter '*-setup.exe' -File -ErrorAction SilentlyContinue |
                Remove-Item -Force
            $env:EMULSION_SIGN_ENABLED = '1'
        }
        Write-Host 'Creating NSIS installer (downloads NSIS on first use)...'
        & cargo packager --release --manifest-path (Join-Path $repoRoot 'crates/emulsion-app/Cargo.toml') --formats nsis --binaries-dir $binaryDir --out-dir $outDir
        if ($LASTEXITCODE -ne 0) { throw "NSIS packaging failed with exit code $LASTEXITCODE." }
        Write-Host "Installer created in $outDir"
        if ($Sign) {
            $psHost = (Get-Process -Id $PID).Path
            & $psHost -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'verify-windows-signatures.ps1')
            if ($LASTEXITCODE -ne 0) { throw 'Signature verification failed; do not ship these artifacts.' }
        }
    }
} finally {
    Remove-Item Env:EMULSION_SIGN_ENABLED -ErrorAction SilentlyContinue
    Pop-Location
}
