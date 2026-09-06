; Script generated for OOS-Lite Windows Installer
; Developer / Publisher: pudo58

#define MyAppName "OOS-Lite"
#define MyAppVersion "0.2.0"
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
OutputBaseFilename=OOS-Lite-Setup-v0.2.0
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=app.ico
VersionInfoVersion=0.2.0.0
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
Name: "contextmenu"; Description: "Integrate with Windows Explorer right-click context menu (Files & Folders)"; GroupDescription: "System Integration:"

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#MyAppGuiExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "app.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\uninstall.bat"; DestDir: "{app}"; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\oos-lite"
Type: filesandordirs; Name: "{app}"

[Icons]
Name: "{group}\{#MyAppName} Dashboard"; Filename: "{app}\{#MyAppGuiExeName}"; IconFilename: "{app}\app.ico"
Name: "{group}\{#MyAppName} Command Line"; Filename: "{cmd}"; Parameters: "/k ""{app}\{#MyAppExeName}"" --help"; WorkingDir: "{userdocs}"; IconFilename: "{app}\app.ico"
Name: "{group}\Uninstall & Stop {#MyAppName}"; Filename: "{uninstallexe}"; IconFilename: "{app}\app.ico"
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

    if WizardIsTaskSelected('contextmenu') then
    begin
      Exec(ExpandConstant('{app}\oos-lite.exe'),
        'shell-ext enable',
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

procedure KillProcessesAndPorts;
var
  ResultCode: Integer;
begin
  // 1. Unmap virtual drive Z:
  Exec('net.exe', 'use Z: /delete /y', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  // 2. Terminate running processes (oos-lite-gui.exe and oos-lite.exe)
  Exec('taskkill.exe', '/F /T /IM oos-lite-gui.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /T /IM oos-lite.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  // 3. Kill any processes holding ports 3000 and 8080 via PowerShell
  Exec('powershell.exe',
    '-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-NetTCPConnection -LocalPort 3000, 8080 -State Listen -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  // 4. Remove all context menu registry keys directly from HKCU
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\*\shell\OOSLite');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\Directory\shell\OOSLite');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\Directory\Background\shell\OOSLite');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\OOSLite.FileMenu');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\OOSLite.DirMenu');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\OOSLite.DirMenuBg');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\OOSLite.Menu');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\OOSLite.MenuBg');

  // 5. Allow Windows kernel to release file handles and TCP ports
  Sleep(1000);
end;

function InitializeUninstall(): Boolean;
begin
  KillProcessesAndPorts;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppDir, OrigPath, NewPath, StorePath: string;
  P: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    KillProcessesAndPorts;

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

    DelTree(ExpandConstant('{localappdata}\oos-lite'), True, True, True);
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    StorePath := GetEnv('USERPROFILE');
    if StorePath = '' then
      StorePath := ExpandConstant('{userdocs}\..');
    StorePath := StorePath + '\.oos-store';

    if DirExists(StorePath) then
    begin
      if MsgBox('Do you also want to remove your OOS-Lite personal vault and stored files?'#13#10#13#10 +
                'Location: ' + StorePath + #13#10#13#10 +
                'Click Yes to delete all stored files, or No to keep your vault for future use.',
                mbConfirmation, MB_YESNO or $100) = idYes then
      begin
        DelTree(StorePath, True, True, True);
      end;
    end;
  end;
end;
