param(
  [Parameter(Mandatory=$true)][ValidateSet('x64','arm64')][string]$Arch,
  [Parameter(Mandatory=$true)][string]$Version,
  [Parameter(Mandatory=$true)][string]$Target
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Out = Join-Path $Root 'dist'; New-Item $Out -ItemType Directory -Force | Out-Null
$Id = Get-Content (Join-Path $Root 'packaging\store-identity.json') -Raw | ConvertFrom-Json
$Rel = Join-Path $Root "target\$Target\release"

# Portable ZIP
$Stage = Join-Path $Root "target\stage-$Arch"
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item $Stage -ItemType Directory | Out-Null
Copy-Item "$Rel\guitar-fx.exe","$Rel\guitar-fx-ui.exe",(Join-Path $Root 'README.md') $Stage
Copy-Item (Join-Path $Root 'presets') $Stage -Recurse
Compress-Archive "$Stage\*" (Join-Path $Out "guitar-fx-$Version-windows-$Arch.zip") -Force

# MSIX with the reserved Microsoft Store identity
$M = Join-Path $Root "target\msix-$Arch"
if (Test-Path $M) { Remove-Item $M -Recurse -Force }
Copy-Item $Stage $M -Recurse
$A = Join-Path $M 'Assets'; New-Item $A -ItemType Directory | Out-Null
Add-Type -AssemblyName System.Drawing
foreach ($s in @(@('StoreLogo',50),@('Square44x44Logo',44),@('Square150x150Logo',150))) {
  $b = [System.Drawing.Bitmap]::new($s[1],$s[1]); $g = [System.Drawing.Graphics]::FromImage($b)
  $g.Clear([System.Drawing.Color]::Transparent)
  $i = [System.Drawing.Image]::FromFile((Join-Path $Root 'packaging\guitar-fx.png'))
  $g.DrawImage($i,0,0,$s[1],$s[1]); $b.Save((Join-Path $A "$($s[0]).png"),[System.Drawing.Imaging.ImageFormat]::Png)
  $i.Dispose(); $g.Dispose(); $b.Dispose()
}
$x = Get-Content (Join-Path $PSScriptRoot 'AppxManifest.xml.in') -Raw
$ms = $Id.microsoft_store
$x = $x.Replace('@NAME@',$ms.package_identity_name).Replace('@PUBLISHER@',$ms.publisher).Replace('@PUBLISHER_DISPLAY@',$ms.publisher_display_name).Replace('@VERSION@',$Version).Replace('@ARCH@',$Arch)
Set-Content (Join-Path $M 'AppxManifest.xml') $x -Encoding utf8
$MakeAppx = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter makeappx.exe |
  Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
$Msix = Join-Path $Out "guitar-fx-$Version-windows-$Arch.msix"
& $MakeAppx.FullName pack /d $M /p $Msix /o
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Msix)) { throw 'MSIX build failed' }

# Optional signing (sideloading). The Store re-signs submissions, so unsigned is fine for upload.
if ($env:GUITARFX_PFX_B64 -and $env:GUITARFX_PFX_PASSWORD) {
  $pfx = Join-Path $Root 'target\sign.pfx'
  [IO.File]::WriteAllBytes($pfx,[Convert]::FromBase64String($env:GUITARFX_PFX_B64))
  $SignTool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter signtool.exe |
    Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
  & $SignTool.FullName sign /fd SHA256 /f $pfx /p $env:GUITARFX_PFX_PASSWORD $Msix
  if ($LASTEXITCODE -ne 0) { throw 'MSIX signing failed' }
  Remove-Item $pfx -Force
} else { Write-Host 'No signing secrets: MSIX left unsigned (fine for Store upload).' }
