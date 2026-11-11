//! Office ZIP readers retain the original distinction between tolerant extraction
//! and strict, byte-preserving editing. No paths are extracted to the filesystem.
use flate2::{Compression, Decompress, FlushDecompress, Status, write::DeflateEncoder};
use std::{collections::HashSet, io::Write};

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const DESCRIPTOR: u32 = 0x0807_4b50;
const MIB: usize = 1024 * 1024;
pub const MAX_ENTRY_SIZE: usize = 64 * MIB;

fn bytes_at(bytes: &[u8], start: usize, length: usize) -> Result<&[u8], String> {
    bytes
        .get(
            start
                ..start
                    .checked_add(length)
                    .ok_or("ZIP 범위를 벗어났습니다.")?,
        )
        .ok_or_else(|| "ZIP 범위를 벗어났습니다.".into())
}
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let b = bytes_at(bytes, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let b = bytes_at(bytes, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn put16(bytes: &mut [u8], at: usize, value: u16) -> Result<(), String> {
    bytes
        .get_mut(at..at + 2)
        .ok_or("ZIP 범위를 벗어났습니다.")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
fn put32(bytes: &mut [u8], at: usize, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "ZIP64 파일은 지원하지 않습니다.")?;
    bytes
        .get_mut(at..at + 4)
        .ok_or("ZIP 범위를 벗어났습니다.")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
fn utf8(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| "ZIP 항목 이름이 올바른 UTF-8이 아닙니다.".into())
}
fn find_end(bytes: &[u8], strict: bool) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    (bytes.len().saturating_sub(65557)..=bytes.len() - 22)
        .rev()
        .find(|i| {
            u32_at(bytes, *i).ok() == Some(END)
                && (!strict
                    || u16_at(bytes, i + 20)
                        .is_ok_and(|length| i + 22 + usize::from(length) == bytes.len()))
        })
}

pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

fn inflate(raw: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut inflater = Decompress::new(false);
    let mut output = Vec::new();
    let mut consumed = 0;
    loop {
        let mut chunk = [0_u8; 8192];
        let before_in = inflater.total_in();
        let before_out = inflater.total_out();
        let status = inflater
            .decompress(&raw[consumed..], &mut chunk, FlushDecompress::None)
            .map_err(|e| format!("ZIP 압축 해제 실패: {e}"))?;
        let read = (inflater.total_in() - before_in) as usize;
        let written = (inflater.total_out() - before_out) as usize;
        consumed += read;
        if output.len() + written > limit {
            return Err("ZIP 확장 크기 한도를 넘었습니다.".into());
        }
        output.extend_from_slice(&chunk[..written]);
        if status == Status::StreamEnd {
            return Ok(output);
        }
        if read == 0 && written == 0 {
            return Err("ZIP 압축 데이터가 완전하지 않습니다.".into());
        }
    }
}

#[derive(Clone, Debug)]
pub struct LooseEntry {
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub packed: u32,
    pub length: u32,
    pub local: usize,
    pub disk_start: u16,
}

#[derive(Debug)]
pub struct LooseArchive<'a> {
    /// Node-compatible first occurrence of each decoded name, in central order.
    pub names: Vec<String>,
    /// Every central record, including duplicates, for .NET-compatible callers.
    pub entries: Vec<LooseEntry>,
    bytes: &'a [u8],
    powershell: bool,
}

impl LooseArchive<'_> {
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| self.read_entry(entry))
            .transpose()
    }

    pub fn read_entry(&self, entry: &LooseEntry) -> Result<Vec<u8>, String> {
        if self.powershell && entry.disk_start != 0 {
            return Err("여러 파일로 분할된 ZIP은 지원하지 않습니다.".into());
        }
        let bad_header = || format!("zip 항목 머리말이 깨졌습니다: {}", entry.name);
        if u32_at(self.bytes, entry.local).map_err(|_| bad_header())? != LOCAL {
            return Err(bad_header());
        }
        let name_len = usize::from(u16_at(self.bytes, entry.local + 26)?);
        let extra_len = usize::from(u16_at(self.bytes, entry.local + 28)?);
        let start = entry.local + 30 + name_len + extra_len;
        // Buffer.subarray clamps truncated stored contents. The tolerant reader
        // intentionally does not validate CRC, flags, length or local names.
        let begin = start.min(self.bytes.len());
        let end = start
            .saturating_add(entry.packed as usize)
            .min(self.bytes.len());
        let raw = &self.bytes[begin..end];
        match entry.method {
            0 => {
                if raw.len() > MAX_ENTRY_SIZE {
                    return Err("ZIP 확장 크기 한도를 넘었습니다.".into());
                }
                Ok(raw.to_vec())
            }
            8 => inflate(raw, MAX_ENTRY_SIZE),
            method => Err(format!(
                "지원하지 않는 zip 압축 방식({method}): {}",
                entry.name
            )),
        }
    }
}

