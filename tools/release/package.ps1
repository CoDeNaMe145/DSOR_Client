# Builds the Windows release: the client, and the converter that turns the
# game data the client downloads into its assets. No game data goes in it.
# docs/release.md describes the layout and what each part is for.
#
#   powershell -File tools/release/package.ps1 [-Version v0.1.0] [-NoDlss] [-SkipBuild]
#       [-DsoGodot <checkout>] [-HostPython <python with pip>] [-Out dist]
#
# Leaves dist/DSOR-<version>-windows-x64/ and its .zip. Needs the Rust
# toolchain and Visual Studio's C++ tools (as tools/Build-Client.ps1), a Python
# with pip to fetch Pillow and numpy for the bundled Python, and the internet
# (Python, vgmstream, DSO_Godot, the Crunch headers).
param(
    [string]$Version,
    [string]$Out = 'dist',
    [switch]$NoDlss,
    [switch]$SkipBuild,
    [string]$DsoGodot,
    [string]$HostPython = 'python',
    [string]$PythonVersion = '3.12.10',
    [string]$VgmstreamTag = 'r2117'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$release = $PSScriptRoot
$godotRev = (Get-Content -LiteralPath (Join-Path $release 'dso_godot.rev') -Raw).Trim()

function Run([string]$what, [scriptblock]$command) {
    Write-Host "[package] $what"
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$what failed with code $LASTEXITCODE." }
}
function Write-Text([string]$path, [string]$text) {
    # Without a byte order mark: the client and the converter read VERSION as is.
    [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding $false))
}
function Fetch([string]$url, [string]$path) {
    Write-Host "[package] download $url"
    Invoke-WebRequest -Uri $url -OutFile $path -UseBasicParsing
}

