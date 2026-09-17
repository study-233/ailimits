; installer/ailimits.iss — Inno Setup installer script for the AI Limits widget.
;
; Build: ISCC.exe installer\ailimits.iss
; Binaries are taken from target\release-min (cargo build --profile release-min).

#define AppName "QuotaBar"
#define AppVersion "0.2.0"
#define AppExe "ailimits.exe"

[Setup]
AppId={{7A1B9C44-5E2D-4F8A-9C3B-AILIMITS0001}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=study-233
AppPublisherURL=https://github.com/study-233/ailimits
LicenseFile=..\LICENSE
DefaultDirName={localappdata}\AiLimits
DefaultGroupName={#AppName}
; Rename the Start menu group during an upgrade instead of reusing the old fork label.
UsePreviousGroup=no
; Per-user install, no admin rights.
PrivilegesRequired=lowest
OutputDir=..\target\installer
OutputBaseFilename=QuotaBar-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
; Close the running widget before updating.
CloseApplications=yes
WizardStyle=modern
DisableProgramGroupPage=yes
ShowLanguageDialog=yes
LanguageDetectionMethod=uilanguage

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "languages\ChineseSimplified.isl"

[CustomMessages]
english.Autostart=Start with Windows
chinesesimplified.Autostart=开机自动启动
english.RunApp=Launch {#AppName}
chinesesimplified.RunApp=启动 {#AppName}

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "..\target\release-min\ailimits.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release-min\ailimits-auth.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.zh-CN.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\TRADEMARKS.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\en\*.md"; DestDir: "{app}\docs\en"; Flags: ignoreversion
Source: "..\docs\zh-CN\*.md"; DestDir: "{app}\docs\zh-CN"; Flags: ignoreversion
Source: "..\docs\images\quotabar-*.png"; DestDir: "{app}\docs\images"; Flags: ignoreversion
Source: "..\docs\images\fluent-*.png"; DestDir: "{app}\docs\images"; Flags: ignoreversion
Source: "languages\ChineseSimplified.LICENSE.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[InstallDelete]
; Remove only shortcuts created by older versions. Preserve configuration and credentials.
Type: files; Name: "{userprograms}\AI Limits (study-233 fork)\AI Limits (study-233 fork).lnk"
Type: dirifempty; Name: "{userprograms}\AI Limits (study-233 fork)"
Type: files; Name: "{userprograms}\AI Limits\AI Limits.lnk"
Type: dirifempty; Name: "{userprograms}\AI Limits"
Type: files; Name: "{autodesktop}\AI Limits (study-233 fork).lnk"
Type: files; Name: "{autodesktop}\AI Limits.lnk"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Autostart via HKCU Run — removed by the uninstaller.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
    ValueName: "AiLimits"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; \
    Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:RunApp}"; \
    Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop the widget before uninstalling.
Filename: "taskkill"; Parameters: "/im {#AppExe} /f"; Flags: runhidden; RunOnceId: "KillWidget"
; Remove the app-created secrets from Credential Manager (API keys, PATs,
; usage tokens) — without this they would outlive the uninstall. A missing
; entry is a no-op (exit 0). Runs before files are deleted.
Filename: "{app}\ailimits-auth.exe"; Parameters: "remove claude"; Flags: runhidden; RunOnceId: "RmKeyClaude"
Filename: "{app}\ailimits-auth.exe"; Parameters: "remove copilot"; Flags: runhidden; RunOnceId: "RmKeyCopilot"
Filename: "{app}\ailimits-auth.exe"; Parameters: "remove-usage-token claude"; Flags: runhidden; RunOnceId: "RmUsageClaude"
Filename: "{app}\ailimits-auth.exe"; Parameters: "remove-usage-token codex"; Flags: runhidden; RunOnceId: "RmUsageCodex"

[UninstallDelete]
; Remove the user config and cache as well.
Type: filesandordirs; Name: "{userappdata}\AiLimits"
