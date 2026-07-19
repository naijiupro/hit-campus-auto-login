[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ExePath
)

$ErrorActionPreference = 'Stop'
$ResolvedExe = (Resolve-Path -LiteralPath $ExePath).Path

$MtCommand = Get-Command mt.exe -ErrorAction SilentlyContinue
if ($MtCommand) {
    $Mt = $MtCommand.Source
} else {
    $KitsBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $Mt = Get-ChildItem -LiteralPath $KitsBin -Directory -ErrorAction SilentlyContinue |
        Where-Object Name -Match '^10\.' |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\mt.exe' } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
}
if (-not $Mt) {
    throw '未找到 Windows SDK mt.exe，无法验证 EXE manifest。'
}

$ManifestPath = Join-Path $env:TEMP ("hit-auto-login-{0}.manifest" -f [guid]::NewGuid())
try {
    & $Mt "-inputresource:$ResolvedExe;#1" "-out:$ManifestPath"
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $ManifestPath)) {
        throw 'EXE 没有可读取的嵌入式 manifest。'
    }

    $Manifest = Get-Content -Raw -LiteralPath $ManifestPath
    $CommonControls = [regex]::Match(
        $Manifest,
        '<assemblyIdentity(?=[^>]*name=["'']Microsoft\.Windows\.Common-Controls["''])(?=[^>]*version=["'']6\.0\.0\.0["''])[^>]*/?>',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if (-not $CommonControls.Success) {
        throw 'EXE manifest 未声明 Microsoft.Windows.Common-Controls 6.0.0.0。'
    }

    $DpiAwareness = [regex]::Match(
        $Manifest,
        '<dpiAwareness[^>]*>\s*PerMonitorV2\s*</dpiAwareness>',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if (-not $DpiAwareness.Success) {
        throw 'EXE manifest 未声明 PerMonitorV2 DPI 感知。'
    }

    Write-Host "Windows manifest 与 DPI 感知验证通过：$ResolvedExe"
} finally {
    if (Test-Path -LiteralPath $ManifestPath) {
        Remove-Item -LiteralPath $ManifestPath -Force
    }
}
