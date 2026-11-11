//! The two legacy preview fonts are read only; repair applies to an owned copy.
use data_encoding::BASE64;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, io::Read, path::Path};

const FONT_LIMIT: u64 = 10 * 1024 * 1024;
const FONTS: [(&str, &str); 2] = [
    ("HYHeadLine-Medium", "H2HDRM.TTF"),
    ("HYSinMyeongJo-Medium", "H2MJSM.TTF"),
];

// Match Buffer.readUIntBE/writeUIntBE's range failures without unchecked slices.
fn at(bytes: &[u8], offset: u64, width: usize) -> Result<usize, String> {
    let Some(last) = bytes.len().checked_sub(width) else {
        return Err("Attempt to access memory outside buffer bounds".into());
    };
    if offset > last as u64 {
        // Node's ERR_OUT_OF_RANGE formats values above 2**32 with separators.
        let received = if offset > 0x1_0000_0000 {
            let digits = offset.to_string();
            digits
                .chars()
                .enumerate()
                .flat_map(|(index, digit)| {
                    let separator = index > 0 && (digits.len() - index).is_multiple_of(3);
                    separator
                        .then_some('_')
                        .into_iter()
                        .chain(std::iter::once(digit))
                })
                .collect::<String>()
        } else {
            offset.to_string()
        };
        return Err(format!(
            "The value of \"offset\" is out of range. It must be >= 0 and <= {last}. Received {received}"
        ));
    }
    Ok(offset as usize)
}
fn u16be(bytes: &[u8], offset: u64) -> Result<u16, String> {
    let at = at(bytes, offset, 2)?;
    Ok(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
}
fn u32be(bytes: &[u8], offset: u64) -> Result<u32, String> {
    let at = at(bytes, offset, 4)?;
    Ok(u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()))
}
fn put16(bytes: &mut [u8], offset: u64, value: u16) -> Result<(), String> {
    let at = at(bytes, offset, 2)?;
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    Ok(())
}
fn put32(bytes: &mut [u8], offset: u64, value: u32) -> Result<(), String> {
    let at = at(bytes, offset, 4)?;
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    Ok(())
}
fn tag(bytes: &[u8], offset: usize) -> String {
    // Buffer.toString('ascii') clears bit 7, unlike UTF-8/Latin-1 decoding.
    bytes[offset..offset + 4]
        .iter()
        .map(|byte| char::from(byte & 0x7f))
        .collect()
}

pub fn preview_embedding_allowed(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || u32be(bytes, 0).ok() != Some(0x0001_0000) {
        return false;
    }
    let count = u16be(bytes, 4).unwrap() as usize;
    if 12 + count * 16 > bytes.len() {
        return false;
    }
    for index in 0..count {
        let table = 12 + index * 16;
        if tag(bytes, table) != "OS/2" {
            continue;
        }
        let offset = u64::from(u32be(bytes, (table + 8) as u64).unwrap());
        let size = u64::from(u32be(bytes, (table + 12) as u64).unwrap());
        if size < 10 || offset + size > bytes.len() as u64 {
            return false;
        }
        let flags = u16be(bytes, offset + 8).unwrap();
        return flags & 0x0002 == 0 && flags & 0x0200 == 0;
    }
    false
}

#[derive(Clone)]
struct Table {
    at: u64,
    offset: u64,
    length: u64,
}

