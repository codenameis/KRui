$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($env:PYTHON3)) {
    $env:PYTHON3 = (& py -c "import sys; print(sys.executable)" | Out-String).Trim()
}

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host " [KRui Harness2 Probe] Starting Empirical Verification Loop" -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan

# Query target directory from cargo metadata so custom CARGO_TARGET_DIR always works
$metadataJson = cargo metadata --format-version 1 --no-deps
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] cargo metadata failed!"
    exit 1
}
$targetDir = ($metadataJson | ConvertFrom-Json).target_directory
Write-Host "  -> Cargo target_directory: $targetDir" -ForegroundColor DarkGray

# Clean stale debug example PDBs/EXEs to avoid transient Windows MSVC LNK1104/LNK1201 locks
if (Test-Path "$targetDir\debug\examples") {
    Remove-Item -Force -ErrorAction SilentlyContinue "$targetDir\debug\examples\*.pdb", "$targetDir\debug\examples\*.exe"
}

if ($env:KRUI_PROBE_INJECT_GATE_FAILURE -eq "cargo_tree") {
    Write-Error "[Probe FAILED] Injected cargo_tree failure gate"
    exit 1
}
if ($env:KRUI_PROBE_INJECT_GATE_FAILURE -eq "example_frames") {
    Write-Error "[Probe FAILED] Injected example_frames failure gate (expected >= 120 frames)"
    exit 1
}

function Assert-PackageExcludedFromTree {
    param(
        [string]$Package,
        [string]$Edges,
        [string]$InvertedTarget
    )
    $oldPref = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $combinedOut = (& cargo tree -p $Package -e $Edges -i $InvertedTarget 2>&1 | Out-String)
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $oldPref

    if ($exitCode -eq 0 -and ([string]::IsNullOrWhiteSpace($combinedOut) -or $combinedOut -match "nothing to print")) {
        return
    }
    if ($exitCode -eq 0) {
        Write-Error "[Probe FAILED] Dependency graph of '$Package' ($Edges) unexpectedly contains '$InvertedTarget':`n$combinedOut"
        exit 1
    }
    if ($exitCode -ne 0 -and $combinedOut -notmatch "package ID specification .* did not match any packages") {
        Write-Error "[Probe FAILED] 'cargo tree -p $Package -e $Edges -i $InvertedTarget' failed with a cargo/manifest error (exit $exitCode):`n$combinedOut"
        exit 1
    }
}

function Assert-PackagePresentInTree {
    param(
        [string]$Package,
        [string]$Features,
        [string]$Edges,
        [string]$InvertedTarget
    )
    $oldPref = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $combinedOut = (& cargo tree -p $Package --features $Features -e $Edges -i $InvertedTarget 2>&1 | Out-String)
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $oldPref

    if ($exitCode -ne 0 -or $combinedOut -notmatch [regex]::Escape($Package)) {
        Write-Error "[Probe FAILED] Expected '$InvertedTarget' in dependency graph of '$Package --features $Features', but cargo tree returned (exit $exitCode):`n$combinedOut"
        exit 1
    }
}

# 1. Formatting, Clippy (default + preview), and Workspace Tests (default + preview)
Write-Host "`n[1/5] Running Formatting, Clippy & Workspace Tests (default + preview)..." -ForegroundColor Yellow
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] cargo fmt --check failed!"
    exit 1
}
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] cargo clippy (default features) failed!"
    exit 1
}
cargo clippy --workspace --all-targets --features preview -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] cargo clippy (--features preview) failed!"
    exit 1
}
cargo test --test anti_hardcode_lint_gate -j 1 --quiet
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] anti_hardcode_lint_gate failed!"
    exit 1
}
cargo test --workspace --lib --bins --tests -j 1 --quiet
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] Workspace tests (default features) failed!"
    exit 1
}
cargo test --workspace --lib --bins --tests --features preview -j 1 --quiet
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] Workspace tests (--features preview) failed!"
    exit 1
}
Write-Host "  -> fmt, clippy (default + preview), and workspace tests (default + preview) PASSED." -ForegroundColor Green

# 1b. Verify Zero Hardcoded Color::rgb( or Color::rgba( Literals in crates/core, crates/renderer, and crates/stylo-bridge/src
$rgbMatches = Get-ChildItem -Path crates/core, crates/renderer, crates/stylo-bridge/src -Recurse -Include *.rs | Select-String -Pattern "Color::rgba?\("
if ($rgbMatches) {
    Write-Error "[Probe FAILED] Found forbidden Color::rgb( or Color::rgba( literals in crates/core, crates/renderer, or crates/stylo-bridge/src:`n$($rgbMatches | Out-String)"
    exit 1
}
Write-Host "  -> Zero hardcoded Color::rgb(/Color::rgba( literals in crates/core, crates/renderer, and crates/stylo-bridge/src PASSED." -ForegroundColor Green

# 1c. Verify Zero Rc<dyn Fn in examples/ and README.md
$rcFnMatches = Get-ChildItem -Path examples -Recurse -Include *.rs | Select-String -Pattern "Rc<dyn Fn"
$readmeRcMatches = Select-String -Path README.md -Pattern "Rc<dyn Fn"
if ($rcFnMatches -or $readmeRcMatches) {
    Write-Error "[Probe FAILED] Found forbidden Rc<dyn Fn in examples/ or README.md:`n$(($rcFnMatches + $readmeRcMatches) | Out-String)"
    exit 1
}
Write-Host "  -> Zero Rc<dyn Fn in examples/ and README.md PASSED." -ForegroundColor Green

