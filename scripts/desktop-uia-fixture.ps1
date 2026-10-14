param([Parameter(Mandatory)][string]$Title,[switch]$Interactions,[string]$TraceFile)
$ErrorActionPreference='Stop'
$env:PSModulePath=(Join-Path $PSHOME 'Modules')+';'+$env:PSModulePath
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DesktopFixtureWindow {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
}
'@
if($TraceFile){
Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition @'
using System;
using System.IO;
using System.Windows.Forms;
public class FixtureMouseTrace : IMessageFilter {
    public string Path;
    public bool PreFilterMessage(ref Message m) {
        if(m.Msg >= 0x200 && m.Msg <= 0x20e) File.AppendAllText(Path, m.Msg + " " + m.HWnd + " " + m.LParam + Environment.NewLine);
        return false;
    }
}
'@
$trace=[FixtureMouseTrace]::new();$trace.Path=$TraceFile
[Windows.Forms.Application]::AddMessageFilter($trace)
}
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
if($Interactions){
    $button=[Windows.Forms.Button]::new()
    $button.Text='Invoke fixture'
    $button.AccessibleName='Invoke fixture'
    $button.Location=[Drawing.Point]::new(220,20)
    $button.Size=[Drawing.Size]::new(150,28)
    $label=[Windows.Forms.Label]::new()
    $label.Text='Invocation count: 0'
    $label.AccessibleName='Invocation count: 0'
    $label.Location=[Drawing.Point]::new(220,55)
    $label.Size=[Drawing.Size]::new(190,25)
    $script:clicks=0
    $button.Add_Click({$script:clicks++;$label.Text="Invocation count: $script:clicks";$label.AccessibleName=$label.Text})
    $form.Controls.Add($button)
    $form.Controls.Add($label)
    $choices=[Windows.Forms.ListBox]::new()
    $choices.AccessibleName='Selection fixture'
    $choices.Location=[Drawing.Point]::new(220,90)
    $choices.Size=[Drawing.Size]::new(190,70)
    [void]$choices.Items.Add('Fixture choice one')
    [void]$choices.Items.Add('Fixture choice two')
    for($i=3;$i -le 30;$i++){[void]$choices.Items.Add("Fixture choice $i")}
    $form.Controls.Add($choices)
}
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
if($Interactions){$button.BringToFront();$label.BringToFront();$choices.BringToFront()}
$timer=[Windows.Forms.Timer]::new()
$timer.Interval=180000
$timer.Add_Tick({$form.Close()})
$timer.Start()
try{[void]$form.ShowDialog()}finally{$timer.Dispose();$form.Dispose()}