fn checksum(bytes: &[u8], offset: u64, length: u64) -> u32 {
    // The legacy unchecked checksum turns an out-of-buffer byte into NaN,
    // then >>>0 into zero. Every following word is also outside the buffer.
    // Compute that result directly instead of iterating a malicious u32 length.
    if length == 0 || offset + length > bytes.len() as u64 {
        return 0;
    }
    let mut sum = 0u32;
    for chunk in bytes[offset as usize..(offset + length) as usize].chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn bounded_checksum(bytes: &[u8], offset: u64, length: u64, work: &mut u64) -> Result<u32, String> {
    // Only in-buffer words take work; a missing tail returns zero immediately.
    // Repeated/overlapping forged tables must not multiply CPU use without bound.
    if offset + length <= bytes.len() as u64 {
        *work += length.div_ceil(4);
        if *work > 16_000_000 {
            return Err("글꼴 미리보기 보정 처리 한도를 넘었습니다.".into());
        }
    }
    Ok(checksum(bytes, offset, length))
}

/// Preserve the legacy cmap format-4 repair and checksum order. Neither the
/// input bytes nor any installed font is modified. Malformed offsets are errors.
pub fn prepare_preview_font(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = input.to_vec();
    let count = u16be(&bytes, 4)?;
    let mut tables: Vec<Table> = Vec::new();
    let mut table_indices = HashMap::<String, usize>::new();
    for index in 0..u64::from(count) {
        let table_at = 12 + index * 16;
        // In JS the tag read clamps, but the following offset read still fails
        // on a truncated entry. Read in that same observable failure order.
        let offset = u64::from(u32be(&bytes, table_at + 8)?);
        let length = u64::from(u32be(&bytes, table_at + 12)?);
        let name = tag(&bytes, table_at as usize);
        let table = Table {
            at: table_at,
            offset,
            length,
        };
        if let Some(&position) = table_indices.get(&name) {
            tables[position] = table; // Map.set retains the first insertion position.
        } else {
            table_indices.insert(name, tables.len());
            tables.push(table);
        }
    }
    let find = |name: &str| {
        table_indices
            .get(name)
            .map(|&position| tables[position].clone())
    };
    let (Some(cmap), Some(maxp)) = (find("cmap"), find("maxp")) else {
        return Ok(bytes);
    };
    let glyphs = u16be(&bytes, maxp.offset + 4)?;
    let mut changed = false;
    let mut index = 0u64;
    let mut work = 0u64;
    while index < u64::from(u16be(&bytes, cmap.offset + 2)?) {
        let start = cmap.offset + u64::from(u32be(&bytes, cmap.offset + 8 + index * 8)?);
        index += 1;
        if u16be(&bytes, start)? != 4 {
            continue;
        }
        let length = u64::from(u16be(&bytes, start + 2)?);
        let doubled_segments = u64::from(u16be(&bytes, start + 6)?);
        // Odd segCountX2 is retained exactly: JS j < x/2 visits ceil(x/2)
        // entries, while all address expressions multiply x/2 by even numbers.
        for segment in 0..doubled_segments.div_ceil(2) {
            work += 1;
            if work > 16_000_000 {
                return Err("글꼴 미리보기 보정 처리 한도를 넘었습니다.".into());
            }
            let end = u16be(&bytes, start + 14 + segment * 2)?;
            let begin = u16be(&bytes, start + 16 + doubled_segments + segment * 2)?;
            let delta = start + 16 + doubled_segments * 2 + segment * 2;
            let range = u16be(&bytes, start + 16 + doubled_segments * 3 + segment * 2)?;
            if begin == 65535 && end == 65535 && range == 0 && u16be(&bytes, delta)? == 0 {
                put16(&mut bytes, delta, 1)?;
                changed = true;
            }
        }
        let mut glyph_at = start + 16 + doubled_segments * 4;
        while glyph_at < start + length {
            work += 1;
            if work > 16_000_000 {
                return Err("글꼴 미리보기 보정 처리 한도를 넘었습니다.".into());
            }
            if u16be(&bytes, glyph_at)? >= glyphs {
                put16(&mut bytes, glyph_at, 0)?;
                changed = true;
            }
            glyph_at += 2;
        }
    }
    if changed {
        let head = find("head");
        if let Some(head) = &head {
            put32(&mut bytes, head.offset + 8, 0)?;
        }
        for table in &tables {
            let sum = bounded_checksum(&bytes, table.offset, table.length, &mut work)?;
            put32(&mut bytes, table.at + 4, sum)?;
        }
        if let Some(head) = &head {
            let adjustment = 0xB1B0_AFBAu32.wrapping_sub(bounded_checksum(
                &bytes,
                0,
                bytes.len() as u64,
                &mut work,
            )?);
            put32(&mut bytes, head.offset + 8, adjustment)?;
        }
    }
    Ok(bytes)
}

/// Read only the two fixed font filenames requested by the SVG. SystemRoot is
/// the existing Windows environment value, not an SVG-supplied path.
pub fn preview_fonts(svg: &str) -> Result<Value, String> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:/Windows".into());
    preview_fonts_at(svg, Path::new(&root))
}
fn preview_fonts_at(svg: &str, system_root: &Path) -> Result<Value, String> {
    let mut css = String::new();
    let mut fonts = Vec::new();
    let mut warnings = Vec::new();
    for (family, file) in FONTS {
        if !svg.contains(family) {
            continue;
        }
        let location = system_root.join("Fonts").join(file);
        if !location.exists() {
            warnings.push(format!("미리보기 글꼴을 찾지 못했습니다: {family}"));
            continue;
        }
        let mut bytes = Vec::new();
        fs::File::open(&location)
            .map_err(|error| error.to_string())?
            .take(FONT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > FONT_LIMIT || !preview_embedding_allowed(&bytes) {
            warnings.push(format!(
                "글꼴 미리보기 포함이 허용되지 않아 대체 글꼴로 표시합니다: {family}"
            ));
            continue;
        }
        let encoded = BASE64.encode(&prepare_preview_font(&bytes)?);
        css.push_str(&format!("@font-face{{font-family:'{family}';src:url(data:font/ttf;base64,{encoded}) format('truetype');font-weight:normal;font-style:normal;}}"));
        fonts.push(family);
    }
    Ok(json!({"css":css,"fonts":fonts,"warnings":warnings}))
}

#[cfg(test)]
pub(super) fn run(operation: &str, input: &Value) -> Result<Value, String> {
    if operation == "previewFonts" {
        return preview_fonts(input["svg"].as_str().ok_or("svg 문자열이 필요합니다.")?);
    }
    let hex = input["bytesHex"]
        .as_str()
        .ok_or("bytesHex 문자열이 필요합니다.")?;
    if hex.len() % 2 != 0 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("잘못된 bytesHex입니다.".into());
    }
    let bytes = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.as_bytes())
        .map_err(|error| error.to_string())?;
    match operation {
        "embeddingAllowed" => Ok(json!(preview_embedding_allowed(&bytes))),
        "prepare" => {
            Ok(json!({"bytesHex":data_encoding::HEXLOWER.encode(&prepare_preview_font(&bytes)?)}))
        }
        _ => Err("알 수 없는 미리보기 글꼴 연산입니다.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedding_flags_bounds_and_ascii_tags() {
        let mut bytes = vec![0; 38];
        put32(&mut bytes, 0, 0x0001_0000).unwrap();
        put16(&mut bytes, 4, 1).unwrap();
        bytes[12..16].copy_from_slice(b"OS/2");
        put32(&mut bytes, 20, 28).unwrap();
        put32(&mut bytes, 24, 10).unwrap();
        for (flags, allowed) in [
            (0, true),
            (4, true),
            (8, true),
            (0x100, true),
            (2, false),
            (0x200, false),
            (0x202, false),
        ] {
            put16(&mut bytes, 36, flags).unwrap();
            assert_eq!(preview_embedding_allowed(&bytes), allowed);
        }
        put16(&mut bytes, 36, 0).unwrap();
        bytes[12] |= 0x80;
        assert!(preview_embedding_allowed(&bytes));
        put32(&mut bytes, 20, u32::MAX).unwrap();
        assert!(!preview_embedding_allowed(&bytes));
        assert_eq!(
            prepare_preview_font(&[]).unwrap_err(),
            "Attempt to access memory outside buffer bounds"
        );
    }
    #[test]
    fn checksum_padding_wrap_and_missing_tail_match_legacy() {
        assert_eq!(checksum(&[255; 8], 0, 8), u32::MAX - 1);
        assert_eq!(checksum(&[1, 2, 3], 0, 3), 0x0102_0300);
        assert_eq!(checksum(&[1, 2, 3], 0, u32::MAX as u64), 0);
    }
}
