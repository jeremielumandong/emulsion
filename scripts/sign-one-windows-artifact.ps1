#Requires -Version 5.1
# cargo-packager invokes this for the app, embedded NSIS uninstaller and setup.
[CmdletBinding()]
param([Parameter(Mandatory, Position = 0, ValueFromRemainingArguments)][string[]]$Path)
$ErrorActionPreference = 'Stop'
if ($env:EMULSION_SIGN_ENABLED -ne '1') { exit 0 }
try {
    . "$PSScriptRoot/lib/trusted-signing.ps1"
    Invoke-TrustedSign -Context (New-TrustedSigningContext) -Path $Path
} catch {
    Write-Error $_
    exit 1
}
exit 0
