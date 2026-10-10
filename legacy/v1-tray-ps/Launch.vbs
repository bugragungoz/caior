' croxz aio runtimes - silent launcher
Set fso = CreateObject("Scripting.FileSystemObject")
Set shell = CreateObject("WScript.Shell")

' Get script directory (src folder)
scriptDir = fso.GetParentFolderName(WScript.ScriptFullName)
' Get parent directory (root where runtimes folder is)
rootDir = fso.GetParentFolderName(scriptDir)

psPath = scriptDir & "\TrayInstaller.ps1"
shell.Run "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File """ & psPath & """ -RuntimesPath """ & rootDir & """", 0, True
