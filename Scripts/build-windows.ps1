$ErrorActionPreference = 'Stop'

$msysUcrtBin = 'C:\msys64\ucrt64\bin'
if (-not (Test-Path -LiteralPath (Join-Path $msysUcrtBin 'gcc.exe'))) {
    throw "MSYS2 UCRT64 GCC was not found at $msysUcrtBin. Install MSYS2 UCRT64 or update this path."
}

$env:PATH = "$msysUcrtBin;$env:PATH"
cargo +stable-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Output 'Built target\x86_64-pc-windows-gnu\release\legacylauncher-rust.exe'