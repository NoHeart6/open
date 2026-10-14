param([Parameter(Mandatory)][string]$Title)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DesktopFixtureWindow {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
}
'@
$form=[Windows.Forms.Form]::new()
$form.Text=$Title
$form.Size=[Drawing.Size]::new(520,380)
$form.Add_Shown({[void][DesktopFixtureWindow]::ShowWindow($form.Handle,5)})
$first=[Windows.Forms.TextBox]::new()
$first.AccessibleName='Repeated editor'
$first.Location=[Drawing.Point]::new(20,20)
$form.Controls.Add($first)
$second=[Windows.Forms.TextBox]::new()
$second.AccessibleName='Repeated editor'
$second.Location=[Drawing.Point]::new(20,55)
$form.Controls.Add($second)
$parent=$form
for($i=0;$i -lt 9;$i++){
    $panel=[Windows.Forms.Panel]::new()
    $panel.AccessibleName="Level $i"
    $panel.AccessibleRole=[Windows.Forms.AccessibleRole]::Grouping
    $panel.Location=[Drawing.Point]::new(5, $(if($i -eq 0){100}else{5}))
    $panel.Size=[Drawing.Size]::new((460-$i*10),(220-$i*10))
    $parent.Controls.Add($panel)
    $parent=$panel
}
$deep=[Windows.Forms.TextBox]::new()
$deep.AccessibleName='Deep unique editor'
$deep.Location=[Drawing.Point]::new(5,5)
$parent.Controls.Add($deep)
$timer=[Windows.Forms.Timer]::new()
$timer.Interval=180000
$timer.Add_Tick({$form.Close()})
$timer.Start()
try{[void]$form.ShowDialog()}finally{$timer.Dispose();$form.Dispose()}
