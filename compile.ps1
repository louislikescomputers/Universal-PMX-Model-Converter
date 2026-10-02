# compile.ps1 - Build mmdconv for Windows (PowerShell)
#
# Usage:
#   .\compile.ps1                 # release build (default)
#   .\compile.ps1 -BuildDebug     # debug build
#   .\compile.ps1 -Tests          # run the test suite first; abort on failure
#   .\compile.ps1 -Clippy         # run cargo clippy -D warnings first
#   .\compile.ps1 -Triple aarch64-pc-windows-msvc   # cross-compile target
#
# Output binary is copied to .\dist\mmdconv.exe
#
# NOTE: we deliberately do NOT declare a [switch]$Debug here. PowerShell
# scripts get the common parameter -Debug automatically (from [CmdletBinding()]),
# so declaring our own would fail at load time with:
#   "A parameter with the name 'Debug' was defined multiple times for the command."
# Use -BuildDebug for a debug build instead.

[CmdletBinding()]
param(
    [switch]$BuildDebug,
    [switch]$Tests,
    [switch]$Clippy,
    [string]$Triple = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Always operate from the repository root (this script's directory).
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $Root
try {
    # --- Preflight -----------------------------------------------------------
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: 'cargo' was not found on PATH." -ForegroundColor Red
        Write-Host "Install Rust from https://rustup.rs and reopen the terminal."
        exit 1
    }

    $Profile = if ($BuildDebug) { "debug" } else { "release" }
    $ProfileFlag = if ($BuildDebug) { @() } else { @("--release") }
    $TargetArgs = @()
    if ($Triple -ne "") {
        $TargetArgs = @("--target", $Triple)
    }

    Write-Host "== mmdconv build (Windows) ==" -ForegroundColor Cyan
    Write-Host "   profile : $Profile"
    Write-Host "   toolchain: $(cargo --version)"

    # --- Optional quality gates ---------------------------------------------
    if ($Clippy) {
        Write-Host "`n-- clippy (errors treated as warnings-fail) --" -ForegroundColor Yellow
        cargo clippy --workspace --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw "clippy failed" }
    }

    if ($Tests) {
        Write-Host "`n-- test suite --" -ForegroundColor Yellow
        $TestArgs = @("test", "--workspace") + $ProfileFlag
        & cargo @TestArgs
        if ($LASTEXITCODE -ne 0) { throw "tests failed" }
    }

    # --- Build ---------------------------------------------------------------
    # NOTE: arguments are assembled into a single array and splatted once.
    # Mixing `@Array` splatting with bare literal arguments in the same native
    # command call can hand stray '-' tokens to cargo on some PowerShell
    # versions ("error: unexpected argument '-' found"), so we avoid it here.
    Write-Host "`n-- building mmdconv (release binary: mmdconv) --" -ForegroundColor Yellow
    $BuildArgs = @("build") + $ProfileFlag + @("--bin", "mmdconv") + $TargetArgs
    & cargo @BuildArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }

    # --- Collect artifact ------------------------------------------------------
    # Ask cargo exactly where it put the binary. This is robust against
    # CARGO_TARGET_DIR overrides, custom profiles and target subdirectories.
    $QueryArgs = @("build") + $ProfileFlag + @("--bin", "mmdconv") + $TargetArgs + @("--message-format", "json-render-diagnostics")
    $ArtifactPath = $null
    foreach ($line in (& cargo @QueryArgs)) {
        try {
            $obj = $line | ConvertFrom-Json
        } catch { continue }
        if ($obj.reason -eq "compiler-artifact" -and $obj.executable) {
            if ($obj.executable -match "mmdconv(\.exe)?$") { $ArtifactPath = $obj.executable }
        }
    }
    if (-not $ArtifactPath) {
        # Fallback to the conventional location.
        $ArtifactPath = Join-Path $Root "target"
        if ($Triple -ne "") { $ArtifactPath = Join-Path $ArtifactPath $Triple }
        $ArtifactPath = Join-Path (Join-Path $ArtifactPath $Profile) "mmdconv.exe"
    }
    $SrcBin = (Resolve-Path -ErrorAction SilentlyContinue $ArtifactPath)
    if (-not $SrcBin) {
        throw "expected artifact not found: $ArtifactPath (did the build produce a 'mmdconv' binary?)"
    }
    $SrcBin = $SrcBin.Path
    # Guard against the old silent no-op stub: a real optimized binary is MBs.
    if ((Get-Item $SrcBin).Length -lt 100kb) {
        throw "artifact is suspiciously small ($((Get-Item $SrcBin).Length) bytes) - looks like a stub, not a real build."
    }

    $Dist = Join-Path $Root "dist"
    New-Item -ItemType Directory -Force -Path $Dist | Out-Null
    $DstBin = Join-Path $Dist "mmdconv.exe"
    Copy-Item -Force $SrcBin $DstBin

    $hash = (Get-FileHash -Algorithm SHA256 $DstBin).Hash.ToLower()
    Set-Content -Path "$DstBin.sha256" -Value "$hash  mmdconv.exe" -Encoding ascii

    Write-Host "`nBUILD OK" -ForegroundColor Green
    Write-Host "  binary : $DstBin"
    Write-Host "  sha256 : $hash"
    Write-Host "  size   : $([math]::Round((Get-Item $DstBin).Length / 1MB, 2)) MB"
    exit 0
}
catch {
    Write-Host "`nBUILD FAILED: $_" -ForegroundColor Red
    exit 1
}
finally {
    Pop-Location
}