# 2. Release & Preview Dependency Graph Check via `cargo tree`
Write-Host "`n[2/5] Verifying Release & Preview Dependency Graphs (cargo tree)..." -ForegroundColor Yellow
Assert-PackageExcludedFromTree -Package "krui" -Edges "normal,no-proc-macro" -InvertedTarget "krui-compiler"
Assert-PackageExcludedFromTree -Package "krui" -Edges "normal,no-proc-macro" -InvertedTarget "krui-preview"
Assert-PackageExcludedFromTree -Package "krui" -Edges "normal,no-proc-macro" -InvertedTarget "lightningcss"
Assert-PackageExcludedFromTree -Package "krui-android-ffi" -Edges "normal,no-proc-macro" -InvertedTarget "krui-compiler"
Assert-PackageExcludedFromTree -Package "krui-android-ffi" -Edges "normal,no-proc-macro" -InvertedTarget "krui-preview"
Assert-PackageExcludedFromTree -Package "krui-android-ffi" -Edges "normal,no-proc-macro" -InvertedTarget "lightningcss"
Assert-PackageExcludedFromTree -Package "krui-core" -Edges "normal" -InvertedTarget "krui-patterns"
Assert-PackagePresentInTree -Package "krui" -Features "preview" -Edges "normal,no-proc-macro" -InvertedTarget "krui-compiler"
Assert-PackagePresentInTree -Package "krui" -Features "preview" -Edges "normal,no-proc-macro" -InvertedTarget "krui-preview"
Write-Host "  -> Dependency graph check PASSED (release excludes krui-compiler/krui-preview/lightningcss; preview includes compiler+preview; krui-core independent of krui-patterns)." -ForegroundColor Green

# 3. Controlled Release Benchmark (`krui-bench`: 30 warmup + 300 measured frames on TinySkia & Wgpu, p50/p99 <= 8,333us)
Write-Host "`n[3/5] Running Controlled Release Benchmark (cargo run --release --bin krui-bench)..." -ForegroundColor Yellow
$benchLines = & cargo run --release --quiet --bin krui-bench
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] Controlled release benchmark (krui-bench) failed budget or relayout invariants!"
    exit 1
}
$benchText = ($benchLines | Out-String)
Write-Host $benchText.TrimEnd()
foreach ($requiredBackend in @("TinySkia", "Wgpu")) {
    if ($benchText -notmatch "\[$requiredBackend\s*\]\s+cold_1st=\s*\d+us\s*\|\s*warm_1st=\s*\d+us\s*\|\s*p50=\s*(\d+)us\s*\|\s*p99=\s*(\d+)us\s*\|\s*max=\s*\d+us\s*\|\s*anim_relayouts=(\d+)") {
        Write-Error "[Probe FAILED] Benchmark output missing valid measurement line for backend '$requiredBackend'!"
        exit 1
    }
    $p50 = [int]$Matches[1]
    $p99 = [int]$Matches[2]
    $relayouts = [int]$Matches[3]
    if ($p50 -gt 8333 -or $p99 -gt 8333 -or $relayouts -ne 0) {
        Write-Error "[Probe FAILED] '$requiredBackend' violated 120 FPS gate: p50=${p50}us, p99=${p99}us (limit 8333us), anim_relayouts=${relayouts}!"
        exit 1
    }
}
Write-Host "  -> Controlled release benchmark (krui-bench) PASSED (TinySkia & Wgpu p50/p99 <= 8,333us, 0 anim relayouts)." -ForegroundColor Green

# 4. Main Showcase Headless & 120Hz Budget Probe
Write-Host "`n[4/5] Probing Main Showcase (120Hz Fast-Path & Frame Budget)..." -ForegroundColor Yellow
cargo run --quiet --bin krui-showcase -- --headless
if ($LASTEXITCODE -ne 0) {
    Write-Error "[Probe FAILED] Main showcase headless verification failed!"
    exit 1
}

# 5. Probe Retained Examples in Headless Multi-Frame Mode (verifying >= 120 frames)
$examples = @(
    "component_gallery"
)

Write-Host "`n[5/5] Probing Component Gallery Example (--headless 120 frames)..." -ForegroundColor Yellow
foreach ($ex in $examples) {
    Write-Host "  -> Running example: $ex"
    $exOut = (& cargo run --quiet --example $ex -- --headless | Out-String)
    if ($LASTEXITCODE -ne 0) {
        Write-Error "[Probe FAILED] Example '$ex' failed in headless mode!"
        exit 1
    }
    Write-Host $exOut.TrimEnd()
    if ($exOut -notmatch "120(-frame|-Frame| frames)") {
        Write-Error "[Probe FAILED] Example '$ex' did not confirm 120-frame headless execution in output:`n$exOut"
        exit 1
    }
}

Write-Host "`n==================================================================" -ForegroundColor Cyan
Write-Host " [KRui Harness2 Probe] ALL GATES PASSED SUCCESSFULLY!" -ForegroundColor Green
Write-Host "==================================================================" -ForegroundColor Cyan
