$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Remove-Item "$toolsDir\gflow.exe" -Force -ErrorAction SilentlyContinue
