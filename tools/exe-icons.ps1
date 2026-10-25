# Usage: tools/exe-icons.ps1 PATH.exe
# Fails unless the executable carries an icon resource. Without one,
# Explorer, shortcuts and a pinned taskbar button show Windows' generic icon.
param([Parameter(Mandatory)][string]$Exe)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace Win32 -Name Shell -MemberDefinition @'
[DllImport("shell32.dll", CharSet = CharSet.Unicode)]
public static extern uint ExtractIconEx(string file, int index,
    IntPtr[] large, IntPtr[] small, uint icons);
'@
# Index -1 with no buffers asks only how many icons the file holds.
$count = [Win32.Shell]::ExtractIconEx((Resolve-Path $Exe).Path, -1, $null, $null, 0)
if ($count -lt 1) {
    Write-Error "$Exe has no icon resource"
    exit 1
}
Write-Output "$Exe carries $count icon(s)"
