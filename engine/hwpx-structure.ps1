# HWPX의 구조를 독립적으로 읽는다. 업무 원본이나 ZIP 내용을 수정하지 않는다.
param([string]$StructureInputPath)
function Get-HwpxStructure {
  param([Parameter(Mandatory = $true)][string]$Path)
  $paragraphNamespace = 'http://www.hancom.co.kr/hwpml/2011/paragraph'
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  Add-Type -AssemblyName System.IO.Compression
  $fullPath = [IO.Path]::GetFullPath($Path)
  $file = Get-Item -LiteralPath $fullPath -ErrorAction Stop
  if ($file.PSIsContainer -or $file.Length -gt 128MB) { throw 'HWPX 파일이 아니거나 크기 제한을 넘었습니다.' }
  $bytes = [IO.File]::ReadAllBytes($fullPath)
  if ($bytes.Length -lt 4 -or $bytes[0] -ne 0x50 -or $bytes[1] -ne 0x4b -or $bytes[2] -ne 3 -or $bytes[3] -ne 4) {
    throw '실제 HWPX ZIP 형식이 아닙니다. HWP는 한글 변환 절차로 확인하세요.'
  }
  $eocd = -1
  for ($i = $bytes.Length - 22; $i -ge [Math]::Max(0, $bytes.Length - 65557); $i--) {
    if ([BitConverter]::ToUInt32($bytes, $i) -eq 0x06054b50 -and
      $i + 22 + [BitConverter]::ToUInt16($bytes, $i + 20) -eq $bytes.Length) { $eocd = $i; break }
  }
  if ($eocd -lt 0) { throw 'ZIP 끝 레코드가 없거나 잘못됐습니다.' }
  if ([BitConverter]::ToUInt16($bytes, $eocd + 10) -eq 65535 -or
    [BitConverter]::ToUInt32($bytes, $eocd + 12) -eq [uint32]::MaxValue -or
    [BitConverter]::ToUInt32($bytes, $eocd + 16) -eq [uint32]::MaxValue) { throw 'ZIP64는 지원하지 않습니다.' }
  if ([BitConverter]::ToUInt16($bytes, $eocd + 4) -ne 0 -or [BitConverter]::ToUInt16($bytes, $eocd + 6) -ne 0) {
    throw '여러 파일로 분할된 ZIP은 지원하지 않습니다.'
  }
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $sourceHash = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() }
  finally { $sha.Dispose() }
  $stream = New-Object IO.MemoryStream(,$bytes)
  $zip = $null
  try {
    $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Read, $false)
    $entryNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    [long]$expandedSize = 0
    foreach ($entry in $zip.Entries) {
      if (-not $entryNames.Add($entry.FullName)) { throw 'ZIP에 같은 이름의 항목이 두 번 있습니다.' }
      $expandedSize += $entry.Length
      if ($expandedSize -gt 256MB) { throw 'ZIP 압축 해제 크기 제한을 넘었습니다.' }
    }
    $sections = @($zip.Entries | Where-Object { $_.FullName -cmatch '^Contents/section(\d+)\.xml$' } |
      Sort-Object { [long]([regex]::Match($_.FullName, 'section(\d+)').Groups[1].Value) })
    if ($sections.Count -eq 0) { throw 'HWPX 본문(Contents/section*.xml)이 없습니다.' }
    $sectionResults = New-Object Collections.Generic.List[object]
    foreach ($entry in $sections) {
      if ($entry.Length -gt 32MB) { throw '본문 XML 크기 제한을 넘었습니다.' }
      $settings = [Xml.XmlReaderSettings]::new()
      $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
      $settings.XmlResolver = $null
      $settings.MaxCharactersInDocument = 32MB
      $entryStream = $entry.Open()
      $reader = $null
      try {
        $reader = [Xml.XmlReader]::Create($entryStream, $settings)
        $document = [Xml.XmlDocument]::new()
        $document.PreserveWhitespace = $true
        $document.XmlResolver = $null
        $document.Load($reader)
      } finally {
        if ($null -ne $reader) { $reader.Dispose() }
        $entryStream.Dispose()
      }
      $ns = [Xml.XmlNamespaceManager]::new($document.NameTable)
      $ns.AddNamespace('hp', $paragraphNamespace)
      $paragraphNodes = $document.SelectNodes('//hp:p', $ns)
      $tableNodes = $document.SelectNodes('//hp:tbl', $ns)
      $paragraphKeys = [Collections.Generic.Dictionary[Xml.XmlNode,string]]::new()
      $tableKeys = [Collections.Generic.Dictionary[Xml.XmlNode,string]]::new()
      $cellKeys = [Collections.Generic.Dictionary[Xml.XmlNode,string]]::new()
      for ($i = 0; $i -lt $paragraphNodes.Count; $i++) { $paragraphKeys.Add($paragraphNodes[$i], ($entry.FullName + '/p' + $i)) }
      for ($i = 0; $i -lt $tableNodes.Count; $i++) { $tableKeys.Add($tableNodes[$i], ($entry.FullName + '/tbl' + $i)) }
      $tableResults = New-Object Collections.Generic.List[object]
      foreach ($table in $tableNodes) {
        $tableKey = $tableKeys[$table]
        $rows = Get-HwpxInteger $table 'rowCnt' 1
        $cols = Get-HwpxInteger $table 'colCnt' 1
        if ([long]$rows * [long]$cols -gt 1000000) { throw '표의 행·열 크기 제한을 넘었습니다.' }
        $occupied = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        $cells = New-Object Collections.Generic.List[object]
        foreach ($cell in $table.SelectNodes('./hp:tr/hp:tc', $ns)) {
          $address = $cell.SelectSingleNode('./hp:cellAddr', $ns)
          $span = $cell.SelectSingleNode('./hp:cellSpan', $ns)
          if ($null -eq $address -or $null -eq $span) { throw '셀 주소 또는 병합 범위가 없습니다.' }
          $row = Get-HwpxInteger $address 'rowAddr' 0
          $col = Get-HwpxInteger $address 'colAddr' 0
          $rowSpan = Get-HwpxInteger $span 'rowSpan' 1
          $colSpan = Get-HwpxInteger $span 'colSpan' 1
          if ([long]$row + $rowSpan -gt $rows -or [long]$col + $colSpan -gt $cols) { throw '셀이 표 범위를 벗어났습니다.' }
          for ($r = $row; $r -lt $row + $rowSpan; $r++) {
            for ($c = $col; $c -lt $col + $colSpan; $c++) {
              if (-not $occupied.Add(($r.ToString() + ':' + $c))) { throw '셀 또는 병합 범위가 겹칩니다.' }
            }
          }
          $cellKey = $tableKey + '/r' + $row + 'c' + $col
          $cellKeys.Add($cell, $cellKey)
          $directParagraphs = @($cell.SelectNodes('./hp:subList/hp:p', $ns) | ForEach-Object { $paragraphKeys[$_] })
          $cells.Add([ordered]@{ key = $cellKey; row = $row; col = $col; rowSpan = $rowSpan; colSpan = $colSpan; paragraphs = $directParagraphs; childTables = @() })
        }
        $tableResults.Add([ordered]@{ key = $tableKey; id = $table.GetAttribute('id'); rows = $rows; cols = $cols; parentCell = $null; cells = @($cells.ToArray()) })
      }
      # 모든 셀 주소를 만든 뒤 중첩 관계를 연결한다.
      for ($i = 0; $i -lt $tableNodes.Count; $i++) {
        $parentCellNode = $tableNodes[$i].SelectSingleNode('ancestor::hp:tc[1]', $ns)
        if ($null -ne $parentCellNode) {
          if (-not $cellKeys.ContainsKey($parentCellNode)) { throw '지원하지 않는 중첩표 위치입니다.' }
          $parentKey = $cellKeys[$parentCellNode]
          $tableResults[$i].parentCell = $parentKey
          foreach ($tableResult in $tableResults) {
            foreach ($cellResult in $tableResult.cells) {
              if ($cellResult.key -ceq $parentKey) { $cellResult.childTables += $tableResults[$i].key }
            }
          }
        }
      }
      $paragraphResults = New-Object Collections.Generic.List[object]
      foreach ($paragraph in $paragraphNodes) {
        $text = New-Object Text.StringBuilder
        foreach ($textNode in $paragraph.SelectNodes('./hp:run/hp:t', $ns)) {
          [void]$text.Append((Get-HwpxText $textNode $paragraphNamespace))
        }
        $parentCellNode = $paragraph.SelectSingleNode('ancestor::hp:tc[1]', $ns)
        $parentKey = $null
        if ($null -ne $parentCellNode) {
          if (-not $cellKeys.ContainsKey($parentCellNode)) { throw '지원하지 않는 셀 문단 위치입니다.' }
          $parentKey = $cellKeys[$parentCellNode]
        }
        $paragraphResults.Add([ordered]@{ key = $paragraphKeys[$paragraph]; text = $text.ToString(); cell = $parentKey })
      }
      $sectionResults.Add([ordered]@{ name = $entry.FullName; paragraphs = @($paragraphResults.ToArray()); tables = @($tableResults.ToArray()) })
    }
    return [ordered]@{ schemaVersion = 1; sourceSha256 = $sourceHash; sections = @($sectionResults.ToArray()) }
  } finally {
    if ($null -ne $zip) { $zip.Dispose() }
    $stream.Dispose()
  }
}

