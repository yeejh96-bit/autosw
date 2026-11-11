param([string]$Workspace)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'runtime/autosw.exe'
$launcher = Join-Path $PSScriptRoot 'autosw.ps1'
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') { throw 'autosw는 Windows x64 PC에서 지원합니다.' }
if (-not [IO.File]::Exists($exe)) { throw '플러그인의 실행 파일이 없습니다. autosw를 다시 설치하세요.' }
$hash = [IO.File]::ReadAllText((Join-Path $root 'runtime/autosw.exe.sha256')).Trim()
$version = ([IO.File]::ReadAllText((Join-Path $root 'plugin.json')) | ConvertFrom-Json).version
$hasher = [Security.Cryptography.SHA256]::Create()
try {
  $file = [IO.File]::OpenRead($exe)
  try { $actual = [BitConverter]::ToString($hasher.ComputeHash($file)).Replace('-', '') }
  finally { $file.Dispose() }
} finally { $hasher.Dispose() }
if ($hash -cnotmatch '^[0-9a-f]{64}$' -or $actual -ine $hash) {
  throw '플러그인 실행 파일의 SHA-256이 다릅니다. autosw를 다시 설치하세요.'
}
if ((& $launcher --version) -cne "autosw $version" -or $LASTEXITCODE -ne 0) { throw '플러그인과 실행 파일의 버전이 다릅니다. 업데이트를 다시 적용하세요.' }
$workspaceArgs = @()
if ($Workspace) { $workspaceArgs = @('--workspace', $Workspace) }
$initialized = & $launcher init @workspaceArgs
if ($LASTEXITCODE -ne 0) { throw '업무 폴더를 준비하지 못했습니다.' }
$selected = ($initialized | ConvertFrom-Json).workspace
& $launcher doctor --workspace $selected
exit $LASTEXITCODE