Push-Location $workspace
try {
    if (-not $Version) {
        $Version = (git describe --tags --always --dirty 2>$null)
        if (-not $Version) { $Version = 'dev' }
    }
    # The toolchain as tools/Build-Client.ps1 finds it.
    if (Test-Path (Join-Path $workspace '.local/cargo/bin/cargo.exe')) {
        $env:CARGO_HOME = Join-Path $workspace '.local/cargo'
        $env:RUSTUP_HOME = Join-Path $workspace '.local/rustup'
        $env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
        $installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        & (Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
        Set-Location $workspace
    }
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $workspace 'target' }
    if (-not $SkipBuild) {
        $features = if ($NoDlss) { @('--no-default-features') } else { @() }
        Run 'build the client' { cargo build --locked --release -p dsor-client @features }
        Run 'build the level tool' { cargo build --locked --release -p dsor-level-data --bin dsor-levels }
        Run 'build the texture packer' { cargo build --locked --release -p dsor-texture-pack --bin dsor-ktx2 }
    }

    $name = "DSOR-$Version-windows-x64"
    New-Item -ItemType Directory -Force $Out | Out-Null
    $Out = (Resolve-Path $Out).Path
    $stage = Join-Path $Out $name
    $temp = Join-Path $Out "$name.tmp"
    foreach ($folder in @($stage, $temp)) {
        if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
    }
    $converter = Join-Path $stage 'converter'
    $bin = Join-Path $converter 'bin'
    $licenses = Join-Path $stage 'licenses'
    foreach ($folder in @($stage, $temp, $converter, $bin, $licenses, (Join-Path $converter 'tools'))) {
        New-Item -ItemType Directory -Force $folder | Out-Null
    }

    # The client, its notes, and DLSS's runtime when it was built in.
    Copy-Item (Join-Path $target 'release/dsor-client.exe') $stage
    Copy-Item (Join-Path $release 'README.txt') $stage
    Copy-Item (Join-Path $workspace 'LICENSE*') $licenses -ErrorAction SilentlyContinue
    if (-not $NoDlss) {
        $sdk = if ($env:DLSS_SDK) { $env:DLSS_SDK } else { Join-Path $workspace '.local/dlss/sdk' }
        Copy-Item (Join-Path $sdk 'lib/Windows_x86_64/rel/nvngx_dlss.dll') $stage
        Copy-Item (Join-Path $sdk 'LICENSE.txt') (Join-Path $licenses 'NVIDIA-DLSS-SDK-LICENSE.txt')
    }

    # The converter: tools/convert_assets.py and its tools, the authored edits it applies.
    Write-Text (Join-Path $converter 'VERSION') "$Version`n"
    Get-ChildItem (Join-Path $workspace 'tools') -Filter '*.py' -File | Where-Object { $_.Name -notlike 'test_*' } |
        ForEach-Object { Copy-Item $_.FullName (Join-Path $converter 'tools') }
    foreach ($folder in @('level-overrides', 'vfx-overrides')) {
        Copy-Item -Recurse (Join-Path $workspace $folder) (Join-Path $converter $folder)
    }
    Copy-Item (Join-Path $target 'release/dsor-levels.exe') $bin
    Copy-Item (Join-Path $target 'release/dsor-ktx2.exe') $bin

    # DSO_Godot at the pinned commit, with the Windows fixes it lacks.
    $godot = Join-Path $converter 'DSO_Godot'
    if ($DsoGodot) {
        Write-Host "[package] DSO_Godot from $DsoGodot"
        New-Item -ItemType Directory -Force $godot | Out-Null
        Get-ChildItem -LiteralPath $DsoGodot -Force | Where-Object { $_.Name -notin @('.git', 'build', '__pycache__') } |
            ForEach-Object { Copy-Item -Recurse $_.FullName $godot }
    } else {
        Fetch "https://github.com/DSO-Reconstruction/DSO_Godot/archive/$godotRev.zip" (Join-Path $temp 'godot.zip')
        Expand-Archive (Join-Path $temp 'godot.zip') $temp
        Move-Item (Join-Path $temp "DSO_Godot-$godotRev") $godot
    }
    Run 'patch DSO_Godot' { & $HostPython (Join-Path $release 'patch_dso_godot.py') $godot }
    Get-ChildItem -Recurse -Directory -Force $godot -Filter '__pycache__' | Remove-Item -Recurse -Force
    Copy-Item (Join-Path $godot 'LICENSE') (Join-Path $licenses 'DSO_Godot-LICENSE.txt')

    # The Crunch transcoder (crn_decomp.h: public domain), static runtime.
    foreach ($header in @('crn_decomp.h', 'crnlib.h')) {
        Fetch "https://raw.githubusercontent.com/BinomialLLC/crunch/master/inc/$header" (Join-Path $temp $header)
    }
    Push-Location $temp
    Run 'build crn2dds' { cl /nologo /O2 /EHsc /W0 /MT /DNDEBUG /DWIN32 /I . (Join-Path $godot 'tools/crn2dds.cpp') /Fe:crn2dds.exe }
    Pop-Location
    Copy-Item (Join-Path $temp 'crn2dds.exe') $bin

    # vgmstream-cli and its libraries, for the audio.
    Fetch "https://github.com/vgmstream/vgmstream/releases/download/$VgmstreamTag/vgmstream-win64.zip" (Join-Path $temp 'vgmstream.zip')
    Expand-Archive (Join-Path $temp 'vgmstream.zip') (Join-Path $bin 'vgmstream')
    Copy-Item (Join-Path $bin 'vgmstream/COPYING') (Join-Path $licenses 'vgmstream-COPYING.txt') -ErrorAction SilentlyContinue

    # Python (the embeddable distribution) with Pillow and numpy. Its ._pth
    # lists the whole path: site-packages added, `import site` for .pth files.
    $python = Join-Path $converter 'python'
    Fetch "https://www.python.org/ftp/python/$PythonVersion/python-$PythonVersion-embed-amd64.zip" (Join-Path $temp 'python.zip')
    Expand-Archive (Join-Path $temp 'python.zip') $python
    $short = ($PythonVersion -split '\.')[0..1] -join ''
    Write-Text (Join-Path $python "python$short._pth") "python$short.zip`r`n.`r`nLib\site-packages`r`nimport site`r`n"
    $abi = "cp$short"
    Run 'install Pillow and numpy' {
        & $HostPython -m pip install --disable-pip-version-check --no-warn-script-location --target (Join-Path $python 'Lib/site-packages') `
            --only-binary=:all: --platform win_amd64 --python-version ($PythonVersion -replace '^(\d+\.\d+).*', '$1') --implementation cp --abi $abi `
            Pillow numpy
    }
    Run 'check the bundled Python' { & (Join-Path $python 'python.exe') -c 'import PIL, numpy, sqlite3, multiprocessing, concurrent.futures; print(\"python ok\", PIL.__version__, numpy.__version__)' }

    Remove-Item -Recurse -Force $temp
    $zip = Join-Path $Out "$name.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Write-Host "[package] zip $zip"
    Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
    $size = (Get-Item $zip).Length / 1MB
    Write-Host ("[package] done: {0} ({1:N1} MB)" -f $zip, $size)
} finally {
    Pop-Location
}
