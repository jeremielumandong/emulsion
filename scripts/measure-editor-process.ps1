# Sample one existing editor process; does not send input or change application state.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [ValidateRange(1, 600)]
    [int]$Seconds = 30,
    [string]$OutputDirectory = 'target/performance'
)

$ErrorActionPreference = 'Stop'
$logicalProcessors = [Environment]::ProcessorCount
$sampledProcess = Get-Process -Id $ProcessId
$processName = $sampledProcess.ProcessName
$processStart = $sampledProcess.StartTime.ToUniversalTime().ToString('o')
$initialCpu = $sampledProcess.TotalProcessorTime.TotalSeconds
$initialPrivate = $sampledProcess.PrivateMemorySize64 / 1MB
$initialWorkingSet = $sampledProcess.WorkingSet64 / 1MB
$previousCpu = $initialCpu
$previousSeconds = 0.0
$samples = [System.Collections.Generic.List[object]]::new()
$startedAt = [DateTime]::UtcNow.ToString('o')
$safeLabel = $Label -replace '[^a-zA-Z0-9_.-]', '_'
$runName = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + $safeLabel
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$csvPath = Join-Path $OutputDirectory ($runName + '.csv')
$jsonPath = Join-Path $OutputDirectory ($runName + '.json')
$timer = [System.Diagnostics.Stopwatch]::StartNew()
Write-Output "MEASUREMENT_STARTED process=$ProcessId label=$Label seconds=$Seconds logical_processors=$logicalProcessors"
try {
    while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
        Start-Sleep -Milliseconds 1000
        $sampledProcess.Refresh()
        if ($sampledProcess.HasExited) {
            throw "Process $ProcessId exited during measurement. Partial samples are in $csvPath"
        }
        $cpu = $sampledProcess.TotalProcessorTime.TotalSeconds
        $elapsed = $timer.Elapsed.TotalSeconds
        $interval = $elapsed - $previousSeconds
        $cpuDelta = $cpu - $previousCpu
        $samples.Add([pscustomobject]@{
            ElapsedSeconds = $elapsed
            IntervalSeconds = $interval
            CpuSeconds = $cpuDelta
            CpuPercentMachine = 100.0 * $cpuDelta / $interval / $logicalProcessors
            PrivateMiB = $sampledProcess.PrivateMemorySize64 / 1MB
            WorkingSetMiB = $sampledProcess.WorkingSet64 / 1MB
        })
        $previousCpu = $cpu
        $previousSeconds = $elapsed
    }
} finally {
    $timer.Stop()
    $samples | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8
}

$summary = [ordered]@{
    Label = $Label
    ProcessId = $ProcessId
    ProcessName = $processName
    ProcessStartedUtc = $processStart
    MeasurementStartedUtc = $startedAt
    LogicalProcessors = $logicalProcessors
    RequestedSeconds = $Seconds
    MeasuredSeconds = $previousSeconds
    CpuSeconds = $previousCpu - $initialCpu
    MeanCpuPercentMachine = 100.0 * ($previousCpu - $initialCpu) / $previousSeconds / $logicalProcessors
    PeakSampleCpuPercentMachine = ($samples | Measure-Object CpuPercentMachine -Maximum).Maximum
    InitialPrivateMiB = $initialPrivate
    FinalPrivateMiB = $samples[$samples.Count - 1].PrivateMiB
    PeakPrivateMiB = [Math]::Max($initialPrivate, ($samples | Measure-Object PrivateMiB -Maximum).Maximum)
    InitialWorkingSetMiB = $initialWorkingSet
    FinalWorkingSetMiB = $samples[$samples.Count - 1].WorkingSetMiB
    PeakWorkingSetMiB = [Math]::Max($initialWorkingSet, ($samples | Measure-Object WorkingSetMiB -Maximum).Maximum)
    SamplesCsv = [System.IO.Path]::GetFullPath($csvPath)
    Note = 'Process CPU normalized across logical processors; includes all app threads. Memory excludes GPU allocations. Does not measure frame time or input latency.'
}
$summary | ConvertTo-Json | Set-Content -LiteralPath $jsonPath -Encoding UTF8
Write-Output 'MEASUREMENT_FINISHED'
$summary | ConvertTo-Json
