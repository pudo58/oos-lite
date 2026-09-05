; Script generated for OOS-Lite Windows Installer
; Developer / Publisher: pudo58

#define MyAppName "OOS-Lite"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "pudo58"
#define MyAppURL "https://github.com/pudo58/oos-lite"
#define MyAppExeName "oos-lite.exe"
#define MyAppGuiExeName "oos-lite-gui.exe"

[Setup]
AppId={{D37E601A-544D-4F1E-8B53-73B8A462F38C}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE-MIT
OutputDir=..\dist
OutputBaseFilename=OOS-Lite-Setup-v0.1.0
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=app.ico
VersionInfoVersion=0.1.0.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=OOS-Lite Setup - Content-Addressed File Storage & Vault Drive
VersionInfoCopyright=Copyright (C) 2026 {#MyAppPublisher}
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion=0.1.0.0
ChangesEnvironment=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "Add OOS-Lite to User PATH environment variable (enables running 'oos-lite' anywhere in terminal)"; GroupDescription: "System Integration:"

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#MyAppGuiExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "app.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName} Dashboard"; Filename: "{app}\{#MyAppGuiExeName}"; IconFilename: "{app}\app.ico"
Name: "{group}\{#MyAppName} Command Line"; Filename: "{cmd}"; Parameters: "/k ""{app}\{#MyAppExeName}"" --help"; WorkingDir: "{userdocs}"; IconFilename: "{app}\app.ico"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppGuiExeName}"; IconFilename: "{app}\app.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppGuiExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Code]
var
  PasswordPage: TInputQueryWizardPage;

procedure InitializeWizard;
begin
  PasswordPage := CreateInputQueryPage(
    wpSelectTasks,
    'Vault Encryption Setup (Optional)',
    'Protect your personal storage vault with XChaCha20-Poly1305 AEAD encryption',
    'Enter a master passphrase to encrypt all stored data in your OOS-Lite vault.'#13#10 +
    'If you do not want encryption, leave blank and click Next to continue in unencrypted mode.'
  );

  PasswordPage.Add('Master Passphrase:', True);
  PasswordPage.Add('Confirm Passphrase:', True);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = PasswordPage.ID then
  begin
    if (PasswordPage.Values[0] <> '') or (PasswordPage.Values[1] <> '') then
    begin
      if PasswordPage.Values[0] <> PasswordPage.Values[1] then
      begin
        MsgBox('Passwords do not match! Please re-enter.', mbError, MB_OK);
        Result := False;
        exit;
      end;
      if Length(PasswordPage.Values[0]) < 4 then
      begin
        MsgBox('Passphrase must be at least 4 characters long!', mbError, MB_OK);
        Result := False;
        exit;
      end;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  StorePath: string;
  AppDataDir: string;
  PassFile: string;
  PassVal: string;
begin
  if CurStep = ssPostInstall then
  begin
    StorePath := GetEnv('USERPROFILE');
    if StorePath = '' then
      StorePath := ExpandConstant('{userdocs}\..');
    StorePath := StorePath + '\.oos-store';
    PassVal := PasswordPage.Values[0];
    AppDataDir := ExpandConstant('{localappdata}\oos-lite');
    ForceDirectories(AppDataDir);

    if PassVal <> '' then
    begin
      PassFile := AppDataDir + '\vault.pass';
      SaveStringToFile(PassFile, PassVal, False);

      Exec(ExpandConstant('{app}\oos-lite.exe'),
        Format('--store "%s" --password-file "%s" init', [StorePath, PassFile]),
        '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    end
    else
    begin
      Exec(ExpandConstant('{app}\oos-lite.exe'),
        Format('--store "%s" init', [StorePath]),
        '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    end;
  end;
end;

function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath)
  then begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + UpperCase(Param) + ';', ';' + UpperCase(OrigPath) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppDir, OrigPath, NewPath: string;
  P: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    AppDir := ExpandConstant('{app}');
    if RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
    begin
      P := Pos(';' + AppDir, OrigPath);
      if P > 0 then
      begin
        NewPath := Copy(OrigPath, 1, P - 1) + Copy(OrigPath, P + Length(';' + AppDir), Length(OrigPath));
        RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', NewPath);
      end else
      begin
        P := Pos(AppDir + ';', OrigPath);
        if P > 0 then
        begin
          NewPath := Copy(OrigPath, 1, P - 1) + Copy(OrigPath, P + Length(AppDir + ';'), Length(OrigPath));
          RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', NewPath);
        end;
      end;
    end;
  end;
end;
