; The Windows installer of Brewed Texture Designer (Inno Setup 6), built by packaging\windows\build.ps1,
; which gives it the version and where the program and the output are:
;   ISCC /DVersion=1.0.0-pre.1 /DSource=<repository> /DOutput=<repository>\target\dist installer.iss
;
; Installed for the user alone (no administrator asked), or for all where chosen; a Start menu
; entry, a desktop one where asked; .brewtex packages opened by it; uninstalled from Settings → Apps.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\.."
#endif
#ifndef Output
  #define Output Source + "\target\dist"
#endif

[Setup]
AppId={{6B3F2E1A-9C4D-4E7B-8A5F-2D1C0B9E7A41}
AppName=Brewed Texture Designer
AppVersion={#Version}
AppVerName=Brewed Texture Designer {#Version}
AppPublisher=Brewed Texture Designer
AppPublisherURL=https://github.com/wtfmydarling/BrewedTextureDesigner
DefaultDirName={autopf}\Brewed Texture Designer
DefaultGroupName=Brewed Texture Designer
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
UninstallDisplayIcon={app}\brewed-texture-designer.exe
LicenseFile={#Source}\LICENSE-MIT
OutputDir={#Output}
OutputBaseFilename=Brewed-Texture-Designer-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Source}\target\release\brewed-texture-designer.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Brewed Texture Designer"; Filename: "{app}\brewed-texture-designer.exe"
Name: "{autodesktop}\Brewed Texture Designer"; Filename: "{app}\brewed-texture-designer.exe"; Tasks: desktopicon

[Registry]
; .brewtex packages open in it.
Root: HKA; Subkey: "Software\Classes\.brewtex"; ValueType: string; ValueName: ""; ValueData: "BrewedTextureDesigner.Package"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\BrewedTextureDesigner.Package"; ValueType: string; ValueName: ""; ValueData: "Brewed Texture Designer package"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\BrewedTextureDesigner.Package\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\brewed-texture-designer.exe,0"
Root: HKA; Subkey: "Software\Classes\BrewedTextureDesigner.Package\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\brewed-texture-designer.exe"" ""%1"""

[Run]
Filename: "{app}\brewed-texture-designer.exe"; Description: "{cm:LaunchProgram,Brewed Texture Designer}"; Flags: nowait postinstall skipifsilent
