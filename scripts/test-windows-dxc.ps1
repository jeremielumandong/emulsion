#Requires -Version 5.1
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib/windows-dxc.ps1')
$repo = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $repo 'target/dxc'
$testDirectory = Join-Path $repo ('target/dxc-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
# A minimal PE header lets us check architecture selection without running code.
foreach ($architecture in @('x64', 'arm64', 'x86')) {
    $directory = Join-Path $testDirectory $architecture
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $executable = Join-Path $directory 'fixture.exe'
    $bytes = [byte[]]::new(70)
    [BitConverter]::GetBytes([int]64).CopyTo($bytes, 60)
    [BitConverter]::GetBytes([int]0x00004550).CopyTo($bytes, 64)
    $machine = @{ x64 = 0x8664; arm64 = 0xaa64; x86 = 0x014c }[$architecture]
    [BitConverter]::GetBytes([uint16]$machine).CopyTo($bytes, 68)
    [IO.File]::WriteAllBytes($executable, $bytes)
    Install-WindowsDxc -Executable $executable -CacheDirectory $cache
    foreach ($name in @('dxcompiler.dll', 'dxil.dll')) {
        $dll = Join-Path $directory $name
        $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($dll))
        try {
            $reader.BaseStream.Position = 60
            $reader.BaseStream.Position = $reader.ReadInt32() + 4
            if ($reader.ReadUInt16() -ne $machine) { throw "Wrong architecture: $dll" }
        } finally { $reader.Dispose() }
    }
    foreach ($name in @('LICENSE-LLVM.txt', 'LICENSE-MIT.txt', 'LICENSE-MS.txt')) {
        if ((Get-Item -LiteralPath (Join-Path $directory "licenses/dxc/$name")).Length -eq 0) {
            throw "Missing DXC notice: $name"
        }
    }
}
$badCache = Join-Path $testDirectory 'invalid-cache'
New-Item -ItemType Directory -Path $badCache -Force | Out-Null
[IO.File]::WriteAllText((Join-Path $badCache 'dxc_2025_07_14.zip'), 'corrupt archive')
try {
    Install-WindowsDxc -Executable $executable -CacheDirectory $badCache
    throw 'Accepted a corrupt DXC archive'
} catch {
    if ($_.Exception.Message -notlike 'DXC checksum mismatch:*') { throw }
}
Write-Host 'DXC staging checks passed: x64, arm64, x86, notices, and corrupt-archive rejection.'
