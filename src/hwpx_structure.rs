//! Independent, read-only port of the PowerShell HWPX structure reader.
//! Uses a namespace-aware XML parser rather than the strict editing model.
use crate::{office_convert::decode_text, office_text::TextMode, office_zip};
use regex::Regex;
use roxmltree::{Document, Node, NodeId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

const HP: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";
const MIB: usize = 1024 * 1024;
static SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Contents/section(\d+)\.xml$").unwrap());

fn little16(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}
fn little32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn is_hp(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(HP) && node.tag_name().name() == name
}
fn ancestor_cell<'a, 'input>(node: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    node.ancestors().skip(1).find(|parent| is_hp(*parent, "tc"))
}
fn integer(node: Node<'_, '_>, name: &str, minimum: i32) -> Result<i32, String> {
    let value = node.attribute(name).unwrap_or("");
    // .NET's anchored regex accepts a final LF; Int32.TryParse then trims it.
    // Literal XML attribute whitespace is normalized by the parser beforehand.
    let digits = value.strip_suffix('\n').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("잘못된 HWPX 숫자 속성: {name}"));
    }
    digits
        .parse::<i32>()
        .ok()
        .filter(|n| *n >= minimum)
        .ok_or_else(|| format!("잘못된 HWPX 숫자 속성: {name}"))
}

fn text_of(node: Node<'_, '_>) -> String {
    let mut text = String::new();
    let mut pending: Vec<_> = node.children().rev().collect();
    while let Some(child) = pending.pop() {
        if child.is_text() {
            text.push_str(child.text().unwrap_or(""));
        } else if child.is_element() {
            if is_hp(child, "lineBreak") {
                text.push('\n');
            } else if is_hp(child, "tab") {
                text.push('\t');
            } else {
                pending.extend(child.children().rev());
            }
        }
    }
    text
}

fn decode_xml(bytes: &[u8]) -> Result<String, String> {
    // XmlReader recognizes UTF-16/32 XML without a BOM. The plain-text
    // converter's StreamReader compatibility must remain separate.
    let marker: Option<&[u8]> = if bytes.starts_with(&[b'<', 0, 0, 0]) {
        Some(&[0xff, 0xfe, 0, 0])
    } else if bytes.starts_with(&[0, 0, 0, b'<']) {
        Some(&[0, 0, 0xfe, 0xff])
    } else if bytes.starts_with(&[b'<', 0]) {
        Some(&[0xff, 0xfe])
    } else if bytes.starts_with(&[0, b'<']) {
        Some(&[0xfe, 0xff])
    } else {
        None
    };
    if let Some(marker) = marker {
        let mut marked = Vec::with_capacity(marker.len() + bytes.len());
        marked.extend_from_slice(marker);
        marked.extend_from_slice(bytes);
        decode_text(&marked, TextMode::PowerShell)
    } else {
        decode_text(bytes, TextMode::PowerShell)
    }
}

