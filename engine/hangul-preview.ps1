param(
  [Parameter(Mandatory=$true)][ValidateSet('create','verify')][string]$Command,
  [string]$InputPath,
  [string]$PdfPath,
  [Parameter(Mandatory=$true)][string]$ReportPath
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Utility/Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop
Import-Module (Join-Path $PSScriptRoot '../scripts/runtime.psm1') -Force -DisableNameChecking
$engine=Join-Path $PSScriptRoot 'hangul-preview.mjs'
if($Command -eq 'verify') {
  Invoke-AutoswNode -ArgumentList @($engine,'verify',$ReportPath)
  exit $LASTEXITCODE
}
if(-not $InputPath -or -not $PdfPath){throw 'InputPath와 PdfPath가 필요합니다.'}
if(Test-Path -LiteralPath $ReportPath){throw '기존 미리보기 기록을 덮어쓰지 않습니다.'}
$hash=(Get-FileHash -LiteralPath $InputPath -Algorithm SHA256).Hash
$candidate=Join-Path ([IO.Path]::GetTempPath()) ('autosw-preview-'+[guid]::NewGuid().ToString('N')+'.hwpx')
try {
  $result=& (Join-Path $PSScriptRoot 'hangul-verify.ps1') -InputPath $InputPath -OutputPath $candidate -PdfPath $PdfPath | ConvertFrom-Json
  Invoke-AutoswNode -ArgumentList @($engine,'bind',$InputPath,$PdfPath,$hash,[string]$result.savedPages,$ReportPath)
  if($LASTEXITCODE){throw '미리보기 기록 실패. 생성된 PDF는 최신 확인을 통과하지 못했습니다.'}
} finally {if([IO.File]::Exists($candidate)){[IO.File]::Delete($candidate)}}
