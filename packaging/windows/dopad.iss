#ifndef AppVersion
  #error AppVersion must be supplied from the Cargo workspace version.
#endif
#define AppName "Dopad"

[Setup]
AppId={{4EDDE7A7-52B1-4D1E-98AF-17D6B40E41B5}
AppName={#AppName}
AppVersion={#AppVersion}
DefaultDirName={autopf}\Dopad
DefaultGroupName=Dopad
UninstallDisplayIcon={app}\dopad-gui.exe
ArchitecturesInstallIn64BitMode=x64
OutputDir=..\..\dist\windows
OutputBaseFilename=dopad-v{#AppVersion}-windows-x86_64-setup
PrivilegesRequired=lowest
ChangesEnvironment=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "addtopath"; Description: "Add Dopad CLI to the current user's PATH"; GroupDescription: "Command line:"; Flags: checkedonce

[Files]
Source: "..\..\dist\windows\Dopad\dopad-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\windows\Dopad\bin\dopad.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Dopad"; Filename: "{app}\dopad-gui.exe"; WorkingDir: "{userdocs}"

[Code]
const
  EnvironmentKey = 'Environment';
  PathValue = 'Path';
  InstallerKey = 'Software\Dopad\Installer';
  CliPathAddedValue = 'CliPathAdded';
  CliPathEntryValue = 'CliPathEntry';

function SamePathEntry(Left, Right: string): Boolean;
begin
  Result := CompareText(RemoveBackslash(Trim(Left)), RemoveBackslash(Trim(Right))) = 0;
end;

function PathContainsEntry(PathValueText, Entry: string): Boolean;
var
  StartAt, SeparatorAt: Integer;
  Part: string;
begin
  Result := False;
  StartAt := 1;
  while StartAt <= Length(PathValueText) + 1 do
  begin
    SeparatorAt := Pos(';', Copy(PathValueText, StartAt, Length(PathValueText) - StartAt + 1));
    if SeparatorAt = 0 then
    begin
      Part := Copy(PathValueText, StartAt, Length(PathValueText) - StartAt + 1);
      StartAt := Length(PathValueText) + 2;
    end
    else
    begin
      Part := Copy(PathValueText, StartAt, SeparatorAt - 1);
      StartAt := StartAt + SeparatorAt;
    end;
    if SamePathEntry(Part, Entry) then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

function RemovePathEntry(PathValueText, Entry: string; var HasRemaining: Boolean): string;
var
  StartAt, SeparatorAt: Integer;
  Part: string;
begin
  Result := '';
  HasRemaining := False;
  StartAt := 1;
  while StartAt <= Length(PathValueText) + 1 do
  begin
    SeparatorAt := Pos(';', Copy(PathValueText, StartAt, Length(PathValueText) - StartAt + 1));
    if SeparatorAt = 0 then
    begin
      Part := Copy(PathValueText, StartAt, Length(PathValueText) - StartAt + 1);
      StartAt := Length(PathValueText) + 2;
    end
    else
    begin
      Part := Copy(PathValueText, StartAt, SeparatorAt - 1);
      StartAt := StartAt + SeparatorAt;
    end;
    if not SamePathEntry(Part, Entry) then
    begin
      if HasRemaining then Result := Result + ';';
      Result := Result + Part;
      HasRemaining := True;
    end;
  end;
end;

procedure AddCliPath;
var
  CurrentPath, UpdatedPath, BinPath, PreviousEntry, WasAdded: string;
  HasRemaining, WasOwned: Boolean;
begin
  BinPath := ExpandConstant('{app}\bin');
  if not RegQueryStringValue(HKCU, 'Environment', PathValue, CurrentPath) then
    CurrentPath := '';
  WasOwned := RegQueryStringValue(HKCU, InstallerKey, CliPathAddedValue, WasAdded) and (WasAdded = '1');
  if WasOwned and RegQueryStringValue(HKCU, InstallerKey, CliPathEntryValue, PreviousEntry) and
     (not SamePathEntry(PreviousEntry, BinPath)) then
  begin
    UpdatedPath := RemovePathEntry(CurrentPath, PreviousEntry, HasRemaining);
    if UpdatedPath <> CurrentPath then
    begin
      CurrentPath := UpdatedPath;
      if HasRemaining then
        RegWriteExpandStringValue(HKCU, 'Environment', PathValue, CurrentPath)
      else
      begin
        RegDeleteValue(HKCU, 'Environment', PathValue);
        CurrentPath := '';
      end;
    end;
  end;
  if not PathContainsEntry(CurrentPath, BinPath) then
  begin
    if CurrentPath <> '' then CurrentPath := CurrentPath + ';';
    CurrentPath := CurrentPath + BinPath;
    RegWriteExpandStringValue(HKCU, 'Environment', PathValue, CurrentPath);
    WasOwned := True;
  end;
  if WasOwned then
  begin
    RegWriteStringValue(HKCU, InstallerKey, CliPathAddedValue, '1');
    RegWriteStringValue(HKCU, InstallerKey, CliPathEntryValue, BinPath)
  end
  else
  begin
    RegWriteStringValue(HKCU, InstallerKey, CliPathAddedValue, '0');
    RegDeleteValue(HKCU, InstallerKey, CliPathEntryValue);
  end;
end;

procedure RemoveCliPath;
var
  CurrentPath, UpdatedPath, BinPath, WasAdded, PathEntry: string;
  HasRemaining: Boolean;
begin
  BinPath := ExpandConstant('{app}\bin');
  if RegQueryStringValue(HKCU, InstallerKey, CliPathAddedValue, WasAdded) and (WasAdded = '1') and
     RegQueryStringValue(HKCU, 'Environment', PathValue, CurrentPath) then
  begin
    if not RegQueryStringValue(HKCU, InstallerKey, CliPathEntryValue, PathEntry) then
      PathEntry := BinPath;
    UpdatedPath := RemovePathEntry(CurrentPath, PathEntry, HasRemaining);
    if UpdatedPath <> CurrentPath then
    begin
      if not HasRemaining then
        RegDeleteValue(HKCU, 'Environment', PathValue)
      else
        RegWriteExpandStringValue(HKCU, 'Environment', PathValue, UpdatedPath);
    end;
  end;
  RegDeleteValue(HKCU, InstallerKey, CliPathAddedValue);
  RegDeleteValue(HKCU, InstallerKey, CliPathEntryValue);
  RegDeleteKeyIfEmpty(HKCU, InstallerKey);
  RegDeleteKeyIfEmpty(HKCU, 'Software\Dopad');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if WizardIsTaskSelected('addtopath') then
      AddCliPath
    else
      RemoveCliPath;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveCliPath;
end;
