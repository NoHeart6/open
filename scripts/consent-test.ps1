param([Parameter(Mandatory)][string]$TaskId,[Parameter(Mandatory)][int]$AgentPid,[ValidateSet('yes','no')][string]$Decision='no')
$ErrorActionPreference='Stop'
# This test driver targets only the exact task dialog belonging to the fixture agent.
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class FixtureConsent {
 public delegate bool Callback(IntPtr hwnd, IntPtr state);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback callback, IntPtr state);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder title,int length);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left,top,right,bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x,y; }
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr hwnd,int id);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd,uint msg,IntPtr wp,IntPtr lp);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd,uint msg,IntPtr wp,IntPtr lp);
}
'@
$script:found=[IntPtr]::Zero
[void][FixtureConsent]::SetProcessDpiAwarenessContext([IntPtr](-4))
$expected="OpenCommander consent $TaskId"
[FixtureConsent]::EnumWindows({param($h,$s)
 $title=[Text.StringBuilder]::new(512);[void][FixtureConsent]::GetWindowText($h,$title,512)
 [uint32]$ownerPid=0;[void][FixtureConsent]::GetWindowThreadProcessId($h,[ref]$ownerPid)
 if($ownerPid -eq $AgentPid -and $title.ToString() -eq $expected){$script:found=$h}
 return $true
},[IntPtr]::Zero) | Out-Null
if($script:found -eq [IntPtr]::Zero){throw 'Expected fixture consent dialog not found'}
$button=if($Decision -eq 'yes'){6}else{7}
[void][FixtureConsent]::SetForegroundWindow($script:found)
$foreground=[FixtureConsent]::GetForegroundWindow() -eq $script:found
$control=[FixtureConsent]::GetDlgItem($script:found,$button)
$rect=[FixtureConsent+RECT]::new()
if($control -eq [IntPtr]::Zero -or -not [FixtureConsent]::GetWindowRect($control,[ref]$rect)){throw 'Consent button not found'}
$point=[FixtureConsent+POINT]::new();$point.x=[int](($rect.left+$rect.right)/2);$point.y=[int](($rect.top+$rect.bottom)/2)
# Grant foreground input to this exact dialog, then activate its exact button.
# BM_CLICK avoids pointer movement from other applications racing the fixture click.
if($foreground){[FixtureConsent]::keybd_event(16,0,0,[UIntPtr]::Zero);[FixtureConsent]::keybd_event(16,0,2,[UIntPtr]::Zero)}
[void][FixtureConsent]::SendMessage($control,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
Write-Output "Fixture consent $Decision applied to exact task"