pub fn read_zip(bytes: &[u8]) -> Result<LooseArchive<'_>, String> {
    let end = find_end(bytes, false).ok_or("zip 형식이 아닙니다 (EOCD 를 못 찾음)")?;
    let count = u16_at(bytes, end + 10)?;
    let size = u32_at(bytes, end + 12)?;
    let offset = u32_at(bytes, end + 16)?;
    if offset == u32::MAX || count == u16::MAX || size == u32::MAX {
        return Err("zip64 파일은 지원하지 않습니다".into());
    }
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut p = offset as usize;
    for _ in 0..count {
        if bytes_at(bytes, p, 46).is_err() || u32_at(bytes, p)? != CENTRAL {
            return Err("zip 중앙 디렉터리가 깨졌습니다".into());
        }
        let nl = usize::from(u16_at(bytes, p + 28)?);
        let el = usize::from(u16_at(bytes, p + 30)?);
        let cl = usize::from(u16_at(bytes, p + 32)?);
        let name_bytes = bytes[p + 46..(p + 46 + nl).min(bytes.len())].to_vec();
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        if seen.insert(name.clone()) {
            names.push(name.clone());
        }
        entries.push(LooseEntry {
            name,
            name_bytes,
            flags: u16_at(bytes, p + 8)?,
            method: u16_at(bytes, p + 10)?,
            packed: u32_at(bytes, p + 20)?,
            length: u32_at(bytes, p + 24)?,
            local: u32_at(bytes, p + 42)? as usize,
            disk_start: u16_at(bytes, p + 34)?,
        });
        p += 46 + nl + el + cl;
    }
    Ok(LooseArchive {
        names,
        entries,
        bytes,
        powershell: false,
    })
}

/// Preserve ZipArchive's split-volume and entry-count checks separately from
/// Node's tolerant readZip. Flags alone are not proof of encrypted contents.
pub fn read_zip_powershell(bytes: &[u8]) -> Result<LooseArchive<'_>, String> {
    let end = find_end(bytes, false).ok_or("zip 형식이 아닙니다 (EOCD 를 못 찾음)")?;
    if u16_at(bytes, end + 4)? != 0 || u16_at(bytes, end + 6)? != 0 {
        return Err("여러 파일로 분할된 ZIP은 지원하지 않습니다.".into());
    }
    if u16_at(bytes, end + 8)? != u16_at(bytes, end + 10)? {
        return Err("ZIP 항목 개수 정보가 다릅니다.".into());
    }
    let mut archive = read_zip(bytes)?;
    // ZipArchive validates an entry's disk only when that entry is opened.
    // Unused attachments must stay lazy even when their metadata is damaged.
    archive.powershell = true;
    Ok(archive)
}

pub fn read_zip_text(zip: &LooseArchive<'_>, name: &str) -> Result<Option<String>, String> {
    zip.read(name).map(|data| {
        data.map(|bytes| {
            let text = String::from_utf8_lossy(&bytes);
            text.strip_prefix('\u{feff}').unwrap_or(&text).to_string()
        })
    })
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub crc: u32,
    pub packed: u32,
    pub local: usize,
    pub start: usize,
    pub central: Vec<u8>,
    pub record: Vec<u8>,
    pub header: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Archive {
    /// Central-directory order, which can differ from physical record order.
    pub entries: Vec<Entry>,
    ordered: Vec<usize>,
    end: Vec<u8>,
}

impl Archive {
    pub fn read(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.data.as_slice())
    }
}

fn validate_extra(extra: &[u8]) -> Result<(), String> {
    let mut p = 0;
    while p < extra.len() {
        if p + 4 > extra.len() {
            return Err("잘못된 ZIP 확장 정보 또는 ZIP64입니다.".into());
        }
        let size = usize::from(u16_at(extra, p + 2)?);
        if p + 4 + size > extra.len() || u16_at(extra, p)? == 1 {
            return Err("잘못된 ZIP 확장 정보 또는 ZIP64입니다.".into());
        }
        p += 4 + size;
    }
    Ok(())
}

