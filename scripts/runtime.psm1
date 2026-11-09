$script:AutoswRoot = Split-Path -Parent $PSScriptRoot
$script:AutoswNodeRelease = 'node-v26.13.0-win-x64'
function Get-AutoswPrivateNodeDirectory {
  if (-not $env:LOCALAPPDATA -or -not [IO.Path]::IsPathRooted($env:LOCALAPPDATA)) { throw '사용자 실행 폴더 위치를 확인하지 못했습니다.' }
  Join-Path (Join-Path $env:LOCALAPPDATA 'autosw') $script:AutoswNodeRelease
}
function Get-AutoswNode {
  $local = Join-Path $script:AutoswRoot '.local\node\node.exe'
  $private = Join-Path (Get-AutoswPrivateNodeDirectory) 'node.exe'
  if (Test-Path -LiteralPath $local -PathType Leaf) { $exe = $local }
  elseif (Test-Path -LiteralPath $private -PathType Leaf) { $exe = $private }
  else { $exe = (Get-Command node.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source }
  & $exe (Join-Path $PSScriptRoot 'check-node.mjs')
  if ($LASTEXITCODE -ne 0) { throw '지원하는 Node가 없습니다. 플러그인의 초기 설정을 실행하세요.' }
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
Export-ModuleMember -Function Get-AutoswNode,Get-AutoswPrivateNodeDirectory,Invoke-AutoswNode
