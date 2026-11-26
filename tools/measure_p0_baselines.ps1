$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($env:PYTHON3)) {
    $pyExe = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"
    if (Test-Path $pyExe) {
        $env:PYTHON3 = $pyExe
    } else {
        $env:PYTHON3 = (& py -c "import sys; print(sys.executable)" | Out-String).Trim()
    }
}

Write-Host "[P0 Baselines] Building release component_gallery and krui-bench..." -ForegroundColor Cyan
cargo build --release --example component_gallery
if ($LASTEXITCODE -ne 0) { exit 1 }

$galleryExe = "target/release/examples/component_gallery.exe"
$galleryBytes = if (Test-Path $galleryExe) { (Get-Item $galleryExe).Length } else { 0 }

$benchOut = (& cargo run --release --quiet --bin krui-bench | Out-String)
if ($LASTEXITCODE -ne 0) { exit 1 }

$report = [ordered]@{
    timestamp = (Get-Date).ToString("o")
    component_gallery_bytes = $galleryBytes
    component_gallery_mib = [Math]::Round($galleryBytes / 1MB, 2)
    krui_bench_raw = $benchOut.Trim()
}

New-Item -ItemType Directory -Force -Path "target" | Out-Null
$report | ConvertTo-Json -Depth 4 | Set-Content -Path "target/p0_baseline.json" -Encoding UTF8
Write-Host ($report | ConvertTo-Json -Depth 4)