pub fn read_zip_strict(bytes: &[u8]) -> Result<Archive, String> {
    if bytes.len() > 128 * MIB || bytes.len() < 22 || u32_at(bytes, 0)? != LOCAL {
        return Err("지원하는 HWPX ZIP 형식이 아닙니다. HWP는 한글 변환이 필요합니다.".into());
    }
    let end = find_end(bytes, true).ok_or("ZIP 종료 정보가 없습니다.")?;
    let count = u16_at(bytes, end + 10)?;
    let size = u32_at(bytes, end + 12)?;
    let offset = u32_at(bytes, end + 16)?;
    if u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || u16_at(bytes, end + 8)? != count
        || count == u16::MAX
        || size == u32::MAX
        || offset == u32::MAX
        || offset as usize + size as usize != end
        || count > 10000
    {
        return Err("분할 ZIP·ZIP64 또는 잘못된 ZIP입니다.".into());
    }
    let offset = offset as usize;
    let mut p = offset;
    let mut total = 0_usize;
    let mut names = HashSet::new();
    let mut entries = Vec::new();
    for _ in 0..count {
        if p + 46 > end || u32_at(bytes, p)? != CENTRAL {
            return Err("잘못된 ZIP 중앙 정보입니다.".into());
        }
        let flags = u16_at(bytes, p + 8)?;
        let method = u16_at(bytes, p + 10)?;
        let crc = u32_at(bytes, p + 16)?;
        let packed = u32_at(bytes, p + 20)?;
        let length = u32_at(bytes, p + 24)?;
        let nl = usize::from(u16_at(bytes, p + 28)?);
        let el = usize::from(u16_at(bytes, p + 30)?);
        let cl = usize::from(u16_at(bytes, p + 32)?);
        let local32 = u32_at(bytes, p + 42)?;
        let local = local32 as usize;
        let central_end = p + 46 + nl + el + cl;
        if central_end > end
            || nl == 0
            || local + 30 > offset
            || packed == u32::MAX
            || length == u32::MAX
            || local32 == u32::MAX
            || u16_at(bytes, p + 34)? != 0
        {
            return Err("ZIP 범위를 벗어났습니다.".into());
        }
        let name = utf8(bytes_at(bytes, p + 46, nl)?)?;
        if !names.insert(name.clone())
            || name.contains('\\')
            || name.starts_with('/')
            || name.split('/').any(|s| s == ".." || s == ".")
            || name.chars().any(|c| c <= '\u{1f}')
        {
            return Err("중복되거나 잘못된 ZIP 항목 이름입니다.".into());
        }
        if flags & !0x80e != 0 || ![0, 8].contains(&method) || (method == 0 && flags & 6 != 0) {
            return Err("암호화 또는 지원하지 않는 ZIP 방식입니다.".into());
        }
        if u32_at(bytes, local)? != LOCAL
            || u16_at(bytes, local + 6)? != flags
            || u16_at(bytes, local + 8)? != method
        {
            return Err("ZIP 로컬 정보가 다릅니다.".into());
        }
        let lnl = usize::from(u16_at(bytes, local + 26)?);
        let lel = usize::from(u16_at(bytes, local + 28)?);
        let start = local + 30 + lnl + lel;
        if start + packed as usize > offset || utf8(bytes_at(bytes, local + 30, lnl)?)? != name {
            return Err("ZIP 로컬 항목 범위가 다릅니다.".into());
        }
        if flags & 8 == 0
            && (u32_at(bytes, local + 14)? != crc
                || u32_at(bytes, local + 18)? != packed
                || u32_at(bytes, local + 22)? != length)
        {
            return Err("ZIP 검사값이 다릅니다.".into());
        }
        validate_extra(bytes_at(bytes, p + 46 + nl, el)?)?;
        validate_extra(bytes_at(bytes, local + 30 + lnl, lel)?)?;
        total += length as usize;
        if length as usize > MAX_ENTRY_SIZE || total > 256 * MIB {
            return Err("ZIP 확장 크기 한도를 넘었습니다.".into());
        }
        let raw = bytes_at(bytes, start, packed as usize)?;
        let data = if method == 0 {
            raw.to_vec()
        } else {
            inflate(raw, (length as usize).max(1))?
        };
        if data.len() != length as usize || crc32(&data) != crc {
            return Err("ZIP 내용 검사값이 다릅니다.".into());
        }
        entries.push(Entry {
            name,
            data,
            flags,
            method,
            crc,
            packed,
            local,
            start,
            central: bytes[p..central_end].to_vec(),
            record: vec![],
            header: vec![],
        });
        p = central_end;
    }
    if p != end {
        return Err("남는 ZIP 중앙 정보가 있습니다.".into());
    }
    let mut ordered: Vec<usize> = (0..entries.len()).collect();
    ordered.sort_by_key(|i| entries[*i].local);
    for (i, index) in ordered.iter().enumerate() {
        let next = ordered.get(i + 1).map_or(offset, |n| entries[*n].local);
        let entry = &mut entries[*index];
        let raw_end = entry.start + entry.packed as usize;
        if (i == 0 && entry.local != 0) || raw_end > next {
            return Err("ZIP 항목이 겹칩니다.".into());
        }
        let descriptor = next - raw_end;
        if entry.flags & 8 == 0 && descriptor != 0 {
            return Err("지원하지 않는 ZIP 항목 간 데이터입니다.".into());
        }
        if entry.flags & 8 != 0 {
            if ![12, 16].contains(&descriptor) {
                return Err("잘못된 ZIP 데이터 설명자입니다.".into());
            }
            let mut d = raw_end;
            if descriptor == 16 {
                if u32_at(bytes, d)? != DESCRIPTOR {
                    return Err("ZIP 설명자 서명이 다릅니다.".into());
                }
                d += 4;
            }
            if u32_at(bytes, d)? != entry.crc
                || u32_at(bytes, d + 4)? != entry.packed
                || u32_at(bytes, d + 8)? as usize != entry.data.len()
            {
                return Err("ZIP 설명자 값이 다릅니다.".into());
            }
        }
        entry.record = bytes[entry.local..next].to_vec();
        entry.header = bytes[entry.local..entry.start].to_vec();
    }
    Ok(Archive {
        entries,
        ordered,
        end: bytes[end..].to_vec(),
    })
}

