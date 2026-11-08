param()
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'runtime.psm1') -Force
$exe = Get-AutoswNode
$npm = Join-Path (Split-Path -Parent $exe) 'npm.cmd'
if (-not (Test-Path -LiteralPath $npm -PathType Leaf)) { throw '선택한 Node 설치의 npm.cmd가 없습니다.' }
$savedPath = $env:Path
try {
  $env:Path = (Split-Path -Parent $exe) + [IO.Path]::PathSeparator + $savedPath
  & $npm ci --prefix (Split-Path -Parent $PSScriptRoot) --no-audit --no-fund
  if ($LASTEXITCODE -ne 0) { throw '의존성 설치 실패' }
  Invoke-AutoswNode -ArgumentList @((Join-Path $PSScriptRoot '../bin/autosw.mjs'), 'doctor')
} finally { $env:Path = $savedPath }
