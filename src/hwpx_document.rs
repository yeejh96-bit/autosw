//! HWPX structure inspection. The original bytes and XML positions remain
//! available for the separately authorized document-writing stage.
use crate::input::trim;
use crate::office_xml::{ParsedXml, XmlPart, parse_office_xml};
use crate::office_zip::{Archive, read_zip_strict};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Target {
    pub key: String,
    pub kind: String,
    pub section: String,
    pub node: usize,
    pub paragraphs: Vec<usize>,
    pub before: Vec<String>,
    pub writable: bool,
    pub reason: Option<String>,
}
pub struct LoadedHwpx {
    pub bytes: Vec<u8>,
    pub archive: Archive,
    pub internals: Vec<(String, ParsedXml)>,
    pub targets: Vec<Target>,
    pub structure: Value,
}
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn integer(value: Option<&str>, positive: bool) -> Result<u64, String> {
    let error = "표 주소·크기·병합 정보가 잘못됐습니다.";
    let value = value.ok_or(error)?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error.into());
    }
    let significant = value.trim_start_matches('0');
    let number = if significant.is_empty() {
        0
    } else {
        significant.parse::<u64>().map_err(|_| error)?
    };
    if number > 9_007_199_254_740_991 || (positive && number == 0) {
        return Err(error.into());
    }
    Ok(number)
}
fn plain_paragraph(doc: &ParsedXml, p: usize) -> Option<String> {
    let reason = |s: &str| Some(s.to_owned());
    if ["ctrl", "header", "footer", "drawText"]
        .iter()
        .any(|name| doc.ancestor_hp(p, name).is_some())
    {
        return reason("머리말·꼬리말·제어 요소의 문단");
    }
    let mut parent = doc.nodes[p].parent;
    while let Some(i) = parent {
        let n = &doc.nodes[i];
        if n.is_hp("tc") && matches!(n.attr("protect"), Some("1" | "true")) {
            return reason("보호된 셀");
        }
        parent = n.parent;
    }
    if doc.nodes[p]
        .children
        .iter()
        .any(|&i| !doc.nodes[i].is_hp("run") && !doc.nodes[i].is_hp("linesegarray"))
    {
        return reason("지원하지 않는 문단 요소");
    }
    let runs = doc.children_hp(p, "run");
    if runs.is_empty()
        || runs.iter().any(|&i| {
            let r = &doc.nodes[i];
            r.children.len() > 1
                || r.children
                    .first()
                    .is_some_and(|&j| !doc.nodes[j].is_hp("t"))
                || r.parts
                    .iter()
                    .any(|part| matches!(part,XmlPart::Text(s) if !trim(s).is_empty()))
        })
    {
        return reason("그림·표·제어 요소 또는 잘못된 run");
    }
    let style = serde_json::to_string(&doc.nodes[runs[0]].attrs).unwrap();
    if runs
        .iter()
        .any(|&i| serde_json::to_string(&doc.nodes[i].attrs).unwrap() != style)
    {
        return reason("혼합 글자 서식");
    }
    if runs.iter().any(|&i| {
        doc.nodes[i].children.first().is_some_and(|&t| {
            doc.nodes[t]
                .children
                .iter()
                .any(|&j| !doc.nodes[j].is_hp("lineBreak"))
        })
    }) {
        return reason("텍스트 안의 탭·제어 요소");
    }
    None
}
fn target(
    doc: &ParsedXml,
    key: String,
    kind: &str,
    section: &str,
    node: usize,
    paragraphs: Vec<usize>,
    reason: Option<String>,
) -> Target {
    let before = paragraphs.iter().map(|&p| doc.paragraph_text(p)).collect();
    Target {
        key,
        kind: kind.into(),
        section: section.into(),
        node,
        paragraphs,
        before,
        writable: reason.is_none(),
        reason,
    }
}
fn section_number(name: &str) -> Option<f64> {
    let digits = name
        .strip_prefix("Contents/section")?
        .strip_suffix(".xml")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.parse().unwrap_or(f64::INFINITY))
}
pub fn load_hwpx(bytes: &[u8]) -> Result<LoadedHwpx, String> {
    let archive = read_zip_strict(bytes)?;
    let mime = std::str::from_utf8(archive.read("mimetype").unwrap_or_default())
        .map_err(|e| e.to_string())?;
    if trim(mime) != "application/hwp+zip" {
        return Err("HWPX mimetype이 없습니다.".into());
    }
    if archive.entries.iter().any(|e| {
        let n = e.name.to_ascii_lowercase();
        n.contains("encrypt")
            || n.contains("signature")
            || n.contains("scripts/")
            || n.contains("docoptions/password")
            || n.contains("docoptions/drm")
    }) {
        return Err("암호화·서명·스크립트 문서는 지원하지 않습니다.".into());
    }
    let mut names: Vec<_> = archive
        .entries
        .iter()
        .filter_map(|e| section_number(&e.name).map(|n| (e.name.clone(), n)))
        .collect();
    names.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    if names.is_empty() {
        return Err("HWPX 본문이 없습니다.".into());
    }
    let mut sections = Vec::new();
    let mut internals = Vec::new();
    let mut targets = Vec::new();
    for (name, _) in names {
        let data = archive.read(&name).unwrap();
        if data.len() > 32 * 1024 * 1024 {
            return Err("본문 XML 크기 한도를 넘었습니다.".into());
        }
        let doc = parse_office_xml(std::str::from_utf8(data).map_err(|e| e.to_string())?)?;
        let ps: Vec<_> = doc
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_hp("p"))
            .map(|(i, _)| i)
            .collect();
        let ts: Vec<_> = doc
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_hp("tbl"))
            .map(|(i, _)| i)
            .collect();
        let pkeys: HashMap<_, _> = ps
            .iter()
            .enumerate()
            .map(|(i, &p)| (p, format!("{name}/p{i}")))
            .collect();
        let tkeys: HashMap<_, _> = ts
            .iter()
            .enumerate()
            .map(|(i, &t)| (t, format!("{name}/tbl{i}")))
            .collect();
        let mut ckeys = HashMap::new();
        let mut tables = Vec::new();
        for &t in &ts {
            let key = &tkeys[&t];
            let rows = integer(doc.nodes[t].attr("rowCnt"), true)?;
            let cols = integer(doc.nodes[t].attr("colCnt"), true)?;
            if u128::from(rows) * u128::from(cols) > 1_000_000 {
                return Err("표 크기 한도를 넘었습니다.".into());
            }
            let mut occupied = HashSet::new();
            let mut cells = Vec::new();
            for tr in doc.children_hp(t, "tr") {
                for c in doc.children_hp(tr, "tc") {
                    let addr = doc.children_hp(c, "cellAddr");
                    let span = doc.children_hp(c, "cellSpan");
                    if addr.len() != 1 || span.len() != 1 {
                        return Err("셀 주소 또는 병합 정보가 없습니다.".into());
                    }
                    let row = integer(doc.nodes[addr[0]].attr("rowAddr"), false)?;
                    let col = integer(doc.nodes[addr[0]].attr("colAddr"), false)?;
                    let row_span = integer(doc.nodes[span[0]].attr("rowSpan"), true)?;
                    let col_span = integer(doc.nodes[span[0]].attr("colSpan"), true)?;
                    if row + row_span > rows || col + col_span > cols {
                        return Err("셀 병합이 표 밖으로 나갑니다.".into());
                    }
                    for r in row..row + row_span {
                        for cc in col..col + col_span {
                            if !occupied.insert((r, cc)) {
                                return Err("겹치는 표 셀입니다.".into());
                            }
                        }
                    }
                    let cell_key = format!("{key}/r{row}c{col}");
                    ckeys.insert(c, cell_key.clone());
                    let sublists = doc.children_hp(c, "subList");
                    let paragraphs: Vec<_> = sublists
                        .iter()
                        .flat_map(|&s| doc.children_hp(s, "p"))
                        .collect();
                    let children: Vec<_> = ts
                        .iter()
                        .filter(|&&child| doc.ancestor_hp(child, "tc") == Some(c))
                        .map(|child| tkeys[child].clone())
                        .collect();
                    cells.push(json!({"key":cell_key,"row":row,"col":col,"rowSpan":row_span,"colSpan":col_span,"paragraphs":paragraphs.iter().map(|p|pkeys[p].clone()).collect::<Vec<_>>(),"childTables":children}));
                    let reason = if paragraphs.is_empty() {
                        Some("입력 문단 없음".into())
                    } else if sublists.len() != 1
                        || doc.nodes[sublists[0]]
                            .children
                            .iter()
                            .any(|&i| !doc.nodes[i].is_hp("p"))
                    {
                        Some("복잡한 셀 본문".into())
                    } else if !children.is_empty() {
                        Some("중첩표를 포함한 부모 셀".into())
                    } else {
                        paragraphs.iter().find_map(|&p| plain_paragraph(&doc, p))
                    };
                    targets.push(target(&doc, cell_key, "cell", &name, c, paragraphs, reason));
                }
            }
            tables.push(json!({"key":key,"id":doc.nodes[t].attr("id").unwrap_or(""),"rows":rows,"cols":cols,"parentCell":Value::Null,"cells":cells}));
        }
        for (i, &t) in ts.iter().enumerate() {
            tables[i]["parentCell"] = json!(doc.ancestor_hp(t, "tc").and_then(|p| ckeys.get(&p)));
        }
        let mut paragraphs = Vec::new();
        for &p in &ps {
            let key = pkeys[&p].clone();
            let reason = plain_paragraph(&doc, p);
            targets.push(target(
                &doc,
                key.clone(),
                "paragraph",
                &name,
                p,
                vec![p],
                reason,
            ));
            paragraphs.push(json!({"key":key,"text":doc.paragraph_text(p),"cell":doc.ancestor_hp(p,"tc").and_then(|p|ckeys.get(&p))}));
        }
        sections.push(json!({"name":name,"paragraphs":paragraphs,"tables":tables}));
        internals.push((name, doc));
    }
    let structure = json!({"schemaVersion":1,"sourceSha256":sha256(bytes),"sections":sections});
    Ok(LoadedHwpx {
        bytes: bytes.to_vec(),
        archive,
        internals,
        targets,
        structure,
    })
}
pub fn inspect_hwpx(bytes: &[u8]) -> Result<Value, String> {
    let doc = load_hwpx(bytes)?;
    let mut result = doc.structure;
    result["targets"]=json!(doc.targets.iter().map(|t|json!({"key":t.key,"kind":t.kind,"before":t.before,"writable":t.writable,"reason":t.reason})).collect::<Vec<_>>());
    result["visualStatus"] = json!("not-checked");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office_xml::HP;
    use crate::office_zip::create_zip;
    fn fixture(body: &str) -> Vec<u8> {
        create_zip(&[
            ("mimetype".into(), b"application/hwp+zip".to_vec()),
            (
                "Contents/section0.xml".into(),
                format!("<s xmlns:h='{HP}'>{body}</s>").into_bytes(),
            ),
        ])
        .unwrap()
    }
    fn cell(body: &str, extra: &str) -> String {
        format!(
            "<h:tc {extra}><h:cellAddr rowAddr='0' colAddr='0'/><h:cellSpan rowSpan='1' colSpan='1'/><h:subList>{body}</h:subList></h:tc>"
        )
    }
    #[test]
    fn nested_tables_keep_direct_body_empty_paragraphs_and_editability() {
        let child = format!(
            "<h:tbl rowCnt='1' colCnt='1' id='child'><h:tr>{}</h:tr></h:tbl>",
            cell("<h:p><h:run><h:t>자식😀</h:t></h:run></h:p>", "")
        );
        let outer = format!(
            "<h:tbl rowCnt='1' colCnt='1'><h:tr>{}</h:tr></h:tbl>",
            cell(
                &format!(
                    "<h:p><h:run><h:t>부모</h:t></h:run><h:run>{child}</h:run></h:p><h:p><h:run/></h:p>"
                ),
                ""
            )
        );
        let bytes = fixture(&format!(
            "<h:p><h:run>{outer}</h:run></h:p><h:p><h:run><h:t> </h:t></h:run></h:p>"
        ));
        let doc = load_hwpx(&bytes).unwrap();
        let section = &doc.structure["sections"][0];
        let ps = section["paragraphs"].as_array().unwrap();
        assert_eq!(
            ps.iter()
                .map(|p| p["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["", "부모", "자식😀", "", " "]
        );
        assert_eq!(
            section["tables"][0]["cells"][0]["childTables"],
            json!(["Contents/section0.xml/tbl1"])
        );
        assert_eq!(
            section["tables"][1]["parentCell"],
            "Contents/section0.xml/tbl0/r0c0"
        );
        assert_eq!(
            doc.targets[0].reason.as_deref(),
            Some("중첩표를 포함한 부모 셀")
        );
        assert!(doc.targets[1].writable);
        let parsed = &doc.internals[0].1;
        assert!(parsed.xml[parsed.nodes[doc.targets[1].node].start..].starts_with("<h:tc"));
        assert_eq!(inspect_hwpx(&bytes).unwrap()["visualStatus"], "not-checked");
    }
    #[test]
    fn protected_mixed_and_control_paragraphs_remain_nonwritable() {
        let table = format!(
            "<h:tbl rowCnt='1' colCnt='1'><h:tr>{}</h:tr></h:tbl>",
            cell("<h:p><h:run><h:t>보호</h:t></h:run></h:p>", "protect='1'")
        );
        let bytes = fixture(&format!(
            "{table}<h:p><h:run a='1'><h:t>A</h:t></h:run><h:run a='2'/></h:p><h:p><h:run><h:t><h:tab/></h:t></h:run></h:p>"
        ));
        let doc = load_hwpx(&bytes).unwrap();
        assert_eq!(doc.targets[0].reason.as_deref(), Some("보호된 셀"));
        assert!(
            doc.targets
                .iter()
                .any(|t| t.reason.as_deref() == Some("혼합 글자 서식"))
        );
        assert!(
            doc.targets
                .iter()
                .any(|t| t.reason.as_deref() == Some("텍스트 안의 탭·제어 요소"))
        );
    }
    #[test]
    fn strict_documents_reject_bad_merges_active_content_and_missing_mime() {
        for body in [
            "<h:tbl rowCnt='1' colCnt='1'><h:tr><h:tc/></h:tr></h:tbl>".into(),
            format!(
                "<h:tbl rowCnt='1' colCnt='1'><h:tr>{0}{0}</h:tr></h:tbl>",
                cell("", "")
            ),
            "<h:tbl rowCnt='1000001' colCnt='1'/>".into(),
            "<h:tbl rowCnt='1.0' colCnt='1'/>".into(),
        ] {
            assert!(load_hwpx(&fixture(&body)).is_err());
        }
        for name in [
            "Scripts/code.js",
            "META-INF/signatures.xml",
            "DocOptions/password.xml",
            "encrypt.xml",
        ] {
            let bytes = create_zip(&[
                ("mimetype".into(), b"application/hwp+zip".to_vec()),
                ("Contents/section0.xml".into(), b"<s/>".to_vec()),
                (name.into(), Vec::new()),
            ])
            .unwrap();
            assert!(
                load_hwpx(&bytes)
                    .err()
                    .unwrap()
                    .contains("암호화·서명·스크립트")
            );
        }
        let bytes = create_zip(&[("Contents/section0.xml".into(), b"<s/>".to_vec())]).unwrap();
        assert_eq!(
            load_hwpx(&bytes).err().unwrap(),
            "HWPX mimetype이 없습니다."
        );
    }
    #[test]
    fn javascript_numeric_and_section_anchors_reject_trailing_line_terminators() {
        // JS /^\d+$/ without /m rejects final line terminators, unlike .NET's $.
        for tail in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}", "\n\n"] {
            assert!(integer(Some(&format!("1{tail}")), true).is_err());
            assert!(section_number(&format!("Contents/section1.xml{tail}")).is_none());
        }
        for reference in ["&#10;", "&#13;", "&#13;&#10;", "&#x2028;", "&#x2029;"] {
            let bytes = fixture(&format!("<h:tbl rowCnt='1{reference}' colCnt='1'/>"));
            assert_eq!(
                load_hwpx(&bytes).err().unwrap(),
                "표 주소·크기·병합 정보가 잘못됐습니다."
            );
        }
    }
}
