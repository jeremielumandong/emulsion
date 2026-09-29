# Pinned Microsoft DXC release; wgpu 29 requires DXC 1.8.2502 or newer.
# The verified archive supplies the compiler, validator, and their notices.
function Install-WindowsDxc {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$CacheDirectory
    )

    $ErrorActionPreference = 'Stop'

    $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($Executable))
    try {
        $reader.BaseStream.Position = 0x3c
        $reader.BaseStream.Position = $reader.ReadInt32()
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Invalid PE executable: $Executable" }
        $architecture = switch ($reader.ReadUInt16()) {
            0x8664 { 'x64' }
            0x014c { 'x86' }
            0xaa64 { 'arm64' }
            default { throw 'Unsupported Windows executable architecture.' }
        }
    } finally { $reader.Dispose() }

    $name = 'dxc_2025_07_14.zip'
    $sha256 = '9ad895a6b039e3a8f8c22a1009f866800b840a74b50db9218d13319e215ea8a4'
    New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
    $archivePath = Join-Path $CacheDirectory $name
    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
        Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.8.2505.1/$name" -OutFile $archivePath
    }
    $hasher = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($archivePath)
    try { $actualHash = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $hasher.Dispose() }
    if ($actualHash -ne $sha256) {
        throw "DXC checksum mismatch: $archivePath. Remove the invalid cached archive and retry."
    }

    $destination = Split-Path -Parent ([IO.Path]::GetFullPath($Executable))
    $licenses = Join-Path $destination 'licenses/dxc'
    New-Item -ItemType Directory -Path $licenses -Force | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        # Extract only fixed paths from the verified archive.
        $files = @{
            "bin/$architecture/dxcompiler.dll" = (Join-Path $destination 'dxcompiler.dll')
            "bin/$architecture/dxil.dll" = (Join-Path $destination 'dxil.dll')
            'LICENSE-LLVM.txt' = (Join-Path $licenses 'LICENSE-LLVM.txt')
            'LICENSE-MIT.txt' = (Join-Path $licenses 'LICENSE-MIT.txt')
            'LICENSE-MS.txt' = (Join-Path $licenses 'LICENSE-MS.txt')
        }
        foreach ($source in $files.Keys) {
            $entry = $archive.GetEntry($source)
            if (-not $entry -or $entry.Length -eq 0) { throw "Missing DXC archive entry: $source" }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $files[$source], $true)
        }
    } finally { $archive.Dispose() }
    Write-Host "Bundled Microsoft DXC 1.8.2505.1 ($architecture) in $destination"
}
