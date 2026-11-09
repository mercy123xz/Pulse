; Pulse installer (Inno Setup 6). Built by GitHub Actions:
;   iscc /DAppVersion=0.3.0 installer\pulse.iss
; Installs for the current user only, so no administrator password is needed.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{8E3F2B61-5C4A-4D7E-9B1A-50C0F7A1D3E2}
AppName=Pulse
AppVersion={#AppVersion}
AppVerName=Pulse {#AppVersion}
AppPublisher=Pulse
AppPublisherURL=https://github.com/mercy123xz/Pulse
AppSupportURL=https://github.com/mercy123xz/Pulse/issues
AppUpdatesURL=https://github.com/mercy123xz/Pulse/releases
DefaultDirName={localappdata}\Programs\Pulse
DefaultGroupName=Pulse
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=PulseSetup-{#AppVersion}
SetupIconFile=..\assets\icon\pulse.ico
UninstallDisplayIcon={app}\pulse.exe
UninstallDisplayName=Pulse
WizardStyle=modern
WizardImageFile=..\assets\icon\installer-side.bmp
WizardSmallImageFile=..\assets\icon\installer-small.bmp
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "portuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\pulse.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pulse"; Filename: "{app}\pulse.exe"
Name: "{autodesktop}\Pulse"; Filename: "{app}\pulse.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\pulse.exe"; Description: "{cm:LaunchProgram,Pulse}"; Flags: nowait postinstall skipifsilent

; Your likes, playlists and settings in %APPDATA%\pulse-player are kept when Pulse is uninstalled,
; so reinstalling picks up where you left off.
