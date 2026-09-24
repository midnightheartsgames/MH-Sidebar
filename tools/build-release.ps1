$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read Cargo package metadata' }
    $project = @($metadata.packages | Where-Object name -eq 'mh-sidebar')
    if ($project.Count -ne 1) { throw 'Cannot identify the MH Sidebar Cargo package' }
    $version = $project[0].version
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }

    $destination = Join-Path $projectRoot "dist/v$version"
    if (Test-Path -LiteralPath $destination) {
        throw "Release directory already exists: $destination"
    }
    New-Item -ItemType Directory -Path $destination | Out-Null
    $files = @{
        'MH-Sidebar.exe' = 'target/release/MH-Sidebar.exe'
        'README.md' = 'README.md'
        'VALIDATION.md' = 'docs/VALIDATION.md'
        'COMPATIBILITY.md' = 'docs/COMPATIBILITY.md'
        'LICENSE' = 'LICENSE'
        'THIRD-PARTY.md' = 'THIRD-PARTY.md'
        'Cuprum-OFL.txt' = 'assets/fonts/OFL.txt'
        'PawnIO-Modules-LICENSE.txt' = 'assets/pawnio/COPYING'
        'PawnIO-Modules-NOTICE.md' = 'assets/pawnio/NOTICE.md'
    }
    foreach ($name in @($files.Keys | Sort-Object)) {
        Copy-Item -LiteralPath $files[$name] -Destination (Join-Path $destination $name)
    }
    $hashes = foreach ($name in @($files.Keys | Sort-Object)) {
        $hash = (Get-FileHash -LiteralPath (Join-Path $destination $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name"
    }
    [System.IO.File]::WriteAllLines((Join-Path $destination 'SHA256SUMS.txt'), [string[]]$hashes)
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -PackageDirectory $destination -ExpectedVersion $version
    if ($LASTEXITCODE -ne 0) { throw 'Release verification failed' }

    $archive = Join-Path $projectRoot "dist/MH-Sidebar-v$version-windows-x64.zip"
    if (Test-Path -LiteralPath $archive) { throw "Release archive already exists: $archive" }
    Compress-Archive -Path (Join-Path $destination '*') -DestinationPath $archive
    $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$archive.sha256", "$archiveHash  $(Split-Path -Leaf $archive)`n")
    Write-Output "Portable archive: $archive"
    Write-Output "SHA256: $archiveHash"
} finally {
    Pop-Location
}
