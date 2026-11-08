param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'runtime.psm1') -Force
Invoke-AutoswNode -ArgumentList (@((Join-Path $PSScriptRoot '../bin/autosw.mjs')) + $Arguments)
exit $LASTEXITCODE
