$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot '../runtime/autosw.exe'
if (-not [IO.File]::Exists($exe)) { throw '플러그인의 실행 파일이 없습니다. autosw를 다시 설치하세요.' }
$previous = [Console]::OutputEncoding
try {
  $encoding = [Text.UTF8Encoding]::new($false)
  [Console]::OutputEncoding = $encoding
  $start = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($exe))
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $start.StandardOutputEncoding = $encoding
  $start.StandardErrorEncoding = $encoding
  # Windows CRT quoting; pass the arguments directly to the executable, without a shell.
  $start.Arguments = @(foreach ($arg in $args) {
    $value = [regex]::Replace([string]$arg, '(\\*)"', '$1$1\"')
    '"' + [regex]::Replace($value, '(\\+)$', '$1$1') + '"'
  }) -join ' '
  $process = [Diagnostics.Process]::Start($start)
  try {
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    if ($stdout.Result.Length) { Write-Output $stdout.Result.TrimEnd([char[]]@(13, 10)) }
    if ($stderr.Result.Length) { [Console]::Error.Write($stderr.Result) }
    $result = $process.ExitCode
  } finally { $process.Dispose() }
} finally { [Console]::OutputEncoding = $previous }
exit $result
