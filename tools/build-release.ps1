$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $destination = Join-Path $projectRoot 'dist'
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    Copy-Item -LiteralPath 'target/release/MH-Sidebar.exe' -Destination $destination
    Copy-Item -LiteralPath 'README.md', 'LICENSE', 'THIRD-PARTY.md' -Destination $destination
    Copy-Item -LiteralPath 'assets/fonts/OFL.txt' -Destination (Join-Path $destination 'Cuprum-OFL.txt')
    $exe = Join-Path $destination 'MH-Sidebar.exe'
    $hash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath (Join-Path $destination 'SHA256SUMS.txt') -Value "$hash  MH-Sidebar.exe" -Encoding ascii
    Write-Output "Portable build: $exe"
    Write-Output "SHA256: $hash"
} finally { Pop-Location }
