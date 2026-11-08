param(
  [Parameter(Mandatory = $true)][ValidateSet('preview', 'apply', 'verify', 'render')][string]$Command,
  [string]$InputPath,
  [string]$OutputPath,
  [string]$PlanPath,
  [string]$ConfirmHash,
  [string]$ReportPath,
  [ValidateSet('native', 'layout')][string]$Engine
)
$ErrorActionPreference = 'Stop'
$runtime = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scripts/runtime.psm1'))
$enginePath = Join-Path $PSScriptRoot 'hwpx-create.mjs'
$taskArguments = @($Command)
if ($InputPath) { $taskArguments += @('--input', $InputPath) }
if ($OutputPath) { $taskArguments += @('--output', $OutputPath) }
if ($PlanPath) { $taskArguments += @('--plan', $PlanPath) }
if ($ConfirmHash) { $taskArguments += @('--confirm', $ConfirmHash) }
if ($ReportPath) { $taskArguments += @('--report', $ReportPath) }
if ($Engine) { $taskArguments += @('--engine', $Engine) }
try {
  Import-Module $runtime -Force -DisableNameChecking -ErrorAction Stop
  Invoke-AutoswNode -ArgumentList (@($enginePath) + $taskArguments)
  exit $LASTEXITCODE
} catch {
  [Console]::Error.WriteLine($_.Exception.Message)
  exit 1
}
