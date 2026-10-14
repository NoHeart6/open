param([Parameter(Mandatory)][string]$ProfileRoot,[Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
$runtime=[IO.Path]::GetFullPath((Join-Path (Split-Path $PSScriptRoot -Parent) '.runtime'))
$target=[IO.Path]::GetFullPath($ProfileRoot)
if(!$target.StartsWith($runtime+'\vscode-launch-fixture-',[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected fixture profile path'}
$expectedExe=[IO.Path]::GetFullPath($Executable)
$roots=@(Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object {$_.ExecutablePath -eq $expectedExe -and $_.CommandLine -and $_.CommandLine.Contains($target)})
$all=@(Get-CimInstance Win32_Process -Filter "Name='Code.exe'")
$owned=@{};foreach($root in $roots){$owned[[int]$root.ProcessId]=$root}
do{
    $changed=$false
    foreach($candidate in $all){
        if($candidate.ExecutablePath -eq $expectedExe -and $owned.ContainsKey([int]$candidate.ParentProcessId) -and !$owned.ContainsKey([int]$candidate.ProcessId)){
            $owned[[int]$candidate.ProcessId]=$candidate;$changed=$true
        }
    }
}while($changed)
foreach($item in $owned.Values){
    $current=Get-CimInstance Win32_Process -Filter "ProcessId=$($item.ProcessId)"
    if($current -and $current.ExecutablePath -eq $expectedExe -and $current.CreationDate -eq $item.CreationDate){Stop-Process -Id $item.ProcessId -ErrorAction SilentlyContinue}
}
for($attempt=0;$attempt -lt 40;$attempt++){
    $remaining=@(Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object {$_.ExecutablePath -eq $expectedExe -and $_.CommandLine -and $_.CommandLine.Contains($target)})
    if(!$remaining.Count){exit 0}
    Start-Sleep -Milliseconds 100
}
throw 'Fixture VS Code processes remain'
