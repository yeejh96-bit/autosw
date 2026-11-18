#requires -Version 5.1
param([string]$LegacyRevision = 'aa92d23')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$test = Join-Path $temp ('autosw-plugin-test-' + [guid]::NewGuid().ToString('N'))
$git = (Get-Command git.exe -CommandType Application | Select-Object -First 1).Source
$codex = (Get-Command codex.exe -CommandType Application | Select-Object -First 1).Source
$claude = (Get-Command claude.exe -CommandType Application | Select-Object -First 1).Source
$powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
$feed = Join-Path $test 'feed'
$work = Join-Path $test '기존 # 업무 폴더'
$url = 'https://github.com/autosw-test-fixture/autosw.git'

function Quote([string]$Value) {
  $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
  '"' + [regex]::Replace($escaped, '(\\+)$', '$1$1') + '"'
}

function ReadJson([string]$Path) { [IO.File]::ReadAllText($Path) | ConvertFrom-Json }

function Run([string]$Tool, [string[]]$Arguments, [string]$Profile = '', [switch]$Failure) {
  $processInfo = [Diagnostics.ProcessStartInfo]::new($Tool)
  $processInfo.WorkingDirectory = $test
  $processInfo.UseShellExecute = $false
  $processInfo.CreateNoWindow = $true
  $processInfo.RedirectStandardOutput = $true
  $processInfo.RedirectStandardError = $true
  $processInfo.StandardOutputEncoding = [Text.Encoding]::UTF8
  $processInfo.StandardErrorEncoding = [Text.Encoding]::UTF8
  $processInfo.Arguments = @($Arguments | ForEach-Object { Quote $_ }) -join ' '
  $processInfo.EnvironmentVariables['PATH'] = "$(Split-Path -Parent $git);$env:SystemRoot\System32;$env:SystemRoot"
  $processInfo.EnvironmentVariables['GIT_TERMINAL_PROMPT'] = '0'
  if ($Profile) {
    $processInfo.EnvironmentVariables['CODEX_HOME'] = $Profile
    $processInfo.EnvironmentVariables['CLAUDE_CONFIG_DIR'] = $Profile
    $processInfo.EnvironmentVariables['GIT_CONFIG_COUNT'] = '1'
    $processInfo.EnvironmentVariables['GIT_CONFIG_KEY_0'] = 'url.' + ([uri]::new($feed)).AbsoluteUri + '.insteadOf'
    $processInfo.EnvironmentVariables['GIT_CONFIG_VALUE_0'] = $url
  }
  $process = [Diagnostics.Process]::Start($processInfo)
  try {
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(120000)) { $process.Kill(); throw "시험 명령 시간 초과: $($Arguments[0])" }
    if (($process.ExitCode -eq 0) -eq [bool]$Failure) { throw "$($Arguments[0]) 종료 코드 $($process.ExitCode): $($stderr.Result) $($stdout.Result)" }
    $stdout.Result
  } finally { $process.Dispose() }
}

