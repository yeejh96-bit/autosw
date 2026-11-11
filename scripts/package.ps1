param([switch]$Check)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'
$runtime = Join-Path $root 'runtime'
$exe = Join-Path $runtime 'autosw.exe'
$hashFile = Join-Path $runtime 'autosw.exe.sha256'
$version = (Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $root 'plugin.json') | ConvertFrom-Json).version
$utf8 = [Text.UTF8Encoding]::new($false)
Push-Location -LiteralPath $root
try {
  if (-not $Check) {
    cargo build --locked --release --target $target
    if ($LASTEXITCODE -ne 0) { throw 'Rust 실행 파일 빌드 실패' }
    $built = Join-Path $root "target/$target/release/autosw.exe"
    if ((& $built --version) -cne "autosw $version" -or $LASTEXITCODE -ne 0) { throw '실행 파일과 플러그인 버전이 다릅니다.' }
    $metadata = cargo metadata --locked --format-version 1 --filter-platform $target | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Rust 라이선스 목록 생성 실패' }
    $packages = $metadata.packages | Where-Object { $_.source } | Sort-Object name, version
    $licenses = [Collections.Generic.List[string]]::new()
    $licenses.Add('autosw Windows x64 실행 파일: Rust 의존성 고지')
    foreach ($package in $packages) {
      $licenses.Add("`n$($package.name) $($package.version) — $($package.license)")
      if ($package.repository) { $licenses.Add($package.repository) }
      foreach ($file in (Get-ChildItem -LiteralPath (Split-Path -Parent $package.manifest_path) -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT)([._-].*)?$' } | Sort-Object Name)) {
        $licenses.Add("`n$($file.Name)")
        $licenses.Add(([IO.File]::ReadAllText($file.FullName)).TrimEnd())
      }
    }
    [void][IO.Directory]::CreateDirectory($runtime)
    [IO.File]::Copy($built, $exe, $true)
    [IO.File]::WriteAllText($hashFile, ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() + "`n"), $utf8)
    [IO.File]::WriteAllText((Join-Path $runtime 'THIRD-PARTY.txt'), (($licenses -join "`n").Replace("`r`n", "`n") + "`n"), $utf8)
  }
  $expected = (Get-Content -LiteralPath $hashFile -Raw).Trim()
  if ($expected -cnotmatch '^[0-9a-f]{64}$' -or (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $expected) { throw '포함된 실행 파일의 SHA-256이 다릅니다.' }
  if ((& $exe --version) -cne "autosw $version" -or $LASTEXITCODE -ne 0) { throw '실행 파일과 플러그인 버전이 다릅니다.' }
  Write-Output "autosw $version Windows x64 패키지 확인"
} finally { Pop-Location }
