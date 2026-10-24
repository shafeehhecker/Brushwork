; Inno Setup script for Brushwork (https://jrsoftware.org/isinfo.php)
; Build:  iscc /DAppVersion=0.1.0 /DSourceExe=..\target\release\brushwork.exe installer\brushwork.iss

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\brushwork.exe"
#endif

[Setup]
AppId={{6B0C3A8E-4F2D-4C61-9B0E-2D8B7C1F5A13}
AppName=Brushwork
AppVersion={#AppVersion}
AppPublisher=Brushwork contributors
DefaultDirName={autopf}\Brushwork
DefaultGroupName=Brushwork
DisableProgramGroupPage=yes
; Installs for the current user without administrator rights unless the user chooses otherwise.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputBaseFilename=Brushwork-{#AppVersion}-setup
OutputDir=..\dist
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\brushwork.exe
SetupIconFile=..\assets\brushwork.ico
LicenseFile=..\LICENSE
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "openwith"; Description: "Add Brushwork to ""Open with"" for PNG, JPEG and BMP images"; GroupDescription: "File types:"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "brushwork.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Brushwork"; Filename: "{app}\brushwork.exe"
Name: "{autodesktop}\Brushwork"; Filename: "{app}\brushwork.exe"; Tasks: desktopicon

[Registry]
; ProgId used by "Open with". Never takes over the default handler.
Root: HKA; Subkey: "Software\Classes\Brushwork.Image"; ValueType: string; ValueData: "Image"; Flags: uninsdeletekey; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Brushwork.Image\DefaultIcon"; ValueType: string; ValueData: "{app}\brushwork.exe,0"; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Brushwork.Image\shell\open\command"; ValueType: string; ValueData: """{app}\brushwork.exe"" ""%1"""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\brushwork.exe\SupportedTypes"; ValueType: string; ValueName: ".png"; ValueData: ""; Flags: uninsdeletekey; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\brushwork.exe\SupportedTypes"; ValueType: string; ValueName: ".jpg"; ValueData: ""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\brushwork.exe\SupportedTypes"; ValueType: string; ValueName: ".jpeg"; ValueData: ""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\brushwork.exe\SupportedTypes"; ValueType: string; ValueName: ".bmp"; ValueData: ""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueType: string; ValueName: "Brushwork.Image"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueType: string; ValueName: "Brushwork.Image"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueType: string; ValueName: "Brushwork.Image"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\.bmp\OpenWithProgids"; ValueType: string; ValueName: "Brushwork.Image"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith

[Run]
Filename: "{app}\brushwork.exe"; Description: "{cm:LaunchProgram,Brushwork}"; Flags: nowait postinstall skipifsilent
