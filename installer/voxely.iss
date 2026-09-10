#define MyAppName "Voxely"
#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "AryaPaw"
#define MyAppURL "https://github.com/AryaPaw/voxely"
#define MyAppExeName "voxely.exe"

[Setup]
AppId={{A7E3C2F1-9B44-4D18-8E61-2F0C9A5B7D33}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppCopyright=Copyright (c) AryaPaw
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\Voxely
DefaultGroupName=Voxely
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=Voxely-Setup-win-x64-{#MyAppVersion}
SetupLogging=yes
CloseApplications=no
RestartApplications=no
RestartIfNeededByRun=no

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start with Windows"

#ifndef PublishDir
#define PublishDir "..\artifacts\publish"
#endif

[Files]
Source: "{#PublishDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Voxely"; Filename: "{app}\{#MyAppExeName}"
Name: "{userstartup}\Voxely"; Filename: "{app}\{#MyAppExeName}"; Tasks: startup

[Run]
Filename: "{app}\{#MyAppExeName}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#MyAppExeName}"; Flags: nowait skipifnotsilent

[Code]
function TaskKillImage(const ImageName: String): Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM ' + ImageName + ' /T', '', SW_HIDE, ewWaitUntilTerminated, Result);
end;

function WaitUntilAppExited: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to 30 do
  begin
    if TaskKillImage('voxely.exe') = 128 then
    begin
      Result := True;
      Exit;
    end;
    Sleep(250);
  end;
  Result := TaskKillImage('voxely.exe') = 128;
end;

function LockedAppMessage: String;
begin
  Result := 'Voxely is still running. Exit from the tray and run Setup again.';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  NeedsRestart := False;
  if WaitUntilAppExited then
    Result := ''
  else
    Result := LockedAppMessage;
end;

function InitializeUninstall(): Boolean;
begin
  Result := WaitUntilAppExited;
  if not Result then
    MsgBox(LockedAppMessage, mbError, MB_OK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    WaitUntilAppExited;
end;
