# Azure Artifact Signing via the Windows SDK SignTool and Microsoft's dlib.
# Uses Azure credentials locally or the authenticated Azure CLI session in CI.
Set-StrictMode -Version Latest

function Import-TrustedSigningEnvironment {
    param([Parameter(Mandatory)][string]$Repository)

    # Parse data only: never execute dotenv contents or expand credential values.
    $settings = @{}
    foreach ($file in @('.env', '.env.local')) {
        $path = Join-Path $Repository $file
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
        $lineNumber = 0
        foreach ($line in (Get-Content -LiteralPath $path -Encoding UTF8)) {
            $lineNumber++
            if ($line -notmatch '^\s*(?:export\s+)?((?:AZURE_|EMULSION_SIGN)[A-Za-z0-9_]*)\s*=(.*)$') { continue }
            $name = $Matches[1]
            $value = $Matches[2].Trim()
            if ($value.StartsWith('"') -or $value.StartsWith("'")) {
                $quote = [regex]::Escape($value.Substring(0, 1))
                if ($value -notmatch "^$quote(.*?)$quote\s*(?:#.*)?$") {
                    throw "Invalid quoted signing setting in ${file} at line $lineNumber."
                }
                $value = $Matches[1]
            } else {
                $value = ($value -replace '\s+#.*$', '').TrimEnd()
            }
            $settings[$name] = $value
        }
    }
    foreach ($name in $settings.Keys) {
        # Existing shell / GitHub settings always take precedence.
        if ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable($name))) {
            [Environment]::SetEnvironmentVariable($name, $settings[$name], 'Process')
        }
    }
}

function New-TrustedSigningContext {
    $root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    Import-TrustedSigningEnvironment -Repository $root
    foreach ($name in @('AZURE_CODESIGNING_ENDPOINT', 'AZURE_CODESIGNING_ACCOUNT', 'AZURE_CODESIGNING_PROFILE')) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
            throw "Missing signing setting: $name. Set it in the repository .env or PowerShell environment."
        }
    }
    $signTool = $env:EMULSION_SIGN_SIGNTOOL
    if (-not $signTool) {
        $sdk = "${env:ProgramFiles(x86)}/Windows Kits/10/bin"
        $candidate = Get-ChildItem $sdk -Filter signtool.exe -Recurse |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $candidate) { throw 'Install the Windows SDK x64 signing tools.' }
        $signTool = $candidate.FullName
    }
    if (-not (Test-Path -LiteralPath $signTool)) { throw 'SignTool does not exist.' }
    $dlib = $env:EMULSION_SIGN_DLIB
    if (-not $dlib) {
        $package = 'microsoft.trusted.signing.client'
        $index = Invoke-RestMethod "https://api.nuget.org/v3-flatcontainer/$package/index.json"
        $versions = @($index.versions | Where-Object { $_ -notmatch '-' })
        if ($versions.Count -eq 0) { throw 'No stable signing client available.' }
        $version = $versions[-1]
        $cache = Join-Path $env:LOCALAPPDATA "Emulsion/trusted-signing/$version"
        $dlib = Join-Path $cache 'bin/x64/Azure.CodeSigning.Dlib.dll'
        if (-not (Test-Path -LiteralPath $dlib)) {
            $zip = Join-Path ([IO.Path]::GetTempPath()) "emulsion-signing-$PID.zip"
            $staging = "$cache.$PID.partial"
            try {
                Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/$package/$version/$package.$version.nupkg" -OutFile $zip -UseBasicParsing
                Expand-Archive -LiteralPath $zip -DestinationPath $staging -Force
                if (-not (Test-Path "$staging/bin/x64/Azure.CodeSigning.Dlib.dll")) { throw 'Signing client has no x64 dlib.' }
                if (Test-Path -LiteralPath $cache) { Remove-Item -LiteralPath $cache -Recurse -Force }
                Move-Item -LiteralPath $staging -Destination $cache
            } finally {
                Remove-Item -LiteralPath $zip, $staging -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
    }
    if (-not (Test-Path -LiteralPath $dlib)) { throw 'Signing dlib does not exist.' }
    $metadataPath = Join-Path $root 'target/trusted-signing-metadata.json'
    New-Item -ItemType Directory -Path (Split-Path -Parent $metadataPath) -Force | Out-Null
    $metadata = [ordered]@{
        Endpoint = $env:AZURE_CODESIGNING_ENDPOINT
        CodeSigningAccountName = $env:AZURE_CODESIGNING_ACCOUNT
        CertificateProfileName = $env:AZURE_CODESIGNING_PROFILE
    }
    if ($env:EMULSION_SIGN_AZURE_CLI_ONLY -eq '1') {
        $metadata.ExcludeCredentials = @(
            'EnvironmentCredential', 'WorkloadIdentityCredential', 'ManagedIdentityCredential',
            'SharedTokenCacheCredential', 'VisualStudioCredential', 'VisualStudioCodeCredential',
            'AzurePowerShellCredential', 'AzureDeveloperCliCredential', 'InteractiveBrowserCredential'
        )
    }
    $metadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding ASCII
    return @{ SignTool = $signTool; Dlib = $dlib; Metadata = $metadataPath }
}

function Invoke-TrustedSign {
    param([Parameter(Mandatory)][hashtable]$Context, [Parameter(Mandatory)][string[]]$Path)
    $files = @($Path | ForEach-Object { (Resolve-Path -LiteralPath $_ -ErrorAction Stop).Path })
    $arguments = @('sign', '/v', '/fd', 'SHA256', '/td', 'SHA256',
        '/tr', 'http://timestamp.acs.microsoft.com', '/dlib', $Context.Dlib,
        '/dmdf', $Context.Metadata) + $files
    # Windows PowerShell treats native stderr as an error record; retain the
    # native exit code so signing failures cannot be hidden by output handling.
    $previous = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $Context.SignTool @arguments 2>&1 | ForEach-Object { Write-Host $_ }
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
    if ($code -ne 0) { throw "Azure signing failed with exit code $code." }
}
