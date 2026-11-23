$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$toolsDir = $PSScriptRoot
$fixtureDir = Join-Path $repoRoot "tests\fixtures\real_css\tailwind_v4"

New-Item -ItemType Directory -Force -Path $fixtureDir | Out-Null

if (-not (Test-Path (Join-Path $toolsDir "node_modules\.bin\tailwindcss.cmd"))) {
    Push-Location $toolsDir
    npm ci --no-audit --no-fund
    Pop-Location
}

$twBin = Join-Path $toolsDir "node_modules\.bin\tailwindcss.cmd"
$normalOut = Join-Path $fixtureDir "tailwind_v4.css"
$optOut = Join-Path $fixtureDir "tailwind_v4.optimize.css"

& $twBin --cwd $toolsDir -i tailwind_input.css -o $normalOut
if ($LASTEXITCODE -ne 0) { throw "tailwindcss normal build failed" }

& $twBin --cwd $toolsDir -i tailwind_input.css -o $optOut --optimize
if ($LASTEXITCODE -ne 0) { throw "tailwindcss --optimize build failed" }

node (Join-Path $toolsDir "extract_css_oracle.mjs")
if ($LASTEXITCODE -ne 0) { throw "extract_css_oracle.mjs failed" }

$normalHash = (Get-FileHash -Path $normalOut -Algorithm SHA256).Hash.ToLowerInvariant()
$optHash = (Get-FileHash -Path $optOut -Algorithm SHA256).Hash.ToLowerInvariant()
$oracleHash = (Get-FileHash -Path (Join-Path $fixtureDir "oracle.json") -Algorithm SHA256).Hash.ToLowerInvariant()

$pkgJson = Get-Content (Join-Path $toolsDir "package.json") -Raw | ConvertFrom-Json
$twCliVersion = $pkgJson.devDependencies.'@tailwindcss/cli'
$twVersion = $pkgJson.devDependencies.tailwindcss
$pwVersion = $pkgJson.devDependencies.playwright

$sourceMd = @"
# Real CSS Fixture Provenance — Tailwind CSS v4

- **Source package**: ``@tailwindcss/cli@$twCliVersion`` + ``tailwindcss@$twVersion`` (npm official registry, MIT License)
- **Scanned template**: ``examples/09_tailwind_v4/views/app.krui`` (via ``tools/tailwind_input.css``)
- **Oracle generator**: ``playwright@$pwVersion`` headless Chromium (``tools/extract_css_oracle.mjs``)
- **Regeneration script**: ``pwsh -NoProfile -File tools/regen_real_css.ps1``
- **Modification policy**: Zero manual edits; exact byte-for-byte CLI output.

## Exact Commands

````powershell
./tools/node_modules/.bin/tailwindcss.cmd --cwd tools -i tailwind_input.css -o tests/fixtures/real_css/tailwind_v4/tailwind_v4.css
./tools/node_modules/.bin/tailwindcss.cmd --cwd tools -i tailwind_input.css -o tests/fixtures/real_css/tailwind_v4/tailwind_v4.optimize.css --optimize
node tools/extract_css_oracle.mjs
````

## SHA256 Digests

| File | SHA256 |
|---|---|
| ``tailwind_v4.css`` | ``$normalHash`` |
| ``tailwind_v4.optimize.css`` | ``$optHash`` |
| ``oracle.json`` | ``$oracleHash`` |
"@

Set-Content -Path (Join-Path $fixtureDir "SOURCE.md") -Value $sourceMd -Encoding utf8NoBOM
Write-Host "Regenerated Tailwind v4 fixtures + Chromium oracle successfully." -ForegroundColor Green
