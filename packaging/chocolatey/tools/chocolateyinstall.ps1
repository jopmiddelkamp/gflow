$ErrorActionPreference = 'Stop'
$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$version = $env:chocolateyPackageVersion
$url = "https://github.com/jopmiddelkamp/gflow/releases/download/v${version}/gflow-windows-x86_64.exe"
$checksum = '__CHECKSUM__'

Get-ChocolateyWebFile -PackageName 'gflow' `
  -FileFullPath "$toolsDir\gflow.exe" `
  -Url64bit $url `
  -Checksum64 $checksum `
  -ChecksumType64 'sha256'
