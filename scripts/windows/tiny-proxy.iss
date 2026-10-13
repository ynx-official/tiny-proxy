#ifndef MyAppVersion
  #error MyAppVersion is required
#endif
#ifndef SourceExe
  #error SourceExe is required
#endif
#ifndef SetupIcon
  #error SetupIcon is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef OutputBaseFilename
  #error OutputBaseFilename is required
#endif

[Setup]
AppId={{A1D8C90D-F9C8-44E6-9D82-671A8D41E7A4}
AppName=Tiny Proxy
AppVersion={#MyAppVersion}
AppVerName=Tiny Proxy {#MyAppVersion}
AppPublisher=Tiny Proxy
AppPublisherURL=https://github.com/ynx-official/tiny-proxy
AppSupportURL=https://github.com/ynx-official/tiny-proxy/issues
AppUpdatesURL=https://github.com/ynx-official/tiny-proxy/releases
DefaultDirName={localappdata}\Programs\TinyProxy
DefaultGroupName=Tiny Proxy
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile={#SetupIcon}
UninstallDisplayIcon={app}\tiny-proxy.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "tiny-proxy.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Tiny Proxy"; Filename: "{app}\tiny-proxy.exe"; AppUserModelID: "TinyProxy.Desktop"
Name: "{autodesktop}\Tiny Proxy"; Filename: "{app}\tiny-proxy.exe"; AppUserModelID: "TinyProxy.Desktop"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\TinyProxy.Desktop"; ValueType: string; ValueName: "DisplayName"; ValueData: "Tiny Proxy"; Flags: uninsdeletekey

[Run]
Filename: "{app}\tiny-proxy.exe"; Description: "{cm:LaunchProgram,Tiny Proxy}"; Flags: nowait postinstall skipifsilent
