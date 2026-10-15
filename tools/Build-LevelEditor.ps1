$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
if (Test-Path (Join-Path $workspace '.local/cargo/bin/cargo.exe')) {
    $env:CARGO_HOME = Join-Path $workspace '.local/cargo'
    $env:RUSTUP_HOME = Join-Path $workspace '.local/rustup'
    $env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
}
$installer = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer'
$vswhere = Join-Path $installer 'vswhere.exe'
if (Test-Path $vswhere) {
    $installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($installation) {
        & (Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    }
}
Push-Location $workspace
try {
    $ErrorActionPreference = 'Continue'
    cargo rustc --locked --release -p dsor-client --bin dsor-client -- -C opt-level=1 -C lto=thin -C codegen-units=256
    if ($LASTEXITCODE -ne 0) { throw "Editor renderer build failed with code $LASTEXITCODE." }
} finally {
    Pop-Location
}
