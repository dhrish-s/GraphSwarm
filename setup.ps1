# Bootstraps a Rust toolchain (if needed) and builds GraphSwarm on Windows.
# Safe to re-run: every step checks whether it already succeeded before acting.
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $RepoRoot

function Test-CommandExists($name) {
    return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

function Add-CargoBinToPath {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) {
        $env:PATH = "$cargoBin;$env:PATH"
    }
}

Write-Host "==> Checking for Rust toolchain..."
if (-not (Test-CommandExists "cargo") -or -not (Test-CommandExists "rustc")) {
    Write-Host "cargo/rustc not found. Installing Rust via rustup (non-interactive)..."
    $installer = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $installer
    & $installer -y --default-toolchain stable
    Remove-Item $installer -ErrorAction SilentlyContinue
    Add-CargoBinToPath
} else {
    Write-Host "Found cargo: $((Get-Command cargo).Source)"
}

# Make sure this session can see a freshly installed toolchain even if PATH
# hasn't been reloaded yet.
Add-CargoBinToPath

if (-not (Test-CommandExists "rustup")) {
    Write-Error "rustup still not found on PATH after install. Open a new terminal and re-run this script."
    exit 1
}

Write-Host "==> Active toolchain:"
rustup show

Write-Host "==> Building graphswarm (release)..."
cargo build --release

$BinPath = Join-Path $RepoRoot "target\release\graphswarm.exe"
if (-not (Test-Path $BinPath)) {
    Write-Error "Build did not produce $BinPath"
    exit 1
}

Write-Host "==> Binary built at: $BinPath"
Write-Host "==> Verifying binary runs:"
& $BinPath --version

Write-Host "==> Setup complete."
