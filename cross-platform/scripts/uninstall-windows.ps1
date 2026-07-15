$ErrorActionPreference = 'Stop'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\HITAutoLogin'
$Exe = Join-Path $InstallDir 'hit-auto-login.exe'
Get-Process hit-auto-login -ErrorAction SilentlyContinue | Stop-Process -Force
reg.exe DELETE 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v HITAutoLogin /f 2>$null | Out-Null
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'HIT 校园网自动登录.lnk'
if (Test-Path -LiteralPath $Shortcut) { Remove-Item -LiteralPath $Shortcut -Force }
if (Test-Path -LiteralPath $InstallDir) { Remove-Item -LiteralPath $InstallDir -Recurse -Force }
Write-Host '程序和登录启动项已移除。配置仍保留在 %APPDATA%\HITAutoLogin。'

