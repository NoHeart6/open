param([switch]$ShowPassword,[string]$StatusPath)
$ErrorActionPreference='Stop'
function LoginStatus([string]$Stage){if($StatusPath){[IO.File]::WriteAllText($StatusPath,$Stage)}}
LoginStatus 'starting'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()
$repo=Split-Path -Parent $PSScriptRoot
$protected=Get-Content (Join-Path $repo '.runtime/railway/secrets.dpapi.json') -Raw|ConvertFrom-Json
$password=[Net.NetworkCredential]::new('',(ConvertTo-SecureString $protected.OPENCOMMANDER_OWNER_PASSWORD)).Password
LoginStatus 'credentials-loaded'
$window=New-Object Windows.Forms.Form
$window.Text='OpenCommander - Login pribadi';$window.Size=New-Object Drawing.Size(570,235);$window.StartPosition='CenterScreen';$window.TopMost=$true;$window.ShowInTaskbar=$true;$window.WindowState='Normal'
$window.Add_Shown({$window.Activate();$window.BringToFront();LoginStatus 'shown'})
$label=New-Object Windows.Forms.Label;$label.Text="Login OAuth OpenCommander: username owner`r`nPassword disimpan terenkripsi DPAPI pada profil Windows ini.";$label.Location=New-Object Drawing.Point(15,15);$label.Size=New-Object Drawing.Size(530,45);$window.Controls.Add($label)
$box=New-Object Windows.Forms.TextBox;$box.Location=New-Object Drawing.Point(15,70);$box.Width=525;$box.ReadOnly=$true;$box.UseSystemPasswordChar=$true;$box.Text=$password;$window.Controls.Add($box)
$show=New-Object Windows.Forms.CheckBox;$show.Text='Tampilkan password';$show.Location=New-Object Drawing.Point(15,105);$show.Width=210;$show.Add_CheckedChanged({$box.UseSystemPasswordChar=!$show.Checked});$window.Controls.Add($show)
if($ShowPassword){$show.Checked=$true}
$copy=New-Object Windows.Forms.Button;$copy.Text='Salin password';$copy.Location=New-Object Drawing.Point(330,100);$copy.Width=210;$copy.Add_Click({[Windows.Forms.Clipboard]::SetText($box.Text)});$window.Controls.Add($copy)
$note=New-Object Windows.Forms.Label;$note.Text='Gunakan hanya di auth-production-b4e7.up.railway.app. Jangan kirim ke chat.';$note.Location=New-Object Drawing.Point(15,145);$note.Size=New-Object Drawing.Size(525,35);$window.Controls.Add($note)
LoginStatus 'form-created'
$null=$window.ShowDialog();$box.Text='';$password='';LoginStatus 'closed'
