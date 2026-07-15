[CmdletBinding()]
param(
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$LocalCargo = Join-Path $Root '.tools\cargo\bin\cargo.exe'

if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $Cargo = (Get-Command cargo).Source
} elseif (Test-Path -LiteralPath $LocalCargo) {
    $Cargo = $LocalCargo
    $env:CARGO_HOME = Join-Path $Root '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $Root '.tools\rustup'
} else {
    throw '未找到 Cargo。请先安装 Rust：https://rustup.rs/'
}

& $Cargo fmt --all -- --check
if (-not $SkipTests) {
    & $Cargo test --workspace
}
& $Cargo build --release -p hit-auto-login

$Dist = Join-Path $Root 'dist\windows'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Exe = Join-Path $Root 'target\release\hit-auto-login.exe'
Copy-Item -LiteralPath $Exe -Destination (Join-Path $Dist 'hit-auto-login.exe') -Force

$Zip = Join-Path $Dist 'HITAutoLogin-Windows-x64.zip'
if (Test-Path -LiteralPath $Zip) { Remove-Item -LiteralPath $Zip -Force }
Compress-Archive -LiteralPath (Join-Path $Dist 'hit-auto-login.exe') -DestinationPath $Zip

$Iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if ($Iscc) {
    & $Iscc.Source (Join-Path $Root 'installer\hit-auto-login.iss')
    Write-Host "Inno Setup 安装包已生成到 $Dist"
} else {
    Write-Host '未找到 Inno Setup；已生成可直接运行的 EXE 和 ZIP。'
}
Write-Host $Dist

