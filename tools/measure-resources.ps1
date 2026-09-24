param(
    [string]$Executable = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target/release/MH-Sidebar.exe'),
    [int]$DurationSeconds = 30,
    [string]$OutputPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target/compatibility/resources.json')
)

$ErrorActionPreference = 'Stop'
if ($DurationSeconds -lt 10 -or $DurationSeconds -gt 300) {
    throw 'DurationSeconds must be between 10 and 300'
}
$exe = (Resolve-Path -LiteralPath $Executable).Path
$output = [System.IO.Path]::GetFullPath($OutputPath)
$directory = Split-Path -Parent $output
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$config = Join-Path $directory 'resource-settings.json'
$stdout = Join-Path $directory 'resource-stdout.txt'
$stderr = Join-Path $directory 'resource-stderr.txt'
[System.IO.File]::WriteAllText($config, '{"schema_version":4,"width":280,"reserve_space":false,"autostart":false,"visible":true,"notifications_enabled":false}')
$smokeSeconds = $DurationSeconds + 5
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $exe
$startInfo.Arguments = "--config `"$config`" --smoke-test $smokeSeconds"
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.CreateNoWindow = $true
$startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
if (-not $process.Start()) { throw 'Could not start the test instance' }
try {
    Start-Sleep -Seconds 2
    if ($process.HasExited) {
        throw 'Test instance exited before measurement; check whether another MH Sidebar instance owns the single-instance mutex'
    }
    $process.Refresh()
    $initialCpu = $process.TotalProcessorTime.TotalSeconds
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $workingSetPeak = 0L
    $privateBytesPeak = 0L
    $handlePeak = 0
    $threadPeak = 0
    $samples = 0
    for ($second = 0; $second -lt $DurationSeconds; $second++) {
        Start-Sleep -Seconds 1
        $process.Refresh()
        if ($process.HasExited) {
            throw "Test instance exited during measurement after $samples samples"
        }
        $workingSetPeak = [Math]::Max($workingSetPeak, $process.WorkingSet64)
        $privateBytesPeak = [Math]::Max($privateBytesPeak, $process.PrivateMemorySize64)
        $handlePeak = [Math]::Max($handlePeak, $process.HandleCount)
        $threadPeak = [Math]::Max($threadPeak, $process.Threads.Count)
        $samples++
    }
    $timer.Stop()
    $cpuSeconds = $process.TotalProcessorTime.TotalSeconds - $initialCpu
    if (-not $process.WaitForExit(15000)) {
        throw 'Test instance did not exit after its smoke-test deadline'
    }
    $process.Refresh()
    $exitCode = $process.ExitCode
    [System.IO.File]::WriteAllText($stdout, $process.StandardOutput.ReadToEnd())
    [System.IO.File]::WriteAllText($stderr, $process.StandardError.ReadToEnd())
    if ($null -eq $exitCode -or $exitCode -ne 0) {
        throw "Test instance exited with code $exitCode"
    }
    $logicalProcessors = [Environment]::ProcessorCount
    $cpuPercent = 100 * $cpuSeconds / $timer.Elapsed.TotalSeconds / $logicalProcessors
    $report = [ordered]@{
        capturedAtUtc = [DateTime]::UtcNow.ToString('o')
        executable = $exe
        osVersion = [Environment]::OSVersion.Version.ToString()
        logicalProcessors = $logicalProcessors
        durationSeconds = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
        samples = $samples
        cpuSeconds = [Math]::Round($cpuSeconds, 2)
        cpuPercentOfMachine = [Math]::Round($cpuPercent, 2)
        peakWorkingSetMiB = [Math]::Round($workingSetPeak / 1MB, 1)
        peakPrivateMiB = [Math]::Round($privateBytesPeak / 1MB, 1)
        peakHandles = $handlePeak
        peakThreads = $threadPeak
        exitCode = $exitCode
        stderr = [System.IO.File]::ReadAllText($stderr)
    }
    [System.IO.File]::WriteAllText($output, ($report | ConvertTo-Json -Depth 3))
    Write-Output ($report | ConvertTo-Json -Depth 3)
} finally {
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id
    }
    $process.Dispose()
}
