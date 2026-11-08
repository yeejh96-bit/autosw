param(
  [Parameter(Mandatory = $true)][string]$InputPath,
  [Parameter(Mandatory = $true)][string]$OutputPath,
  [Parameter(Mandatory = $true)][string]$PdfPath,
  [ValidateSet('HWPX', 'HWP')][string]$OutputFormat = 'HWPX',
  [string]$VerificationHwpxPath
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Utility/Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop
$source = (Resolve-Path -LiteralPath $InputPath).Path
$output = [IO.Path]::GetFullPath($OutputPath)
$pdf = [IO.Path]::GetFullPath($PdfPath)
$verification = if ($OutputFormat -eq 'HWP') {
  if (-not $VerificationHwpxPath) { throw 'HWP 출력에는 재저장 내용 대조용 VerificationHwpxPath가 필요합니다.' }
  [IO.Path]::GetFullPath($VerificationHwpxPath)
} else { $output }
if ([IO.Path]::GetExtension($source).ToLowerInvariant() -notin @('.hwp', '.hwpx')) { throw 'HWP 또는 HWPX 파일이 필요합니다.' }
if ([IO.Path]::GetExtension($output).ToLowerInvariant() -ne ('.' + $OutputFormat.ToLowerInvariant()) -or [IO.Path]::GetExtension($pdf).ToLowerInvariant() -ne '.pdf' -or [IO.Path]::GetExtension($verification).ToLowerInvariant() -ne '.hwpx') { throw '출력 형식과 파일 확장자를 확인하세요.' }
if ($source -eq $output -or $source -eq $pdf -or $output -eq $pdf) { throw '입력과 출력은 서로 다른 경로여야 합니다.' }
if ($OutputFormat -eq 'HWP' -and ($verification -eq $source -or $verification -eq $output -or $verification -eq $pdf)) { throw 'HWP 검증 사본도 별도 경로여야 합니다.' }
foreach ($path in (@($output, $pdf, $verification) | Select-Object -Unique)) {
  if ([IO.File]::Exists($path)) { throw "기존 파일을 덮어쓰지 않습니다: $path" }
  if (-not [IO.Directory]::Exists([IO.Path]::GetDirectoryName($path))) { throw "출력 폴더가 없습니다: $path" }
}
$sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
$dir = Join-Path ([IO.Path]::GetTempPath()) ('autosw-hangul-' + [guid]::NewGuid().ToString('N'))
$hwp = $null
try {
  [void][IO.Directory]::CreateDirectory($dir)
  $localSource = Join-Path $dir ('source' + [IO.Path]::GetExtension($source))
  $localOutput = Join-Path $dir ('checked.' + $OutputFormat.ToLowerInvariant())
  $localPdf = Join-Path $dir 'checked.pdf'
  $localVerification = if ($OutputFormat -eq 'HWP') { Join-Path $dir 'verification.hwpx' } else { $localOutput }
  [IO.File]::Copy($source, $localSource, $false)
  $hwp = New-Object -ComObject HWPFrame.HwpObject
  if (-not $hwp.Open($localSource, '', 'suspendpassword:TRUE;forceopen:TRUE')) { throw '한글에서 원본을 열지 못했습니다.' }
  $pages = $hwp.PageCount
  if (-not $hwp.SaveAs($localOutput, $OutputFormat, '')) { throw '한글 저장에 실패했습니다.' }
  [void]$hwp.Clear(1)
  if (-not $hwp.Open($localOutput, $OutputFormat, 'suspendpassword:TRUE;forceopen:TRUE')) { throw '저장본을 다시 열지 못했습니다.' }
  $savedPages = $hwp.PageCount
  if (-not $hwp.SaveAs($localPdf, 'PDF', '')) { throw '한글 PDF 출력에 실패했습니다.' }
  if ($OutputFormat -eq 'HWP' -and -not $hwp.SaveAs($localVerification, 'HWPX', '')) { throw 'HWP 재열기 후 내용 대조용 HWPX 저장에 실패했습니다.' }
  [void]$hwp.Clear(1)
  $signature = [IO.File]::ReadAllBytes($localOutput)
  $expected = if ($OutputFormat -eq 'HWP') { @(0xd0, 0xcf, 0x11, 0xe0) } else { @(0x50, 0x4b, 0x03, 0x04) }
  if ($signature.Length -lt 4) { throw '한글 출력 파일이 비어 있습니다.' }
  for ($i = 0; $i -lt 4; $i++) { if ($signature[$i] -ne $expected[$i]) { throw '한글 출력의 실제 형식이 요청과 다릅니다.' } }
  if ($sourceHash -ne (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash) { throw '원본이 작업 중 변경되어 결과를 공개하지 않았습니다.' }
  [IO.File]::Copy($localOutput, $output, $false)
  try { [IO.File]::Copy($localPdf, $pdf, $false) }
  catch { throw "한글파일은 저장됐지만 PDF 저장에 실패했습니다. 파일: $output. 오류: $_" }
  if ($OutputFormat -eq 'HWP') {
    try { [IO.File]::Copy($localVerification, $verification, $false) }
    catch { throw "한글파일과 PDF는 저장됐지만 내용 대조용 HWPX 저장에 실패했습니다. 파일: $output, PDF: $pdf. 오류: $_" }
  }
  [pscustomobject]@{ status='awaiting-visual-review'; inputPages=$pages; savedPages=$savedPages; output=$output; pdf=$pdf; verificationHwpx=$verification; format=$OutputFormat } | ConvertTo-Json -Compress
} finally {
  if ($null -ne $hwp) {
    try { $hwp.Quit() } catch { Write-Warning '이번 한글 인스턴스를 종료하지 못했습니다.' }
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($hwp)
  }
  foreach ($item in (@($localSource, $localOutput, $localPdf, $localVerification) | Select-Object -Unique)) {
    if ($item -and [IO.File]::Exists($item)) { [IO.File]::Delete($item) }
  }
  if ([IO.Directory]::Exists($dir)) { [IO.Directory]::Delete($dir, $false) }
}
