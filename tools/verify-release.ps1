param(
    [Parameter(Mandatory = $true)][string]$PackageDirectory,
    [Parameter(Mandatory = $true)][string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
$package = (Resolve-Path -LiteralPath $PackageDirectory).Path
$required = @(
    'COMPATIBILITY.md',
    'Cuprum-OFL.txt',
    'LICENSE',
    'MH-Sidebar.exe',
    'PawnIO-Modules-LICENSE.txt',
    'PawnIO-Modules-NOTICE.md',
    'README.md',
    'THIRD-PARTY.md',
    'VALIDATION.md'
)
$actual = @(Get-ChildItem -LiteralPath $package -File -Force | ForEach-Object Name)
$expected = @($required + 'SHA256SUMS.txt')
$difference = @(Compare-Object -ReferenceObject $expected -DifferenceObject $actual)
if ($difference.Count -ne 0) {
    throw "Release contents differ from the required file list: $($difference | Out-String)"
}

$manifestPath = Join-Path $package 'SHA256SUMS.txt'
$entries = @{}
foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if ($line -cnotmatch '^([0-9a-f]{64})  ([A-Za-z0-9.-]+)$') {
        throw "Invalid SHA256SUMS entry: $line"
    }
    $name = $Matches[2]
    if ($entries.ContainsKey($name)) {
        throw "Duplicate SHA256SUMS entry: $name"
    }
    $entries[$name] = $Matches[1]
}
if ($entries.Count -ne $required.Count) {
    throw "SHA256SUMS has $($entries.Count) entries; expected $($required.Count)"
}
foreach ($name in $required) {
    if (-not $entries.ContainsKey($name)) {
        throw "SHA256SUMS is missing $name"
    }
    $path = Join-Path $package $name
    if ((Get-Item -LiteralPath $path).Length -eq 0) {
        throw "Release file is empty: $name"
    }
    $actualHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -cne $entries[$name]) {
        throw "SHA256 mismatch: $name"
    }
}

$exe = Join-Path $package 'MH-Sidebar.exe'
$version = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
if ($version -cne $ExpectedVersion) {
    throw "EXE version $version differs from expected version $ExpectedVersion"
}
$stream = [System.IO.File]::OpenRead($exe)
try {
    $reader = [System.IO.BinaryReader]::new($stream)
    if ($reader.ReadUInt16() -ne 0x5a4d) {
        throw 'Executable is missing its DOS header'
    }
    $stream.Position = 0x3c
    $peOffset = $reader.ReadUInt32()
    if ($peOffset -lt 0x40 -or $peOffset -gt $stream.Length - 6) {
        throw 'Executable has an invalid PE header offset'
    }
    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) {
        throw 'Executable is not a Windows x64 PE file'
    }
} finally {
    $stream.Dispose()
}

Write-Output "Verified MH Sidebar v$ExpectedVersion Windows x64: $package"
