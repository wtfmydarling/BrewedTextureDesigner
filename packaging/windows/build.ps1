# Build Brewed Texture Designer for Windows: an installer, and a zip of the program alone.
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
#     → target\dist\Brewed-Texture-Designer-<version>-windows-x64-setup.exe
#     → target\dist\Brewed-Texture-Designer-<version>-windows-x64.zip
#
# Needs Rust (https://rustup.rs, with the MSVC toolchain) and, for the installer, Inno Setup 6
# (winget install JRSoftware.InnoSetup); without it, only the zip is made.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value

cargo build --release --locked -p brewed-texture-designer
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dist = Join-Path $root "target\dist"
New-Item -ItemType Directory -Force $dist | Out-Null

# The zip: the program, what it is, its licenses.
$name = "Brewed-Texture-Designer-$version-windows-x64"
$stage = Join-Path $root "target\windows\$name"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\brewed-texture-designer.exe, README.md, LICENSE-MIT, LICENSE-APACHE $stage
Compress-Archive -Force -Path $stage -DestinationPath (Join-Path $dist "$name.zip")
Write-Host "Built $dist\$name.zip"

# The installer, where Inno Setup is.
$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Host "Inno Setup 6 is not installed (winget install JRSoftware.InnoSetup): no installer made."
    exit 0
}
& $iscc "/DVersion=$version" "/DSource=$root" "/DOutput=$dist" (Join-Path $PSScriptRoot "installer.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
Write-Host "Built $dist\$name-setup.exe"