fn section(name: &str, xml: &str) -> Result<Value, String> {
    if xml.encode_utf16().count() > 32 * MIB {
        return Err("본문 XML 크기 제한을 넘었습니다.".into());
    }
    let document = Document::parse(xml).map_err(|e| format!("HWPX XML을 읽을 수 없습니다: {e}"))?;
    let paragraphs: Vec<_> = document.descendants().filter(|n| is_hp(*n, "p")).collect();
    let tables: Vec<_> = document
        .descendants()
        .filter(|n| is_hp(*n, "tbl"))
        .collect();
    let paragraph_keys: HashMap<NodeId, String> = paragraphs
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id(), format!("{name}/p{i}")))
        .collect();
    let table_keys: HashMap<NodeId, String> = tables
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id(), format!("{name}/tbl{i}")))
        .collect();
    let mut cell_keys: HashMap<NodeId, String> = HashMap::new();
    let mut table_results = Vec::new();
    for table in &tables {
        let key = &table_keys[&table.id()];
        let rows = integer(*table, "rowCnt", 1)?;
        let cols = integer(*table, "colCnt", 1)?;
        if i64::from(rows) * i64::from(cols) > 1_000_000 {
            return Err("표의 행·열 크기 제한을 넘었습니다.".into());
        }
        let mut occupied = HashSet::new();
        let mut cells = Vec::new();
        for cell in table
            .children()
            .filter(|n| is_hp(*n, "tr"))
            .flat_map(|row| row.children().filter(|n| is_hp(*n, "tc")))
        {
            let address = cell
                .children()
                .find(|n| is_hp(*n, "cellAddr"))
                .ok_or("셀 주소 또는 병합 범위가 없습니다.")?;
            let span = cell
                .children()
                .find(|n| is_hp(*n, "cellSpan"))
                .ok_or("셀 주소 또는 병합 범위가 없습니다.")?;
            let row = integer(address, "rowAddr", 0)?;
            let col = integer(address, "colAddr", 0)?;
            let row_span = integer(span, "rowSpan", 1)?;
            let col_span = integer(span, "colSpan", 1)?;
            if i64::from(row) + i64::from(row_span) > i64::from(rows)
                || i64::from(col) + i64::from(col_span) > i64::from(cols)
            {
                return Err("셀이 표 범위를 벗어났습니다.".into());
            }
            for r in row..row + row_span {
                for c in col..col + col_span {
                    if !occupied.insert((r, c)) {
                        return Err("셀 또는 병합 범위가 겹칩니다.".into());
                    }
                }
            }
            let cell_key = format!("{key}/r{row}c{col}");
            cell_keys.insert(cell.id(), cell_key.clone());
            let direct: Vec<_> = cell
                .children()
                .filter(|n| is_hp(*n, "subList"))
                .flat_map(|list| list.children().filter(|n| is_hp(*n, "p")))
                .map(|node| paragraph_keys[&node.id()].clone())
                .collect();
            cells.push(json!({"key":cell_key,"row":row,"col":col,"rowSpan":row_span,"colSpan":col_span,"paragraphs":direct,"childTables":[]}));
        }
        table_results.push(json!({"key":key,"id":table.attribute("id").unwrap_or(""),"rows":rows,"cols":cols,"parentCell":null,"cells":cells}));
    }
    for (i, table) in tables.iter().enumerate() {
        if let Some(parent) = ancestor_cell(*table) {
            let parent_key = cell_keys
                .get(&parent.id())
                .ok_or("지원하지 않는 중첩표 위치입니다.")?;
            let child_key = table_keys[&table.id()].clone();
            table_results[i]["parentCell"] = json!(parent_key);
            for table_result in &mut table_results {
                for cell in table_result["cells"].as_array_mut().unwrap() {
                    if cell["key"].as_str() == Some(parent_key) {
                        cell["childTables"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(child_key));
                    }
                }
            }
        }
    }
    let mut paragraph_results = Vec::new();
    for paragraph in &paragraphs {
        let mut text = String::new();
        for node in paragraph
            .children()
            .filter(|n| is_hp(*n, "run"))
            .flat_map(|run| run.children().filter(|n| is_hp(*n, "t")))
        {
            text.push_str(&text_of(node));
        }
        let cell = ancestor_cell(*paragraph)
            .map(|node| {
                cell_keys
                    .get(&node.id())
                    .cloned()
                    .ok_or("지원하지 않는 셀 문단 위치입니다.")
            })
            .transpose()?;
        paragraph_results
            .push(json!({"key":paragraph_keys[&paragraph.id()],"text":text,"cell":cell}));
    }
    Ok(json!({"name":name,"paragraphs":paragraph_results,"tables":table_results}))
}

