#Requires -Version 5.1
# Contract tests use fake files/signatures and never contact Azure or sign a file.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("emulsion-signing-test-" + [guid]::NewGuid())
$saved = @{}
$names = @('AZURE_CODESIGNING_ENDPOINT', 'AZURE_CODESIGNING_ACCOUNT', 'AZURE_CODESIGNING_PROFILE',
    'EMULSION_SIGN_SIGNTOOL', 'EMULSION_SIGN_DLIB', 'EMULSION_SIGN_AZURE_CLI_ONLY', 'EMULSION_SIGN_ENABLED',
    'EMULSION_SIGN_EXPECTED_SUBJECT', 'EMULSION_SIGN_METADATA', 'AZURE_CLIENT_SECRET', 'EMULSION_DOTENV_UNRELATED')
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
function Assert-True($Condition, $Message) { if (-not $Condition) { throw $Message } }
function Assert-Rejected([scriptblock]$Action, [string]$Pattern) {
    try { & $Action } catch {
        Assert-True ($_.Exception.Message -match $Pattern) "Unexpected error: $_"
        return
    }
    throw "Expected rejection matching: $Pattern"
}
try {
    New-Item -ItemType Directory -Path "$fixture/scripts/lib", "$fixture/target/windows", "$fixture/target/release" -Force | Out-Null
    Copy-Item "$PSScriptRoot/lib/trusted-signing.ps1" "$fixture/scripts/lib/"
    Copy-Item "$PSScriptRoot/verify-windows-signatures.ps1" "$fixture/scripts/"
    Copy-Item "$PSScriptRoot/build-windows.ps1" "$fixture/scripts/"
    foreach ($path in @('dlib.dll', 'target/release/emulsion.exe', 'target/release/runtime.dll', 'target/windows/test-setup.exe')) {
        Set-Content -LiteralPath "$fixture/$path" -Value 'fixture'
    }
    $env:AZURE_CODESIGNING_ENDPOINT = 'https://eus.codesigning.azure.net'
    $env:AZURE_CODESIGNING_ACCOUNT = 'test-account'
    $env:AZURE_CODESIGNING_PROFILE = 'test-profile'
    $env:EMULSION_SIGN_SIGNTOOL = (Get-Process -Id $PID).Path
    $env:EMULSION_SIGN_DLIB = "$fixture/dlib.dll"
    $env:EMULSION_SIGN_AZURE_CLI_ONLY = '1'
    . "$fixture/scripts/lib/trusted-signing.ps1"
    $context = New-TrustedSigningContext
    $metadata = Get-Content -LiteralPath $context.Metadata -Raw | ConvertFrom-Json
    Assert-True ($metadata.CodeSigningAccountName -eq 'test-account') 'Wrong signing account'
    Assert-True ($metadata.CertificateProfileName -eq 'test-profile') 'Wrong signing profile'
    Assert-True ($metadata.ExcludeCredentials -contains 'ManagedIdentityCredential') 'Managed identity must be excluded in CI'
    Assert-True ($metadata.ExcludeCredentials -notcontains 'AzureCliCredential') 'Azure CLI must remain enabled'
    $env:AZURE_CODESIGNING_ACCOUNT = ''
    Assert-Rejected { New-TrustedSigningContext } 'Missing signing setting'
    $env:AZURE_CODESIGNING_ACCOUNT = 'test-account'

    # Exercise local configuration through the same entry point as build/CI.
    @'
# Signing settings only; unrelated variables must not be imported.
AZURE_CODESIGNING_ENDPOINT=https://example.invalid
AZURE_CODESIGNING_ACCOUNT=dotenv-account
export AZURE_CODESIGNING_PROFILE="dotenv-profile" # comment
EMULSION_SIGN_EXPECTED_SUBJECT='CN=Local Publisher, O=Local Org'
AZURE_CLIENT_SECRET='literal$env:USERNAME$(throw "must not execute")=with#hash'
EMULSION_DOTENV_UNRELATED=must-not-load
'@ | Set-Content -LiteralPath "$fixture/.env" -Encoding UTF8
    'AZURE_CODESIGNING_PROFILE=local-profile # override' |
        Set-Content -LiteralPath "$fixture/.env.local" -Encoding UTF8
    foreach ($name in @('AZURE_CODESIGNING_ACCOUNT', 'AZURE_CODESIGNING_PROFILE',
        'EMULSION_SIGN_EXPECTED_SUBJECT', 'AZURE_CLIENT_SECRET', 'EMULSION_DOTENV_UNRELATED')) {
        [Environment]::SetEnvironmentVariable($name, $null)
    }
    Push-Location -LiteralPath $env:TEMP
    try { $context = New-TrustedSigningContext } finally { Pop-Location }
    $metadata = Get-Content -LiteralPath $context.Metadata -Raw | ConvertFrom-Json
    Assert-True ($metadata.Endpoint -eq 'https://eus.codesigning.azure.net') 'Shell/CI endpoint was overwritten'
    Assert-True ($metadata.CodeSigningAccountName -eq 'dotenv-account') 'Dotenv account was not loaded'
    Assert-True ($metadata.CertificateProfileName -eq 'local-profile') 'Dotenv local override was not loaded'
    Assert-True ($env:EMULSION_SIGN_EXPECTED_SUBJECT -eq 'CN=Local Publisher, O=Local Org') 'Quoted subject was not preserved'
    Assert-True ($env:AZURE_CLIENT_SECRET -ceq 'literal$env:USERNAME$(throw "must not execute")=with#hash') 'Credential value was expanded or truncated'
    Assert-True (-not $env:EMULSION_DOTENV_UNRELATED) 'Unrelated dotenv variable was imported'
    $hostExe = (Get-Process -Id $PID).Path
    'if ($env:EMULSION_SIGN_EXPECTED_SUBJECT -ne "CN=Local Publisher, O=Local Org") { exit 1 }' |
        Set-Content -LiteralPath "$fixture/check-inherited-env.ps1"
    & $hostExe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$fixture/check-inherited-env.ps1"
    Assert-True ($LASTEXITCODE -eq 0) 'Verification subprocess did not inherit signing settings'
    Remove-Item -LiteralPath "$fixture/.env.local"
    $env:AZURE_CODESIGNING_PROFILE = ''
    $null = New-TrustedSigningContext
    Assert-True ($env:AZURE_CODESIGNING_PROFILE -eq 'dotenv-profile') 'Double quotes or inline comment were not removed'
    'AZURE_CLIENT_SECRET="unterminated-secret' | Set-Content -LiteralPath "$fixture/.env.local"
    Assert-Rejected { New-TrustedSigningContext } 'Invalid quoted signing setting in .env.local at line 1\.'
    Remove-Item -LiteralPath "$fixture/.env", "$fixture/.env.local"

    # Packaging after OIDC login must never trigger another compilation. Mock
    # Cargo and stop at the runtime gate so this test needs no SDK or compiler.
    function cargo { throw 'Cargo invoked by test' }
    function cargo-packager { }
    $build = "$fixture/scripts/build-windows.ps1"
    Assert-Rejected { & $build -SkipBuild } 'requires -Package or -Sign'
    Assert-Rejected { & $build -Package } 'Cargo invoked by test'
    Assert-Rejected { & $build -Package -SkipBuild } 'Missing runtime dependency'
    Assert-Rejected { & $build -Sign -SkipBuild } 'Missing runtime dependency'
    Remove-Item -LiteralPath "$fixture/target/release/emulsion.exe"
    Assert-Rejected { & $build -Sign -SkipBuild } 'Missing .*emulsion\.exe'
    Set-Content -LiteralPath "$fixture/target/release/emulsion.exe" -Value 'fixture'

    # The inactive packaging hook must work without signing configuration.
    $env:EMULSION_SIGN_ENABLED = ''
    $hostExe = (Get-Process -Id $PID).Path
    & $hostExe -NoProfile -NonInteractive -File "$PSScriptRoot/sign-one-windows-artifact.ps1" missing.exe
    Assert-True ($LASTEXITCODE -eq 0) 'Unsigned packaging hook failed'

    # Mock only Windows signature inspection; exercise the real verification gate.
    function Import-Module { param($Name, $ErrorAction) }
    $global:emulsionTestSignatureStatus = 'Valid'
    $global:emulsionTestTimestamp = $true
    $global:emulsionTestSubject = 'CN=Expected Publisher'
    function Get-AuthenticodeSignature {
        param($LiteralPath)
        [pscustomobject]@{
            Status = $global:emulsionTestSignatureStatus
            TimeStamperCertificate = $(if ($global:emulsionTestTimestamp) { 'timestamp' } else { $null })
            SignerCertificate = [pscustomobject]@{
                Subject = $(if ($LiteralPath.EndsWith('.dll')) { 'CN=Third Party' } else { $global:emulsionTestSubject })
            }
        }
    }
    $verify = "$fixture/scripts/verify-windows-signatures.ps1"
    & $verify -ExpectedSubject 'CN=Expected Publisher'
    $global:emulsionTestSubject = 'CN=Wrong Publisher'
    Assert-Rejected { & $verify -ExpectedSubject 'CN=Expected Publisher' } 'Unexpected publisher'
    $global:emulsionTestSubject = 'CN=Expected Publisher'
    $global:emulsionTestTimestamp = $false
    Assert-Rejected { & $verify -ExpectedSubject 'CN=Expected Publisher' } 'untimestamped'
    $global:emulsionTestTimestamp = $true
    $global:emulsionTestSignatureStatus = 'NotSigned'
    Assert-Rejected { & $verify -ExpectedSubject 'CN=Expected Publisher' } 'Invalid'
    $global:emulsionTestSignatureStatus = 'Valid'
    Assert-Rejected { & $verify -ExpectedSubject '' } 'EXPECTED_SUBJECT'
    Remove-Item "$fixture/target/windows/test-setup.exe"
    Assert-Rejected { & $verify -ExpectedSubject 'CN=Expected Publisher' } 'Missing executable or installer'
    Write-Host 'Windows signing contract tests passed (no Azure calls).'
} finally {
    Remove-Variable emulsionTestSignatureStatus, emulsionTestTimestamp, emulsionTestSubject -Scope Global -ErrorAction SilentlyContinue
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
