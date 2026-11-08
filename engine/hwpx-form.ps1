param(
  [Parameter(Mandatory = $true)][ValidateSet('inspect', 'preview', 'apply', 'verify')][string]$Command,
  [string]$InputPath,
  [string]$RequestPath,
  [string]$OutputPath,
  [string]$PlanPath,
  [string]$ConfirmHash
)
$ErrorActionPreference = 'Stop'
$runtime = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scripts/runtime.psm1'))
$engine = Join-Path $PSScriptRoot 'hwpx-form.mjs'
$taskArguments = @($Command)
if ($InputPath) { $taskArguments += @('--input', $InputPath) }
if ($RequestPath) { $taskArguments += @('--request', $RequestPath) }
if ($OutputPath) { $taskArguments += @('--output', $OutputPath) }
if ($PlanPath) { $taskArguments += @('--plan', $PlanPath) }
if ($ConfirmHash) { $taskArguments += @('--confirm', $ConfirmHash) }
try {
  Import-Module $runtime -Force -DisableNameChecking -ErrorAction Stop
  # .cmd를 거치면 파일명의 & 등이 셸 연산자로 재해석될 수 있어 exe에 인자 배열을 전달한다.
  Invoke-AutoswNode -ArgumentList (@($engine) + $taskArguments)
  exit $LASTEXITCODE
} catch {
  [Console]::Error.WriteLine($_.Exception.Message)
  exit 1
}