pub fn read_structure(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 128 * MIB {
        return Err("HWPX 파일이 아니거나 크기 제한을 넘었습니다.".into());
    }
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err("실제 HWPX ZIP 형식이 아닙니다. HWP는 한글 변환 절차로 확인하세요.".into());
    }
    let end = if bytes.len() < 22 {
        None
    } else {
        (bytes.len().saturating_sub(65557)..=bytes.len() - 22)
            .rev()
            .find(|i| {
                little32(bytes, *i) == Some(0x0605_4b50)
                    && little16(bytes, i + 20)
                        .is_some_and(|n| i + 22 + usize::from(n) == bytes.len())
            })
    }
    .ok_or("ZIP 끝 레코드가 없거나 잘못됐습니다.")?;
    if little16(bytes, end + 10) == Some(u16::MAX)
        || little32(bytes, end + 12) == Some(u32::MAX)
        || little32(bytes, end + 16) == Some(u32::MAX)
    {
        return Err("ZIP64는 지원하지 않습니다.".into());
    }
    if little16(bytes, end + 4) != Some(0) || little16(bytes, end + 6) != Some(0) {
        return Err("여러 파일로 분할된 ZIP은 지원하지 않습니다.".into());
    }
    let archive = office_zip::read_zip_powershell(bytes)?;
    let mut names = HashSet::new();
    let mut expanded = 0_u64;
    let mut sections = Vec::new();
    for entry in &archive.entries {
        if !names.insert(&entry.name) {
            return Err("ZIP에 같은 이름의 항목이 두 번 있습니다.".into());
        }
        expanded += u64::from(entry.length);
        if expanded > 256 * MIB as u64 {
            return Err("ZIP 압축 해제 크기 제한을 넘었습니다.".into());
        }
        if let Some(capture) = SECTION.captures(&entry.name) {
            let number = capture[1]
                .parse::<i64>()
                .map_err(|_| "본문 번호가 정수 범위를 벗어났습니다.")?;
            sections.push((number, entry));
        }
    }
    if sections.is_empty() {
        return Err("HWPX 본문(Contents/section*.xml)이 없습니다.".into());
    }
    sections.sort_by_key(|(number, _)| *number);
    let mut results = Vec::new();
    for (_, entry) in sections {
        if entry.length as usize > 32 * MIB {
            return Err("본문 XML 크기 제한을 넘었습니다.".into());
        }
        let data = archive.read_entry(entry)?;
        let xml = decode_xml(&data)?;
        results.push(section(&entry.name, &xml)?);
    }
    let source_hash: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(json!({"schemaVersion":1,"sourceSha256":source_hash,"sections":results}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wrap(body: &str) -> String {
        format!("<section xmlns:hp='{HP}' xmlns:가='urn:test'>{body}</section>")
    }
    fn zip(body: &str) -> Vec<u8> {
        office_zip::create_zip(&[("Contents/section0.xml".into(), wrap(body).into_bytes())])
            .unwrap()
    }
    fn cell(contents: &str) -> String {
        format!(
            "<hp:tc><hp:cellAddr rowAddr='0' colAddr='0'/><hp:cellSpan rowSpan='1' colSpan='1'/><hp:subList>{contents}</hp:subList></hp:tc>"
        )
    }
    #[test]
    fn preserves_direct_text_whitespace_namespace_and_nested_links() {
        let nested = format!(
            "<hp:tbl id='inside' rowCnt='1' colCnt='1'><hp:tr>{}</hp:tr></hp:tbl>",
            cell("<hp:p><hp:run><hp:t>안쪽</hp:t></hp:run></hp:p>")
        );
        let outer = format!(
            "<hp:p><hp:run><hp:t>바깥 {nested}<hp:lineBreak/><hp:tab/><가:이름>다음</가:이름></hp:t></hp:run></hp:p>"
        );
        let source = zip(&format!(
            "<hp:tbl id='outside' rowCnt='1' colCnt='1'><hp:tr>{}</hp:tr></hp:tbl><hp:p><hp:run><hp:t><![CDATA[  ]]></hp:t></hp:run></hp:p>",
            cell(&outer)
        ));
        let value = read_structure(&source).unwrap();
        let result = &value["sections"][0];
        assert_eq!(result["paragraphs"][0]["text"], "바깥 안쪽\n\t다음");
        assert_eq!(result["paragraphs"][1]["text"], "안쪽");
        assert_eq!(result["paragraphs"][2]["text"], "  ");
        assert_eq!(
            result["tables"][1]["parentCell"],
            "Contents/section0.xml/tbl0/r0c0"
        );
        assert_eq!(
            result["tables"][0]["cells"][0]["childTables"],
            json!(["Contents/section0.xml/tbl1"])
        );
    }
    #[test]
    fn first_address_is_used_but_orphan_cells_are_rejected() {
        let source = zip(
            "<hp:tbl rowCnt='1' colCnt='1'><hp:tr><hp:tc><hp:cellAddr rowAddr='0' colAddr='0'/><hp:cellAddr rowAddr='99' colAddr='99'/><hp:cellSpan rowSpan='1' colSpan='1'/><hp:cellSpan rowSpan='9' colSpan='9'/></hp:tc></hp:tr></hp:tbl>",
        );
        assert!(read_structure(&source).is_ok());
        assert!(
            read_structure(&zip("<hp:tc><hp:p/></hp:tc>"))
                .unwrap_err()
                .contains("셀 문단 위치")
        );
        assert!(
            read_structure(&zip("<hp:tc><hp:tbl rowCnt='1' colCnt='1'/></hp:tc>"))
                .unwrap_err()
                .contains("중첩표 위치")
        );
        // A parentless tc without paragraphs or child tables is ignored.
        assert!(read_structure(&zip("<hp:tc/>")).is_ok());
    }
    #[test]
    fn validates_integer_merges_and_prohibits_dtd() {
        for rows in ["2147483648", "0", "-1", "1.0", "", "١"] {
            assert!(
                read_structure(&zip(&format!("<hp:tbl rowCnt='{rows}' colCnt='1'/>"))).is_err()
            );
        }
        assert!(read_structure(&zip("<hp:tbl rowCnt='001' colCnt='1'/>")).is_ok());
        assert!(read_structure(&zip("<hp:tbl rowCnt='1&#10;' colCnt='1'/>")).is_ok());
        assert!(
            read_structure(&zip("<hp:tbl rowCnt='1001' colCnt='1000'/>"))
                .unwrap_err()
                .contains("크기 제한")
        );
        let duplicate = format!(
            "<hp:tbl rowCnt='1' colCnt='1'><hp:tr>{}{}</hp:tr></hp:tbl>",
            cell(""),
            cell("")
        );
        assert!(
            read_structure(&zip(&duplicate))
                .unwrap_err()
                .contains("겹칩니다")
        );
        assert!(section("x", "<!DOCTYPE x [<!ENTITY a 'x'>]><x>&a;</x>").is_err());
        assert!(section("x", "<x>&unknown;</x>").is_err());
    }
    #[test]
    fn independent_zip_rules_sort_sections_and_skip_unrelated_data() {
        let source = office_zip::create_zip(&[
            (
                "Contents/section10.xml".into(),
                wrap("<hp:p/>").into_bytes(),
            ),
            ("Contents/section2.xml".into(), wrap("<hp:p/>").into_bytes()),
            ("unrelated.bin".into(), vec![0xff]),
        ])
        .unwrap();
        let value = read_structure(&source).unwrap();
        assert_eq!(value["sections"][0]["name"], "Contents/section2.xml");
        assert_eq!(value["sections"][1]["name"], "Contents/section10.xml");
        assert_eq!(value["sourceSha256"].as_str().unwrap().len(), 64);
        // Unlike strict writing, a structure read requires no mimetype/header.
        assert!(crate::hwpx_document::load_hwpx(&source).is_err());
        let mut trailing = source.clone();
        trailing.push(0);
        assert!(read_structure(&trailing).unwrap_err().contains("끝 레코드"));
    }

    #[test]
    fn structure_preserves_dotnet_volume_and_entry_count_checks() {
        let bytes = zip("<hp:p/>");
        let end = bytes.len() - 22;
        let central = little32(&bytes, end + 16).unwrap() as usize;
        for (position, value) in [
            (end + 4, 1_u16),
            (end + 6, 1),
            (end + 8, 0),
            (central + 34, 1),
        ] {
            let mut changed = bytes.clone();
            changed[position..position + 2].copy_from_slice(&value.to_le_bytes());
            assert!(read_structure(&changed).is_err(), "position {position}");
        }
        for flag in [1_u16, 64] {
            let mut changed = bytes.clone();
            changed[6..8].copy_from_slice(&(0x800 | flag).to_le_bytes());
            changed[central + 8..central + 10].copy_from_slice(&(0x800 | flag).to_le_bytes());
            assert!(read_structure(&changed).is_ok());
        }
        let mut unused = office_zip::create_zip(&[
            ("Contents/section0.xml".into(), wrap("<hp:p/>").into_bytes()),
            ("unrelated.bin".into(), vec![0xff]),
        ])
        .unwrap();
        let central = little32(&unused, unused.len() - 22 + 16).unwrap() as usize;
        let second = central + 46 + "Contents/section0.xml".len();
        unused[second + 34..second + 36].copy_from_slice(&1_u16.to_le_bytes());
        assert!(read_structure(&unused).is_ok());
    }

    #[test]
    fn xml_reader_detects_bomless_utf16_and_utf32() {
        let xml = wrap("<hp:p><hp:run><hp:t>한글😀</hp:t></hp:run></hp:p>");
        for little in [true, false] {
            let utf16: Vec<u8> = xml
                .encode_utf16()
                .flat_map(|unit| {
                    if little {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    }
                })
                .collect();
            let utf32: Vec<u8> = xml
                .chars()
                .flat_map(|c| {
                    if little {
                        (c as u32).to_le_bytes()
                    } else {
                        (c as u32).to_be_bytes()
                    }
                })
                .collect();
            for data in [utf16, utf32] {
                let bytes =
                    office_zip::create_zip(&[("Contents/section0.xml".into(), data)]).unwrap();
                let value = read_structure(&bytes).unwrap();
                assert_eq!(value["sections"][0]["paragraphs"][0]["text"], "한글😀");
            }
        }
    }
}
