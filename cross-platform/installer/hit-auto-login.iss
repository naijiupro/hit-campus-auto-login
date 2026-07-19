#define MyAppName "HIT 校园网自动登录"
#define MyAppVersion "1.0.2"
#define MyAppExeName "hit-auto-login.exe"

[Setup]
AppId={{B39218DB-382E-45A3-BC1E-B10BA476423D}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
DefaultDirName={localappdata}\Programs\HITAutoLogin
DefaultGroupName={#MyAppName}
PrivilegesRequired=lowest
OutputDir=..\dist\windows
OutputBaseFilename=HITAutoLogin-Setup-x64
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{cmd}"; Parameters: "/c reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v HITAutoLogin /f"; Flags: runhidden; RunOnceId: "RemoveAutoStart"
