[CmdletBinding()]
param(
    [string]$SourceExe
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
if (-not $SourceExe) {
    $SourceExe = Join-Path $Root 'dist\windows\hit-auto-login.exe'
}
if (-not (Test-Path -LiteralPath $SourceExe)) {
    & (Join-Path $PSScriptRoot 'build-windows.ps1')
}

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\HITAutoLogin'
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$InstalledExe = Join-Path $InstallDir 'hit-auto-login.exe'
Copy-Item -LiteralPath $SourceExe -Destination $InstalledExe -Force

$Programs = [Environment]::GetFolderPath('Programs')
$ShortcutPath = Join-Path $Programs 'HIT 校园网自动登录.lnk'
$Shell = New-Object -ComObject WScript.Shell
$Shortcut = $Shell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = $InstalledExe
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description = 'HIT 校园网自动登录'
$Shortcut.Save()

Start-Process -FilePath $InstalledExe
Write-Host "安装完成：$InstalledExe"

