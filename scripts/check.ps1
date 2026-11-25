# 공개 전 Rust·패키지 검사를 순서대로 실행한다. 첫 실패에서 멈추고 0이 아닌 값으로 끝난다.
# -Quick: 서식·clippy만 확인한다(개발 중 끝 검사용).
param([switch]$Quick)
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$root = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
  $cargoBin = Join-Path $cargoHome 'bin'
  if (Test-Path -LiteralPath (Join-Path $cargoBin 'cargo.exe')) { $env:Path = "$cargoBin;$env:Path" }
  else { [Console]::Error.WriteLine('검사 실패: cargo를 찾지 못했습니다. Rust를 설치했는지 확인하세요.'); exit 1 }
}


$steps = [Collections.Generic.List[object]]::new()
$steps.Add(@{ Name = '서식(cargo fmt)'; Exe = 'cargo'; Args = @('fmt', '--all', '--', '--check') })
$steps.Add(@{ Name = '경고(cargo clippy)'; Exe = 'cargo'; Args = @('clippy', '--locked', '--all-targets', '--target', $target, '--', '-D', 'warnings') })
if (-not $Quick) {
  $steps.Add(@{ Name = '시험(cargo test)'; Exe = 'cargo'; Args = @('test', '--locked', '--target', $target) })
  $steps.Add(@{ Name = '포함 실행 파일(package.ps1 -Check)'; Exe = 'powershell'; Args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'package.ps1'), '-Check') })
}

$done = [Collections.Generic.List[string]]::new()
Push-Location -LiteralPath $root
try {
  foreach ($step in $steps) {
    Write-Output "== $($step.Name)"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $global:LASTEXITCODE = 0
    # 외부 명령의 오류 출력을 PowerShell 오류로 바꾸지 않도록 이 단계만 계속 진행 모드로 둔다.
    $ErrorActionPreference = 'Continue'
    & $step.Exe @($step.Args) 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) {
      [Console]::Error.WriteLine("검사 실패: $($step.Name) 단계에서 멈췄습니다(종료 코드 $code). 위 출력을 확인해 고친 뒤 다시 실행하세요.")
      if ($done.Count) { [Console]::Error.WriteLine("통과한 단계: $($done -join ', ')") }
      exit 1
    }
    $done.Add("$($step.Name) $([math]::Round($watch.Elapsed.TotalSeconds))초")
  }
} finally { Pop-Location }

$kind = if ($Quick) { '빠른 검사' } else { '전체 검사' }
Write-Output "$kind 통과: $($done -join ', ')"
exit 0
