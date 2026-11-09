param([string]$Workspace)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'runtime.psm1') -Force
$root = Split-Path -Parent $PSScriptRoot
try { $exe = Get-AutoswNode } catch {
  if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Node 자동 준비는 Windows x64에서 지원합니다. 이 PC에 맞는 최신 Node.js LTS를 설치한 뒤 다시 시도하세요.' }
  $destination = Get-AutoswPrivateNodeDirectory
  $local = Split-Path -Parent $destination
  if (Test-Path -LiteralPath $destination) { throw '기존 autosw 전용 Node를 사용할 수 없습니다. 플러그인 설치 상태를 확인하세요.' }
  [void][IO.Directory]::CreateDirectory($local)
  if ((Get-Item -LiteralPath $local -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'autosw 실행 폴더가 연결 경로입니다.' }
  $stage = Join-Path $local ('download-' + [guid]::NewGuid().ToString('N'))
  [void][IO.Directory]::CreateDirectory($stage)
  try {
    $release = Split-Path -Leaf $destination
    $zip = Join-Path $stage ($release + '.zip')
    Invoke-WebRequest -UseBasicParsing -Uri ('https://nodejs.org/dist/v26.13.0/' + $release + '.zip') -OutFile $zip -TimeoutSec 120
    # Use the Windows runtime directly so module lookup cannot block the hash check.
    $hasher = [Security.Cryptography.SHA256]::Create()
    try {
      $archive = [IO.File]::OpenRead($zip)
      try {
        $actualHash = [BitConverter]::ToString($hasher.ComputeHash($archive)).Replace('-', '')
      } finally { $archive.Dispose() }
    } finally { $hasher.Dispose() }
    if ($actualHash -ne 'DCC60D59A0746999F99064A01C020445D54D84228C691CDBF25B49AC3B59150A') {
      throw '공식 Node 설치 파일의 확인값이 다릅니다.'
    }
    & (Join-Path $env:SystemRoot 'System32/tar.exe') -xf $zip -C $stage
    if ($LASTEXITCODE -ne 0) { throw 'Node 압축을 풀지 못했습니다.' }
    $source = Join-Path $stage $release
    & (Join-Path $source 'node.exe') (Join-Path $PSScriptRoot 'check-node.mjs')
    if ($LASTEXITCODE -ne 0) { throw '받은 Node 실행 파일을 확인하지 못했습니다.' }
    [IO.Directory]::Move($source, $destination)
  } finally {
    if ([IO.Directory]::Exists($stage)) { [IO.Directory]::Delete($stage, $true) }
  }
  $exe = Get-AutoswNode
}
$npmCli = Join-Path (Split-Path -Parent $exe) 'node_modules/npm/bin/npm-cli.js'
if (-not (Test-Path -LiteralPath $npmCli -PathType Leaf)) { throw '선택한 Node 설치의 npm 도구가 없습니다.' }
$savedPath = $env:Path
try {
  $env:Path = (Split-Path -Parent $exe) + [IO.Path]::PathSeparator + $savedPath
  & $exe $npmCli ci --prefix $root --ignore-scripts --no-audit --no-fund | Out-Host
  if ($LASTEXITCODE -ne 0) { throw '의존성 설치 실패' }
  $entry = Join-Path $root 'bin/autosw.mjs'
  $workspaceArgs = if ($Workspace) { @('--workspace', $Workspace) }
  $initialized = Invoke-AutoswNode -ArgumentList (@($entry, 'init') + @($workspaceArgs | Where-Object { $_ }))
  if ($LASTEXITCODE -ne 0) { throw '업무 폴더를 준비하지 못했습니다.' }
  $selected = ($initialized | ConvertFrom-Json).workspace
  Invoke-AutoswNode -ArgumentList @($entry, 'doctor', '--workspace', $selected)
  exit $LASTEXITCODE
} finally { $env:Path = $savedPath }