function FixtureGit([string[]]$Arguments) {
  Run $git (@('-C', $feed, '-c', 'user.name=autosw fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'core.autocrlf=false') + $Arguments) | Out-Null
}

function InstallPlugin([string]$Profile) {
  Run $codex @('plugin', 'add', 'autosw@autosw-marketplace', '--json') $Profile | Out-Null
}

function Cache([string]$Profile, [string]$Version) {
  $path = Join-Path $Profile 'plugins/cache/autosw-marketplace/autosw'
  $installed = @(Get-ChildItem -LiteralPath $path -Directory | Where-Object {
    $manifest = Join-Path $_.FullName 'plugin.json'
    [IO.File]::Exists($manifest) -and ((ReadJson $manifest).version -eq $Version)
  })
  if ($installed.Count -ne 1) { throw "설치된 플러그인 $Version 버전을 찾지 못했습니다." }
  $installed[0].FullName
}

function Setup([string]$Cache, [string]$Workspace, [string]$Version) {
  $diagnosis = Run $powershell @('-NoProfile', '-NonInteractive', '-File', (Join-Path $Cache 'scripts/install.ps1'), '-Workspace', $Workspace) | ConvertFrom-Json
  if (-not $diagnosis.workspaceInitialized -or ($diagnosis.checks | Where-Object name -eq 'autosw Rust').version -ne $Version) {
    throw '첫 설정이나 실행 파일 버전 확인 실패'
  }
}

[void][IO.Directory]::CreateDirectory($test)
try {
  $upgradeProfile = Join-Path $test 'upgrade-profile'
  $freshProfile = Join-Path $test 'fresh-profile'
  foreach ($dir in @($feed, $upgradeProfile, $freshProfile, (Join-Path $work '.autosw'))) { [void][IO.Directory]::CreateDirectory($dir) }
  $archive = Join-Path $test 'legacy.tar'
  Run $git @('-C', $root, 'archive', "--output=$archive", $LegacyRevision) | Out-Null
  Run (Join-Path $env:SystemRoot 'System32/tar.exe') @('-xf', $archive, '-C', $feed) | Out-Null
  FixtureGit @('init', '-q', '-b', 'main')
  FixtureGit @('add', '--all')
  FixtureGit @('commit', '-q', '-m', 'Legacy plugin test fixture')
  $legacy = (ReadJson (Join-Path $feed 'plugin.json')).version
  Run $codex @('plugin','marketplace','add',$url,'--ref','main','--json') $upgradeProfile | Out-Null
  InstallPlugin $upgradeProfile
  [void](Cache $upgradeProfile $legacy)
  Write-Output "시험 카탈로그에 이전 플러그인 $legacy 설치"

  $ledger = Join-Path $work '.autosw/ledger.json'
  $settings = Join-Path $work 'autosw.config.json'
  [IO.File]::Copy((Join-Path $PSScriptRoot 'fixtures/ledger-v1.json'), $ledger)
  [IO.File]::WriteAllText($settings, '{"schemaVersion":1,"institution":"Synthetic existing settings"}')
  $beforeLedger = (Get-FileHash -LiteralPath $ledger).Hash
  $beforeSettings = (Get-FileHash -LiteralPath $settings).Hash

  FixtureGit @('rm', '-q', '-r', '--', '.')
  foreach ($entry in @('.agents','.claude-plugin','plugin.json','scripts','runtime','skills','docs','README.md','NOTICE','LICENSE','src/hwpx_layout/data')) {
    $destination = Join-Path $feed $entry
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $destination))
    Copy-Item -LiteralPath (Join-Path $root $entry) -Destination $destination -Recurse
  }
  FixtureGit @('add', '--all')
  FixtureGit @('commit', '-q', '-m', 'Rust plugin test fixture')
  $version = (ReadJson (Join-Path $feed 'plugin.json')).version
  Run $codex @('plugin','marketplace','upgrade','autosw-marketplace','--json') $upgradeProfile | Out-Null
  InstallPlugin $upgradeProfile
  $updated = Cache $upgradeProfile $version
  Setup $updated $work $version
  $launch = Join-Path $updated 'scripts/autosw.ps1'
  $query = Run $powershell @('-NoProfile','-NonInteractive','-File',$launch,'ledger-query','--workspace',$work) | ConvertFrom-Json
  $expected = ReadJson (Join-Path $PSScriptRoot 'fixtures/ledger-v1-report.json')
  if ($query.sourceHash -ne $expected.sourceHash -or $query.totals.expenseTotal -ne $expected.totals.expenseTotal) { throw '기존 장부 조회가 달라졌습니다.' }
  if ($beforeLedger -ne (Get-FileHash -LiteralPath $ledger).Hash -or $beforeSettings -ne (Get-FileHash -LiteralPath $settings).Hash) { throw '기존 업무 파일이 변경됐습니다.' }
  Write-Output "플러그인 업데이트 후 기존 장부·설정 대조 통과"

  Run $codex @('plugin','marketplace','add',$url,'--ref','main','--json') $freshProfile | Out-Null
  InstallPlugin $freshProfile
  $fresh = Cache $freshProfile $version
  $newWorkspace = Join-Path $test '새 # 업무 폴더'
  Setup $fresh $newWorkspace $version
  if ([IO.File]::Exists((Join-Path $newWorkspace '.autosw/ledger.json'))) { throw '초기 설정에서 장부를 자동으로 썼습니다.' }
  $content = @{ message = '한글 "따옴표" C:\폴더 이름\' } | ConvertTo-Json -Compress
  $arguments = @((Join-Path $fresh 'scripts/autosw.ps1'), 'write-draft', '--workspace', $newWorkspace, '--output', 'drafts/quote.json', '--content', $content)
  $escaped = ($arguments | ForEach-Object { "'" + $_.Replace("'", "''") + "'" }) -join ' '
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes('& ' + $escaped + '; exit $LASTEXITCODE'))
  Run $powershell @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded) | Out-Null
  if ([IO.File]::ReadAllText((Join-Path $newWorkspace 'drafts/quote.json')) -cne $content) { throw 'Windows PowerShell에서 원고 인자가 달라졌습니다.' }
  $guide = Run $powershell @('-NoProfile','-NonInteractive','-File',(Join-Path $fresh 'scripts/autosw.ps1'),'guide','--topic','references/convert.md') | ConvertFrom-Json
  if ($guide.topic -ne 'references/convert.md') { throw 'Codex 설치본에서 변환 지침을 찾지 못했습니다.' }

  $claudeProfile = Join-Path $test 'claude-profile'
  [void][IO.Directory]::CreateDirectory($claudeProfile)
  Run $claude @('plugin','validate',$feed) $claudeProfile | Out-Null
  Run $claude @('plugin','marketplace','add',$feed) $claudeProfile | Out-Null
  Run $claude @('plugin','install','autosw@autosw-marketplace') $claudeProfile | Out-Null
  $claudeCache = Join-Path $claudeProfile "plugins/cache/autosw-marketplace/autosw/$version"
  if ((ReadJson (Join-Path $claudeCache '.claude-plugin/plugin.json')).version -ne $version) { throw 'Claude Code 설치 버전이 다릅니다.' }
  Setup $claudeCache $work $version
  $guide = Run $powershell @('-NoProfile','-NonInteractive','-File',(Join-Path $claudeCache 'scripts/autosw.ps1'),'guide','--topic','references/convert.md') | ConvertFrom-Json
  if ($guide.topic -ne 'references/convert.md') { throw 'Claude Code 설치본에서 변환 지침을 찾지 못했습니다.' }
  if ($beforeLedger -ne (Get-FileHash -LiteralPath $ledger).Hash -or $beforeSettings -ne (Get-FileHash -LiteralPath $settings).Hash) { throw 'Claude Code 설정이 기존 업무 파일을 변경했습니다.' }
  Write-Output "Claude Code 신규 설치와 Codex 업무 폴더 공동 사용 통과"
  $badWorkspace = Join-Path $test '손상 패키지 시험'
  [IO.File]::AppendAllText((Join-Path $fresh 'runtime/autosw.exe'), 'test-tamper')
  Run $powershell @('-NoProfile','-NonInteractive','-File',(Join-Path $fresh 'scripts/install.ps1'),'-Workspace',$badWorkspace) -Failure | Out-Null
  if ([IO.Directory]::Exists($badWorkspace)) { throw '실행 파일 검사 전에 업무 폴더를 생성했습니다.' }
  Write-Output "Codex CLI 신규 설치 및 $legacy → $version 업데이트, Windows PowerShell 첫 설정, 기존 장부·설정 보존, 손상 패키지 차단 통과"
} finally {
  $exact = (Get-Item -LiteralPath $test).FullName
  if ((Split-Path -Parent $exact) -ne $temp.TrimEnd('\','/') -or -not (Split-Path -Leaf $exact).StartsWith('autosw-plugin-test-')) {
    throw "정리 대상 확인 실패: $exact"
  }
  Remove-Item -LiteralPath $exact -Recurse -Force
}