pub fn patch_zip(archive: &Archive, replacements: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if replacements
        .iter()
        .any(|(name, _)| !archive.entries.iter().any(|e| &e.name == name))
    {
        return Err("없는 ZIP 항목은 추가하지 않습니다.".into());
    }
    let mut result = Vec::new();
    let mut central_records = vec![vec![]; archive.entries.len()];
    for index in &archive.ordered {
        let entry = archive
            .entries
            .get(*index)
            .ok_or("ZIP 항목 순서가 다릅니다.")?;
        let mut central = entry.central.clone();
        put32(&mut central, 42, result.len())?;
        if let Some((_, data)) = replacements
            .iter()
            .rev()
            .find(|(name, _)| name == &entry.name)
        {
            let raw = if entry.method == 0 {
                data.clone()
            } else {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
                encoder
                    .write_all(data)
                    .map_err(|e| format!("ZIP 압축 실패: {e}"))?;
                encoder
                    .finish()
                    .map_err(|e| format!("ZIP 압축 실패: {e}"))?
            };
            let crc = crc32(data);
            let flags = entry.flags & !8;
            let mut header = entry.header.clone();
            put16(&mut header, 6, flags)?;
            put32(&mut header, 14, crc as usize)?;
            put32(&mut header, 18, raw.len())?;
            put32(&mut header, 22, data.len())?;
            put16(&mut central, 8, flags)?;
            put32(&mut central, 16, crc as usize)?;
            put32(&mut central, 20, raw.len())?;
            put32(&mut central, 24, data.len())?;
            result.extend_from_slice(&header);
            result.extend_from_slice(&raw);
        } else {
            result.extend_from_slice(&entry.record);
        }
        central_records[*index] = central;
    }
    let offset = result.len();
    for central in central_records {
        result.extend_from_slice(&central);
    }
    let mut end = archive.end.clone();
    put32(&mut end, 12, result.len() - offset)?;
    put32(&mut end, 16, offset)?;
    result.extend_from_slice(&end);
    Ok(result)
}

