#Requires -Version 5.1
# Fail closed before a Windows release is uploaded.
[CmdletBinding()]
param([string]$ExpectedSubject = $env:EMULSION_SIGN_EXPECTED_SUBJECT)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security') -ErrorAction Stop
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root 'target/release/emulsion.exe'
$installers = @(Get-ChildItem "$root/target/windows/*-setup.exe" -File)
if (-not (Test-Path -LiteralPath $app) -or $installers.Count -eq 0) {
    throw 'Missing executable or installer to verify.'
}
if (-not $ExpectedSubject) { throw 'Set EMULSION_SIGN_EXPECTED_SUBJECT to the certificate profile subject.' }
$files = @($app) + @($installers.FullName)
$dlls = @(Get-ChildItem "$root/target/release/*.dll" -File)
foreach ($path in ($files + @($dlls | ForEach-Object { $_.FullName }))) {
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) {
        throw "Invalid or untimestamped signature: $path ($($signature.Status))"
    }
    # Third-party runtime DLLs retain their original publisher signatures.
    if ($files -contains $path -and $signature.SignerCertificate.Subject -cne $ExpectedSubject) {
        throw "Unexpected publisher for $path : $($signature.SignerCertificate.Subject)"
    }
    Write-Host "Verified: $path"
}