function Get-HwpxInteger {
  param([Xml.XmlElement]$Element, [string]$Name, [int]$Minimum)
  $value = $Element.GetAttribute($Name)
  [int]$number = 0
  if ($value -cnotmatch '^\d+$' -or -not [int]::TryParse($value, [ref]$number) -or $number -lt $Minimum) {
    throw ('잘못된 HWPX 숫자 속성: ' + $Name)
  }
  return $number
}

function Get-HwpxText {
  param([Xml.XmlNode]$Node, [string]$Namespace)
  $text = New-Object Text.StringBuilder
  foreach ($child in $Node.ChildNodes) {
    if ($child.NodeType -eq [Xml.XmlNodeType]::Element) {
      if ($child.NamespaceURI -ceq $Namespace -and $child.LocalName -ceq 'lineBreak') { [void]$text.Append("`n") }
      elseif ($child.NamespaceURI -ceq $Namespace -and $child.LocalName -ceq 'tab') { [void]$text.Append("`t") }
      else { [void]$text.Append((Get-HwpxText $child $Namespace)) }
    } elseif ($child.NodeType -in @([Xml.XmlNodeType]::Text, [Xml.XmlNodeType]::CDATA, [Xml.XmlNodeType]::Whitespace, [Xml.XmlNodeType]::SignificantWhitespace)) {
      [void]$text.Append($child.Value)
    }
  }
  return $text.ToString()
}

if ($MyInvocation.InvocationName -ne '.') {
  $ErrorActionPreference = 'Stop'
  try {
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    Get-HwpxStructure -Path $StructureInputPath | ConvertTo-Json -Depth 100
  } catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
  }
}