pub fn create_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if entries.is_empty() || entries.len() > 10000 {
        return Err("잘못된 새 ZIP 항목입니다.".into());
    }
    let mut records = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let filename = name.as_bytes();
        if filename.len() > 65535 || data.len() > MAX_ENTRY_SIZE {
            return Err("ZIP 항목 크기 한도 초과".into());
        }
        let crc = crc32(data);
        let mut local = vec![0_u8; 30];
        let mut central = vec![0_u8; 46];
        put32(&mut local, 0, LOCAL as usize)?;
        put16(&mut local, 4, 20)?;
        put16(&mut local, 6, 0x800)?;
        put16(&mut local, 12, 33)?;
        put32(&mut local, 14, crc as usize)?;
        put32(&mut local, 18, data.len())?;
        put32(&mut local, 22, data.len())?;
        put16(&mut local, 26, filename.len() as u16)?;
        put32(&mut central, 0, CENTRAL as usize)?;
        put16(&mut central, 4, 20)?;
        put16(&mut central, 6, 20)?;
        put16(&mut central, 8, 0x800)?;
        put16(&mut central, 14, 33)?;
        put32(&mut central, 16, crc as usize)?;
        put32(&mut central, 20, data.len())?;
        put32(&mut central, 24, data.len())?;
        put16(&mut central, 28, filename.len() as u16)?;
        put32(&mut central, 42, records.len())?;
        records.extend_from_slice(&local);
        records.extend_from_slice(filename);
        records.extend_from_slice(data);
        directory.extend_from_slice(&central);
        directory.extend_from_slice(filename);
        if records.len() > 120 * MIB {
            return Err("새 ZIP 전체 크기 한도 초과".into());
        }
    }
    let mut end = vec![0_u8; 22];
    put32(&mut end, 0, END as usize)?;
    put16(&mut end, 8, entries.len() as u16)?;
    put16(&mut end, 10, entries.len() as u16)?;
    put32(&mut end, 12, directory.len())?;
    put32(&mut end, 16, records.len())?;
    records.extend_from_slice(&directory);
    records.extend_from_slice(&end);
    read_zip_strict(&records)?;
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        create_zip(&[
            ("mimetype".into(), b"application/hwp+zip".to_vec()),
            (
                "Contents/section0.xml".into(),
                "\u{feff}<p>가상 글자</p>".as_bytes().to_vec(),
            ),
        ])
        .unwrap()
    }
    fn set32(bytes: &mut [u8], at: usize, value: usize) {
        put32(bytes, at, value).unwrap();
    }
    fn set16(bytes: &mut [u8], at: usize, value: u16) {
        put16(bytes, at, value).unwrap();
    }

    /// Independent synthetic builder with stored/deflated content, extras,
    /// comments and optional signed/unsigned data descriptors.
    fn rich_zip(descriptor_size: usize) -> Vec<u8> {
        let mut records = Vec::new();
        let mut centrals = Vec::new();
        for (name, data, method) in [
            ("first.xml", b"first text".as_slice(), 8_u16),
            ("second.bin", b"binary\0sample".as_slice(), 0),
        ] {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data).unwrap();
            let raw = if method == 8 {
                encoder.finish().unwrap()
            } else {
                data.to_vec()
            };
            let flags = 0x800 | if descriptor_size > 0 { 8 } else { 0 };
            let local_offset = records.len();
            let extra = [0xfe, 0xca, 2, 0, 0xab, 0xcd];
            let mut local = vec![0_u8; 30];
            set32(&mut local, 0, LOCAL as usize);
            set16(&mut local, 4, 20);
            set16(&mut local, 6, flags);
            set16(&mut local, 8, method);
            set16(&mut local, 10, 0x7788);
            set16(&mut local, 12, 0x5533);
            if descriptor_size == 0 {
                set32(&mut local, 14, crc32(data) as usize);
                set32(&mut local, 18, raw.len());
                set32(&mut local, 22, data.len());
            }
            set16(&mut local, 26, name.len() as u16);
            set16(&mut local, 28, extra.len() as u16);
            records.extend(local);
            records.extend(name.as_bytes());
            records.extend(extra);
            records.extend(&raw);
            if descriptor_size > 0 {
                if descriptor_size == 16 {
                    records.extend(DESCRIPTOR.to_le_bytes());
                }
                records.extend(crc32(data).to_le_bytes());
                records.extend((raw.len() as u32).to_le_bytes());
                records.extend((data.len() as u32).to_le_bytes());
            }
            let mut central = vec![0_u8; 46];
            set32(&mut central, 0, CENTRAL as usize);
            set16(&mut central, 4, 0x314);
            set16(&mut central, 6, 20);
            set16(&mut central, 8, flags);
            set16(&mut central, 10, method);
            set16(&mut central, 12, 0x7788);
            set16(&mut central, 14, 0x5533);
            set32(&mut central, 16, crc32(data) as usize);
            set32(&mut central, 20, raw.len());
            set32(&mut central, 24, data.len());
            set16(&mut central, 28, name.len() as u16);
            set16(&mut central, 30, extra.len() as u16);
            set16(&mut central, 32, 3);
            set16(&mut central, 36, 1);
            set32(&mut central, 38, 0x20);
            set32(&mut central, 42, local_offset);
            central.extend(name.as_bytes());
            central.extend(extra);
            central.extend(b"tag");
            centrals.push(central);
        }
        let offset = records.len();
        // Central-directory order intentionally differs from local order.
        for central in centrals.into_iter().rev() {
            records.extend(central);
        }
        let mut end = vec![0_u8; 22];
        set32(&mut end, 0, END as usize);
        set16(&mut end, 8, 2);
        set16(&mut end, 10, 2);
        set32(&mut end, 12, records.len() - offset);
        set32(&mut end, 16, offset);
        set16(&mut end, 20, 7);
        records.extend(end);
        records.extend(b"comment");
        records
    }

    #[test]
    fn crc_and_created_archive_round_trip() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        let bytes = sample();
        let zip = read_zip_strict(&bytes).unwrap();
        assert_eq!(zip.entries[0].name, "mimetype");
        assert_eq!(
            zip.read("mimetype"),
            Some(b"application/hwp+zip".as_slice())
        );
        assert!(zip.read("missing").is_none());
        assert_eq!(patch_zip(&zip, &[]).unwrap(), bytes);
        let loose = read_zip(&bytes).unwrap();
        assert_eq!(
            read_zip_text(&loose, "Contents/section0.xml").unwrap(),
            Some("<p>가상 글자</p>".into())
        );
        assert!(read_zip_text(&loose, "missing").unwrap().is_none());
    }

    #[test]
    fn patch_preserves_untouched_records_and_metadata() {
        for descriptor in [0, 12, 16] {
            let bytes = rich_zip(descriptor);
            let archive = read_zip_strict(&bytes).unwrap();
            assert_eq!(archive.entries[0].name, "second.bin");
            assert_eq!(patch_zip(&archive, &[]).unwrap(), bytes);
            let result = patch_zip(
                &archive,
                &[("first.xml".into(), "교체한 가상 내용".as_bytes().to_vec())],
            )
            .unwrap();
            let changed = read_zip_strict(&result).unwrap();
            assert_eq!(changed.entries[0].record, archive.entries[0].record);
            assert_eq!(changed.entries[0].data, archive.entries[0].data);
            assert_eq!(changed.end[20..], archive.end[20..]);
            assert_eq!(changed.entries[1].flags & 8, 0);
            assert_eq!(
                changed.entries[1].header[26..],
                archive.entries[1].header[26..]
            );
            assert_eq!(
                changed.read("first.xml"),
                Some("교체한 가상 내용".as_bytes())
            );
            assert!(patch_zip(&archive, &[("absent".into(), vec![])]).is_err());
        }
    }

    #[test]
    fn loose_reader_keeps_first_duplicate_and_is_lazy() {
        let mut bytes = create_zip(&[
            ("a.xml".into(), b"first".to_vec()),
            ("b.xml".into(), b"second".to_vec()),
        ])
        .unwrap();
        let end = find_end(&bytes, true).unwrap();
        let central = u32_at(&bytes, end + 16).unwrap() as usize;
        let second = central + 46 + 5;
        bytes[second + 46] = b'a';
        let loose = read_zip(&bytes).unwrap();
        assert_eq!(loose.names, ["a.xml"]);
        assert_eq!(loose.entries.len(), 2);
        assert_eq!(loose.read("a.xml").unwrap().unwrap(), b"first");
        assert_eq!(loose.read_entry(&loose.entries[1]).unwrap(), b"second");
        assert!(read_zip_strict(&bytes).is_err());
        set16(&mut bytes, second + 10, 99);
        let loose = read_zip(&bytes).unwrap();
        assert_eq!(loose.read("a.xml").unwrap().unwrap(), b"first");
        assert!(
            loose
                .read_entry(&loose.entries[1])
                .unwrap_err()
                .contains("99")
        );
    }

    #[test]
    fn powershell_checks_volume_and_count_without_changing_node_or_flags() {
        let bytes = sample();
        let end = find_end(&bytes, false).unwrap();
        let central = u32_at(&bytes, end + 16).unwrap() as usize;
        for (position, value) in [(end + 4, 1), (end + 6, 1), (end + 8, 0), (central + 34, 1)] {
            let mut changed = bytes.clone();
            set16(&mut changed, position, value);
            assert!(
                read_zip_powershell(&changed)
                    .and_then(|zip| zip.read("mimetype"))
                    .is_err(),
                "position {position}"
            );
            assert_eq!(
                read_zip(&changed)
                    .unwrap()
                    .read("mimetype")
                    .unwrap()
                    .unwrap(),
                b"application/hwp+zip"
            );
        }
        for flag in [1, 64] {
            let mut changed = bytes.clone();
            set16(&mut changed, 6, 0x800 | flag);
            set16(&mut changed, central + 8, 0x800 | flag);
            assert_eq!(
                read_zip_powershell(&changed)
                    .unwrap()
                    .read("mimetype")
                    .unwrap()
                    .unwrap(),
                b"application/hwp+zip"
            );
        }
        let mut unused = bytes.clone();
        let second_central = central + 46 + "mimetype".len();
        set16(&mut unused, second_central + 34, 1);
        let zip = read_zip_powershell(&unused).unwrap();
        assert_eq!(
            zip.read("mimetype").unwrap().unwrap(),
            b"application/hwp+zip"
        );
        assert!(zip.read("Contents/section0.xml").is_err());
    }

    #[test]
    fn strict_rejects_crc_flags_names_and_zip64() {
        let bytes = sample();
        let end = find_end(&bytes, true).unwrap();
        let central = u32_at(&bytes, end + 16).unwrap() as usize;
        let archive = read_zip_strict(&bytes).unwrap();
        let mut changed = bytes.clone();
        changed[archive.entries[0].start] ^= 1;
        assert!(
            read_zip_strict(&changed)
                .unwrap_err()
                .contains("내용 검사값")
        );
        assert!(read_zip(&changed).unwrap().read("mimetype").is_ok());
        for (at, value) in [
            (central + 8, 1),
            (central + 10, 99),
            (end + 4, 1),
            (end + 8, 0),
        ] {
            let mut changed = bytes.clone();
            set16(&mut changed, at, value);
            assert!(read_zip_strict(&changed).is_err());
        }
        for at in [end + 12, end + 16, central + 20, central + 24, central + 42] {
            let mut changed = bytes.clone();
            set32(&mut changed, at, u32::MAX as usize);
            assert!(read_zip_strict(&changed).is_err());
        }
        for name in ["", "../x", "x/../y", "./x", "/x", "x\\y", "x\0y", "x\ny"] {
            assert!(create_zip(&[(name.into(), vec![])]).is_err());
        }
        assert!(create_zip(&[]).is_err());
        assert!(create_zip(&[("a".into(), vec![]), ("a".into(), vec![])]).is_err());
    }

    #[test]
    fn strict_checks_extras_descriptors_and_boundaries() {
        let bytes = rich_zip(16);
        let archive = read_zip_strict(&bytes).unwrap();
        let first = &archive.entries[1];
        for at in [
            first.start + first.packed as usize,
            first.start + first.packed as usize + 4,
            first.start + first.packed as usize + 8,
        ] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert!(read_zip_strict(&changed).is_err());
        }
        let mut zip64 = bytes.clone();
        set16(&mut zip64, first.local + 30 + first.name.len(), 1);
        assert!(read_zip_strict(&zip64).unwrap_err().contains("ZIP64"));
        let mut invalid_extra = bytes.clone();
        set16(
            &mut invalid_extra,
            first.local + 30 + first.name.len() + 2,
            30,
        );
        assert!(
            read_zip_strict(&invalid_extra)
                .unwrap_err()
                .contains("확장 정보")
        );
        let mut wrong_local = bytes.clone();
        wrong_local[first.local + 30] ^= 1;
        assert!(read_zip_strict(&wrong_local).is_err());
        let mut trailing = bytes.clone();
        trailing.push(1);
        assert!(read_zip_strict(&trailing).is_err());
        assert!(read_zip(&trailing).is_ok());
    }

    #[test]
    fn bounded_inflate_rejects_bombs_and_truncated_streams() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&[0; 4096]).unwrap();
        let raw = encoder.finish().unwrap();
        assert!(inflate(&raw, 4095).unwrap_err().contains("한도"));
        assert_eq!(inflate(&raw, 4096).unwrap().len(), 4096);
        for cut in 0..raw.len() {
            assert!(inflate(&raw[..cut], 4096).is_err(), "cut {cut}");
        }
        let mut trailing = raw.clone();
        trailing.extend(b"extra");
        assert_eq!(inflate(&trailing, 4096).unwrap().len(), 4096);
    }

    #[test]
    fn tolerant_reader_limits_only_the_requested_entry() {
        // Compress incrementally so generating the fixture needs no 64 MiB input.
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        for _ in 0..MAX_ENTRY_SIZE / 8192 {
            encoder.write_all(&[0; 8192]).unwrap();
        }
        encoder.write_all(&[0]).unwrap();
        let raw = encoder.finish().unwrap();
        let mut bytes = create_zip(&[("x".into(), vec![])]).unwrap();
        let original_central = 31;
        bytes.splice(original_central..original_central, raw.iter().copied());
        let central = original_central + raw.len();
        let end = bytes.len() - 22;
        set16(&mut bytes, 8, 8);
        set16(&mut bytes, central + 10, 8);
        set32(&mut bytes, 18, raw.len());
        set32(&mut bytes, central + 20, raw.len());
        set32(&mut bytes, end + 16, central);
        let archive = read_zip(&bytes).unwrap();
        assert_eq!(archive.names, ["x"]);
        assert!(archive.read("missing").unwrap().is_none());
        assert!(archive.read("x").unwrap_err().contains("한도"));
    }

    #[test]
    fn strict_rejects_overlapping_records_and_inter_record_padding() {
        let inner = create_zip(&[("b".into(), b"inside".to_vec())]).unwrap();
        let inner_archive = read_zip_strict(&inner).unwrap();
        let inner_entry = &inner_archive.entries[0];
        // An outer stored entry contains a complete, CRC-valid local record.
        let outer = create_zip(&[("a".into(), inner_entry.record.clone())]).unwrap();
        let outer_archive = read_zip_strict(&outer).unwrap();
        let central_start = outer_archive.entries[0].record.len();
        let mut overlap = outer[..outer.len() - 22].to_vec();
        let mut nested_central = inner_entry.central.clone();
        set32(&mut nested_central, 42, 31);
        overlap.extend(&nested_central);
        let mut end = outer[outer.len() - 22..].to_vec();
        set16(&mut end, 8, 2);
        set16(&mut end, 10, 2);
        set32(&mut end, 12, overlap.len() - central_start);
        overlap.extend(end);
        assert!(read_zip_strict(&overlap).unwrap_err().contains("겹칩니다"));

        let mut gap = sample();
        let archive = read_zip_strict(&gap).unwrap();
        let insert = archive.entries[1].local;
        let original_end = find_end(&gap, true).unwrap();
        let original_central = u32_at(&gap, original_end + 16).unwrap() as usize;
        gap.insert(insert, 0);
        let central = original_central + 1;
        let second = central + archive.entries[0].central.len();
        let end = original_end + 1;
        set32(&mut gap, second + 42, insert + 1);
        set32(&mut gap, end + 16, central);
        assert!(
            read_zip_strict(&gap)
                .unwrap_err()
                .contains("항목 간 데이터")
        );
    }

    #[test]
    fn strict_limits_and_utf8_differ_from_tolerant_text() {
        let mut bytes = create_zip(&[("x".into(), vec![0xef, 0xbb, 0xbf, 0xff, b'x'])]).unwrap();
        assert_eq!(
            read_zip_text(&read_zip(&bytes).unwrap(), "x").unwrap(),
            Some("\u{fffd}x".into())
        );
        let end = bytes.len() - 22;
        let central = u32_at(&bytes, end + 16).unwrap() as usize;
        set32(&mut bytes, 22, MAX_ENTRY_SIZE + 1);
        set32(&mut bytes, central + 24, MAX_ENTRY_SIZE + 1);
        assert!(read_zip_strict(&bytes).unwrap_err().contains("한도"));
        // Tolerant reads intentionally ignore the declared uncompressed length.
        assert_eq!(
            read_zip(&bytes).unwrap().read("x").unwrap().unwrap().len(),
            5
        );
        let mut bad_name = create_zip(&[("x".into(), vec![])]).unwrap();
        let end = bad_name.len() - 22;
        let central = u32_at(&bad_name, end + 16).unwrap() as usize;
        bad_name[30] = 0xff;
        bad_name[central + 46] = 0xff;
        assert!(read_zip_strict(&bad_name).unwrap_err().contains("UTF-8"));
        assert_eq!(read_zip(&bad_name).unwrap().names, ["\u{fffd}"]);
    }

    #[test]
    fn damaged_inputs_return_errors_without_panicking() {
        let bytes = rich_zip(12);
        for end in 0..bytes.len() {
            assert!(read_zip_strict(&bytes[..end]).is_err());
            if let Ok(zip) = read_zip(&bytes[..end]) {
                for entry in &zip.entries {
                    let _ = zip.read_entry(entry);
                }
            }
        }
        let mut seed = 0x1234_5678_u32;
        for size in 0..512 {
            let mut data = vec![0_u8; size];
            for b in &mut data {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *b = seed as u8;
            }
            let _ = read_zip_strict(&data);
            if let Ok(zip) = read_zip(&data) {
                for entry in &zip.entries {
                    let _ = zip.read_entry(entry);
                }
            }
        }
    }
}
