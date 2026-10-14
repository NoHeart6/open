param([Parameter(Mandatory)][int]$ProcessId)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$ProcessId)
$windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
$window=$null
foreach($candidate in $windows){if($candidate.Current.ClassName -eq 'Notepad'){$window=$candidate;break}}
if($null -eq $window){throw 'Fixture Notepad window not found'}
$edit=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Document))
if($null -eq $edit){$edit=$window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))}
if($null -eq $edit){
 Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class NativeFixtureEditor {
 public delegate bool Callback(IntPtr hwnd,IntPtr state);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb,IntPtr state);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent,Callback cb,IntPtr state);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd,StringBuilder text,int count);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr hwnd,uint msg,UIntPtr wp,StringBuilder text,uint flags,uint timeout,out UIntPtr result);
}
'@
 $script:editor=[IntPtr]::Zero
 [NativeFixtureEditor]::EnumWindows({param($h,$s)
  [uint32]$ownerPid=0;[void][NativeFixtureEditor]::GetWindowThreadProcessId($h,[ref]$ownerPid)
  $class=[Text.StringBuilder]::new(128);[void][NativeFixtureEditor]::GetClassName($h,$class,128)
  if($ownerPid -eq $ProcessId -and $class.ToString() -eq 'Notepad'){
   [void][NativeFixtureEditor]::EnumChildWindows($h,{param($child,$unused)
    $name=[Text.StringBuilder]::new(128);[void][NativeFixtureEditor]::GetClassName($child,$name,128)
    if($name.ToString() -eq 'Edit'){$script:editor=$child}
    return $true
   },[IntPtr]::Zero)
  }
  return $true
 },[IntPtr]::Zero) | Out-Null
 if($script:editor -eq [IntPtr]::Zero){throw 'Fixture editor control not found in UI Automation or native window tree'}
 $text=[Text.StringBuilder]::new(4096);$result=[UIntPtr]::Zero
 if([NativeFixtureEditor]::SendMessageTimeout($script:editor,0xD,[UIntPtr]::new([uint64]4096),$text,2,2000,[ref]$result) -eq [IntPtr]::Zero){throw 'Native fixture text query failed'}
 Write-Output $text.ToString();exit 0
}
$pattern=$null
if($edit.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern,[ref]$pattern)){$pattern.DocumentRange.GetText(-1)}
elseif($edit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){$pattern.Current.Value}
else{throw 'Fixture editor cannot be inspected'}
