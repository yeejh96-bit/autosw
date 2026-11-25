# 개발용 끝 검사(Claude Code Stop 훅). Rust 코드·시험·Cargo·스크립트가 바뀐 경우에만 check.ps1 -Quick을 실행한다.
# 실패하면 이유를 오류 출력에 쓰고 2로 끝나 Claude가 고치게 한다.
# 같은 변경 상태에서 이미 실패를 알렸으면 다시 막지 않고 경고만 남겨 끝없는 반복을 막는다.
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$root = if ($env:CLAUDE_PROJECT_DIR) { $env:CLAUDE_PROJECT_DIR } else { Split-Path -Parent $PSScriptRoot }
$utf8 = [Text.UTF8Encoding]::new($false)

if (-not (Get-Command git -ErrorAction SilentlyContinue)) { exit 0 }
$ErrorActionPreference = 'Continue'
$status = & git -C $root status --porcelain=v1 -z --untracked-files=all 2>$null
$gitCode = $LASTEXITCODE
$head = & git -C $root rev-parse --verify -q HEAD 2>$null
$ErrorActionPreference = 'Stop'
if ($gitCode -ne 0) { exit 0 }

# -z 출력: "XY 경로" 항목이 NUL로 나뉘고, 이름 바꾸기는 원래 경로가 다음 항목으로 붙는다.
$entries = @("$status" -split "`0" | Where-Object { $_ })
$relevant = [Collections.Generic.List[string]]::new()
$pattern = '^(src|tests|engine|scripts|\.cargo)/|^(Cargo\.toml|Cargo\.lock|rust-toolchain\.toml|build\.rs)$'
for ($i = 0; $i -lt $entries.Count; $i++) {
  $entry = $entries[$i]
  if ($entry.Length -lt 4) { continue }
  $code = $entry.Substring(0, 2)
  $path = $entry.Substring(3)
  if ($code -match '[RC]') { $i++ }
  if ($path -match $pattern) { $relevant.Add("$code $path") }
}
if ($relevant.Count -eq 0) { exit 0 }

# 지문: HEAD + 관련 파일의 상태와 현재 내용 해시
$lines = [Collections.Generic.List[string]]::new()
$lines.Add("HEAD $head")
foreach ($item in ($relevant | Sort-Object)) {
  $file = Join-Path $root $item.Substring(3)
  $hash = if (Test-Path -LiteralPath $file -PathType Leaf) { (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash } else { '-' }
  $lines.Add("$item $hash")
}
$sha = [Security.Cryptography.SHA256]::Create()
$fingerprint = -join ($sha.ComputeHash($utf8.GetBytes(($lines -join "`n"))) | ForEach-Object { $_.ToString('x2') })

$stateDir = Join-Path $root 'target'
$stateFile = Join-Path $stateDir 'stop-check.json'
$previous = $null
if (Test-Path -LiteralPath $stateFile) {
  try { $previous = [IO.File]::ReadAllText($stateFile, $utf8) | ConvertFrom-Json } catch { $previous = $null }
}
if ($previous -and $previous.fingerprint -eq $fingerprint) {
  if ($previous.result -eq 'pass') { exit 0 }
  # 같은 실패 상태를 이미 알렸다: 다시 막지 않고 사용자에게 경고만 보여 준다.
  $message = '끝 검사 경고: 빠른 검사(서식·clippy)가 실패한 상태 그대로입니다. 고치지 않고 마쳤습니다. scripts/check.ps1 -Quick 결과를 확인하세요.'
  Write-Output (ConvertTo-Json -Compress @{ systemMessage = $message })
  exit 0
}

$check = Join-Path $PSScriptRoot 'check.ps1'
$ErrorActionPreference = 'Continue'
$output = & powershell -NoProfile -ExecutionPolicy Bypass -File $check -Quick 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
$result = if ($code -eq 0) { 'pass' } else { 'fail' }

[void][IO.Directory]::CreateDirectory($stateDir)
$state = ConvertTo-Json -Compress @{ fingerprint = $fingerprint; result = $result; checkedAt = (Get-Date).ToString('s') }
[IO.File]::WriteAllText($stateFile, $state + "`n", $utf8)

if ($code -eq 0) { exit 0 }
$tail = @($output | Where-Object { $_ -notmatch '^\s+(Compiling|Checking) ' } | Select-Object -Last 60)
[Console]::Error.WriteLine('끝 검사 실패: Rust 코드·스크립트 변경 뒤 빠른 검사(cargo fmt --check, cargo clippy -D warnings)가 통과하지 못했습니다. 아래 원인을 고친 뒤 마치세요.')
foreach ($line in $tail) { [Console]::Error.WriteLine($line) }
exit 2
