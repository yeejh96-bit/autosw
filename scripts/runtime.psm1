$script:AutoswRoot = Split-Path -Parent $PSScriptRoot
function Get-AutoswNode {
  $local = Join-Path $script:AutoswRoot '.local\node\node.exe'
  if (Test-Path -LiteralPath $local -PathType Leaf) { $exe = $local }
  else { $exe = (Get-Command node.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source }
  & $exe (Join-Path $PSScriptRoot 'check-node.mjs')
  if ($LASTEXITCODE -ne 0) { throw 'Node 24.22.1 이상 24.x를 설치하세요.' }
  return $exe
}
function Invoke-AutoswNode {
  param([string[]]$ArgumentList)
  $exe = Get-AutoswNode
  $savedPath = $env:Path
  $savedEncoding = [Console]::OutputEncoding
  try {
    $env:Path = (Split-Path -Parent $exe) + [IO.Path]::PathSeparator + $savedPath
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    & $exe @ArgumentList
    $global:LASTEXITCODE = $LASTEXITCODE
  } finally {
    $env:Path = $savedPath
    [Console]::OutputEncoding = $savedEncoding
  }
}
Export-ModuleMember -Function Get-AutoswNode,Invoke-AutoswNode
