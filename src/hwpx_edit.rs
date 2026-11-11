//! In-memory HWPX edits. Final publication and approval remain the caller's job.
//! Rebuilds and verifies the exact output; compression differences never relax
//! source hashes, preservation checks, or the byte equality in verify_hwpx.
use crate::hwpx_document::{LoadedHwpx, Target, load_hwpx, sha256};
use crate::input::{js_string, trim};
use crate::office_xml::{ParsedXml, XmlNode, check_xml_text, xml_escape};
use crate::office_zip::patch_zip;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub struct Prepared {
    pub output: Vec<u8>,
    pub preview: Value,
    pub unassigned: Value,
    pub report: Value,
}
struct Edit {
    start: usize,
    end: usize,
    value: String,
}
fn exact_keys(value: &Value, allowed: &[&str], label: &str) -> Result<(), String> {
    if value
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(format!("{label}에 잘못된 항목이 있습니다."));
    }
    Ok(())
}
fn field_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(js_string)
        .unwrap_or_else(|| "undefined".into())
}
fn prefix(node: &XmlNode) -> String {
    node.name
        .split_once(':')
        .map_or_else(String::new, |(p, _)| format!("{p}:"))
}
fn text_content(value: &str, prefix: &str) -> Result<String, String> {
    value
        .split('\n')
        .map(xml_escape)
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join(&format!("<{prefix}lineBreak/>")))
}
fn text_element(doc: &ParsedXml, node: usize, value: &str) -> Result<String, String> {
    let n = &doc.nodes[node];
    let content = text_content(value, &prefix(n))?;
    let opening = &doc.xml[n.start..n.open_end];
    if n.self_closing {
        Ok(format!(
            "{}>{content}</{}>",
            &opening[..opening.len() - 2],
            n.name
        ))
    } else {
        Ok(format!(
            "{opening}{content}{}",
            &doc.xml[n.close_start..n.end]
        ))
    }
}
fn paragraph_replacement(doc: &ParsedXml, paragraph: usize, value: &str) -> Result<String, String> {
    let p = &doc.nodes[paragraph];
    let mut edits = Vec::new();
    for (index, run) in doc.children_hp(paragraph, "run").into_iter().enumerate() {
        let r = &doc.nodes[run];
        let value = if index == 0 { value } else { "" };
        if let Some(&t) = r.children.first() {
            let node = &doc.nodes[t];
            edits.push(Edit {
                start: node.start,
                end: node.end,
                value: text_element(doc, t, value)?,
            });
        } else {
            let prefix = prefix(r);
            let content = format!("<{prefix}t>{}</{prefix}t>", text_content(value, &prefix)?);
            let replacement = if r.self_closing {
                let opening = &doc.xml[r.start..r.end];
                format!("{}>{content}</{}>", &opening[..opening.len() - 2], r.name)
            } else {
                format!(
                    "{}{content}{}",
                    &doc.xml[r.start..r.open_end],
                    &doc.xml[r.close_start..r.end]
                )
            };
            edits.push(Edit {
                start: r.start,
                end: r.end,
                value: replacement,
            });
        }
    }
    for line in doc.children_hp(paragraph, "linesegarray") {
        let n = &doc.nodes[line];
        edits.push(Edit {
            start: n.start,
            end: n.end,
            value: String::new(),
        });
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut text = doc.xml[p.start..p.end].to_owned();
    for edit in edits {
        text.replace_range(edit.start - p.start..edit.end - p.start, &edit.value);
    }
    Ok(text)
}
struct Lookup<'a> {
    targets: HashMap<&'a str, &'a Target>,
    sections: HashMap<&'a str, &'a ParsedXml>,
}
impl<'a> Lookup<'a> {
    fn new(doc: &'a LoadedHwpx) -> Self {
        Self {
            targets: doc.targets.iter().map(|t| (t.key.as_str(), t)).collect(),
            sections: doc
                .internals
                .iter()
                .map(|(name, xml)| (name.as_str(), xml))
                .collect(),
        }
    }
}
fn internal<'a>(index: &Lookup<'a>, section: &str) -> &'a ParsedXml {
    index.sections[section]
}
fn find_target<'a>(index: &Lookup<'a>, key: &str) -> Option<&'a Target> {
    index.targets.get(key).copied()
}
fn identity(index: &Lookup<'_>, target: &Target, paragraph: usize) -> (String, usize) {
    (
        target.section.clone(),
        internal(index, &target.section).nodes[paragraph].start,
    )
}

pub fn prepare_hwpx(bytes: &[u8], request: &Value) -> Result<Prepared, String> {
    exact_keys(
        request,
        &[
            "schemaVersion",
            "sourceSha256",
            "changes",
            "requiredTargets",
            "unmapped",
        ],
        "입력",
    )?;
    let doc = load_hwpx(bytes)?;
    let index = Lookup::new(&doc);
    if request["schemaVersion"].as_f64() != Some(1.0)
        || request["sourceSha256"] != doc.structure["sourceSha256"]
    {
        return Err("원본 버전이 달라졌습니다. 구조와 미리보기를 다시 확인하세요.".into());
    }
    let changes = request["changes"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("입력할 변경 항목이 없습니다.")?;
    if let Some(unmapped) = request.get("unmapped")
        && unmapped.as_array().is_none_or(|a| !a.is_empty())
    {
        return Err("입력 위치를 정하지 못한 항목이 남아 있습니다.".into());
    }
    if let Some(required) = request.get("requiredTargets")
        && required.as_array().is_none_or(|keys| {
            keys.iter().any(|key| {
                !key.is_string() || !changes.iter().any(|c| c.get("target") == Some(key))
            })
        })
    {
        return Err("필수 입력 위치의 조치가 누락됐습니다.".into());
    }
    let mut edits: Vec<(String, Vec<Edit>)> = Vec::new();
    let mut touched = HashSet::new();
    let mut preview = Vec::new();
    for change in changes {
        exact_keys(
            change,
            &["target", "label", "source", "action", "expected", "values"],
            "변경",
        )?;
        let target_name = field_string(change, "target");
        let target = change["target"]
            .as_str()
            .and_then(|key| find_target(&index, key))
            .ok_or_else(|| format!("입력 위치가 없습니다: {target_name}"))?;
        if !target.writable && change["action"] != "preserve" {
            return Err(format!(
                "지원하지 않는 입력 위치: {target_name} ({})",
                target.reason.as_deref().unwrap_or("null")
            ));
        }
        if change["label"].as_str().is_none_or(|s| trim(s).is_empty())
            || change["source"].as_str().is_none_or(|s| trim(s).is_empty())
        {
            return Err("항목명과 확인된 값의 출처가 필요합니다.".into());
        }
        let action = change["action"]
            .as_str()
            .filter(|s| matches!(*s, "set" | "clear" | "preserve"))
            .ok_or("입력·비우기·보존 조치를 지정하세요.")?;
        let before = json!(target.before);
        if change.get("expected") != Some(&before) {
            return Err(format!("현재 값이 다릅니다: {target_name}"));
        }
        if action == "preserve" && change.get("values").is_some_and(|v| v != &before) {
            return Err("보존 조치의 값은 현재 값과 같아야 합니다.".into());
        }
        let missing_values = Value::Null;
        let values = if action == "preserve" {
            &before
        } else {
            change.get("values").unwrap_or(&missing_values)
        };
        let values = values
            .as_array()
            .filter(|v| v.len() == target.paragraphs.len() && v.iter().all(Value::is_string))
            .ok_or("기존 문단 수와 입력값 배열을 맞추세요. 문단 안 줄바꿈은 사용할 수 있습니다.")?;
        if action == "clear" && values.iter().any(|v| !v.as_str().unwrap().is_empty()) {
            return Err("비우기에는 빈 문자열만 지정하세요.".into());
        }
        if action == "set" && values.iter().all(|v| v.as_str().unwrap().is_empty()) {
            return Err("전체 비우기는 clear로 명시하세요.".into());
        }
        if action != "preserve" {
            for value in values {
                let value = value.as_str().unwrap();
                check_xml_text(value)?;
                if value.contains(['\r', '\t']) || value.encode_utf16().count() > 1_000_000 {
                    return Err(
                        "입력은 LF 줄바꿈을 사용하며 탭·과도하게 긴 값은 지원하지 않습니다.".into(),
                    );
                }
            }
        }
        let parsed = internal(&index, &target.section);
        for (paragraph_index, &paragraph) in target.paragraphs.iter().enumerate() {
            if !touched.insert(identity(&index, target, paragraph)) {
                return Err("같은 문단·셀에 중복 입력이 있습니다.".into());
            }
            let value = values[paragraph_index].as_str().unwrap();
            if value != target.before[paragraph_index] {
                let section_index = edits
                    .iter()
                    .position(|(name, _)| name == &target.section)
                    .unwrap_or_else(|| {
                        edits.push((target.section.clone(), Vec::new()));
                        edits.len() - 1
                    });
                let p = &parsed.nodes[paragraph];
                edits[section_index].1.push(Edit {
                    start: p.start,
                    end: p.end,
                    value: paragraph_replacement(parsed, paragraph, value)?,
                });
            }
        }
        preview.push(json!({"target":change["target"],"label":change["label"],"source":change["source"],"action":change["action"],"before":before,"after":values}));
    }
    let mut replacements = Vec::new();
    for (section, mut list) in edits {
        let mut xml = internal(&index, &section).xml.clone();
        list.sort_by_key(|e| std::cmp::Reverse(e.start));
        for (index, edit) in list.iter().enumerate() {
            if index > 0 && edit.end > list[index - 1].start {
                return Err("수정 영역이 겹칩니다.".into());
            }
            xml.replace_range(edit.start..edit.end, &edit.value);
        }
        replacements.push((section, xml.into_bytes()));
    }
    let output = patch_zip(&doc.archive, &replacements)?;
    let after = load_hwpx(&output)?;
    let after_index = Lookup::new(&after);
    for item in &preview {
        if find_target(&after_index, item["target"].as_str().unwrap())
            .map(|t| json!(t.before))
            .as_ref()
            != Some(&item["after"])
        {
            return Err(format!(
                "작성 후 값 대조 실패: {}",
                item["label"].as_str().unwrap()
            ));
        }
    }
    for entry in &doc.archive.entries {
        let wanted = replacements
            .iter()
            .find(|(name, _)| name == &entry.name)
            .map_or(entry.data.as_slice(), |(_, data)| data.as_slice());
        if after.archive.read(&entry.name) != Some(wanted) {
            return Err("대상 밖 ZIP 내용 변경이 있습니다.".into());
        }
    }
    let mut expected_structure = doc.structure.clone();
    expected_structure["sourceSha256"] = after.structure["sourceSha256"].clone();
    for section in expected_structure["sections"].as_array_mut().unwrap() {
        let name = section["name"].as_str().unwrap().to_owned();
        for paragraph in section["paragraphs"].as_array_mut().unwrap() {
            let key = paragraph["key"].as_str().unwrap();
            let old = find_target(&index, key).unwrap();
            let node = &internal(&index, &name).nodes[old.node];
            if touched.contains(&(name.clone(), node.start)) {
                paragraph["text"] = find_target(&after_index, key)
                    .and_then(|t| t.before.first())
                    .map_or(Value::Null, |s| json!(s));
            }
        }
    }
    if serde_json::to_string(&expected_structure).map_err(|e| e.to_string())?
        != serde_json::to_string(&after.structure).map_err(|e| e.to_string())?
    {
        return Err("문단·표 구조 또는 대상 밖 글자가 바뀌었습니다.".into());
    }
    let unassigned: Vec<_> = doc
        .targets
        .iter()
        .filter(|t| {
            t.writable
                && (t.kind == "cell"
                    || (t.kind == "paragraph"
                        && internal(&index, &t.section)
                            .ancestor_hp(t.node, "tc")
                            .is_none()))
                && !t
                    .paragraphs
                    .iter()
                    .all(|&p| touched.contains(&identity(&index, t, p)))
        })
        .map(|t| json!({"target":t.key,"before":t.before,"action":"unchanged-not-requested"}))
        .collect();
    let preview = json!(preview);
    let unassigned = json!(unassigned);
    let report = json!({"schemaVersion":1,"sourceSha256":doc.structure["sourceSha256"],"outputSha256":sha256(&output),"changedSections":replacements.iter().map(|(name,_)|name).collect::<Vec<_>>(),"valueCheck":"passed","preservationCheck":"passed","visualStatus":"not-checked","previewStatus":"requires-hangul-save","changes":preview,"unassigned":unassigned});
    Ok(Prepared {
        output,
        preview,
        unassigned,
        report,
    })
}
pub fn verify_hwpx(bytes: &[u8], request: &Value, actual: &[u8]) -> Result<Value, String> {
    let expected = prepare_hwpx(bytes, request)?;
    if actual != expected.output {
        return Err("산출물이 승인된 입력으로 만든 파일과 다릅니다. 한글 재저장본은 별도 화면·내용 검증이 필요합니다.".into());
    }
    Ok(expected.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office_xml::HP;
    use crate::office_zip::create_zip;
    fn fixture(body: &str) -> Vec<u8> {
        create_zip(&[("mimetype".into(),b"application/hwp+zip".to_vec()),("Contents/section0.xml".into(),format!("\u{feff}<?xml version='1.0'?>\r\n<s xmlns:hp='{HP}'><!--keep-->\r\n{body}\r\n</s>").into_bytes()),("BinData/keep.bin".into(),vec![0,255,128,10]),("Preview/PrvText.txt".into(),b"original preview".to_vec())]).unwrap()
    }
    fn paragraph(text: &str) -> String {
        format!("<hp:p><hp:run><hp:t>{text}</hp:t></hp:run></hp:p>")
    }
    fn change(key: &str, before: Value, action: &str, values: Value) -> Value {
        json!({"target":key,"label":"합성 항목","source":"합성 입력 근거","action":action,"expected":before,"values":values})
    }
    fn request(bytes: &[u8], changes: Vec<Value>) -> Value {
        json!({"schemaVersion":1,"sourceSha256":sha256(bytes),"changes":changes})
    }
    #[test]
    fn edits_preserve_other_bytes_escape_text_remove_only_edited_layout_and_verify_exactly() {
        let original = fixture(
            "<hp:p id='first'><hp:run charPrID='9'><hp:t>old 😀</hp:t></hp:run><hp:run charPrID='9'><hp:t/></hp:run><hp:linesegarray><hp:lineseg /></hp:linesegarray></hp:p><hp:p id='second'><hp:run/></hp:p>",
        );
        let wanted = "새<&😀\n끝";
        let req = request(
            &original,
            vec![change(
                "Contents/section0.xml/p0",
                json!(["old 😀"]),
                "set",
                json!([wanted]),
            )],
        );
        let prepared = prepare_hwpx(&original, &req).unwrap();
        let output = load_hwpx(&prepared.output).unwrap();
        let xml = &output.internals[0].1.xml;
        assert!(xml.starts_with("\u{feff}<?xml version='1.0'?>\r\n"));
        assert!(xml.contains("<!--keep-->\r\n"));
        assert!(xml.contains("<hp:t>새&lt;&amp;😀<hp:lineBreak/>끝</hp:t>"));
        assert!(xml.contains("<hp:run charPrID='9'><hp:t></hp:t></hp:run>"));
        assert!(!xml.contains("linesegarray"));
        assert!(xml.contains("<hp:p id='second'><hp:run/></hp:p>"));
        assert_eq!(
            output.archive.read("BinData/keep.bin"),
            Some([0, 255, 128, 10].as_slice())
        );
        assert_eq!(
            output.archive.read("Preview/PrvText.txt"),
            Some(b"original preview".as_slice())
        );
        assert_eq!(prepared.preview[0]["after"], json!([wanted]));
        assert_eq!(
            prepared.unassigned,
            json!([{"target":"Contents/section0.xml/p1","before":[""],"action":"unchanged-not-requested"}])
        );
        assert_eq!(
            verify_hwpx(&original, &req, &prepared.output).unwrap(),
            prepared.report
        );
        let mut changed = prepared.output.clone();
        changed[0] ^= 1;
        assert!(verify_hwpx(&original, &req, &changed).is_err());
        assert!(verify_hwpx(&original, &req, &original).is_err());
    }
    #[test]
    fn empty_runs_clear_and_preserve_keep_original_document_contracts() {
        let original = fixture("<hp:p><hp:run/></hp:p><hp:p><hp:run></hp:run></hp:p>");
        let req = request(
            &original,
            vec![
                change("Contents/section0.xml/p0", json!([""]), "set", json!(["A"])),
                change("Contents/section0.xml/p1", json!([""]), "set", json!(["B"])),
            ],
        );
        let changed = prepare_hwpx(&original, &req).unwrap();
        let loaded = load_hwpx(&changed.output).unwrap();
        assert!(
            loaded.internals[0]
                .1
                .xml
                .contains("<hp:run><hp:t>A</hp:t></hp:run>")
        );
        assert!(
            loaded.internals[0]
                .1
                .xml
                .contains("<hp:run><hp:t>B</hp:t></hp:run>")
        );
        let clear = request(
            &changed.output,
            vec![change(
                "Contents/section0.xml/p0",
                json!(["A"]),
                "clear",
                json!([""]),
            )],
        );
        assert_eq!(
            load_hwpx(&prepare_hwpx(&changed.output, &clear).unwrap().output)
                .unwrap()
                .targets[0]
                .before,
            [""]
        );
        let preserve = request(
            &original,
            vec![
                json!({"target":"Contents/section0.xml/p0","label":"보존","source":"원문 유지","action":"preserve","expected":[""]}),
            ],
        );
        let kept = prepare_hwpx(&original, &preserve).unwrap();
        assert_eq!(kept.output, original);
        assert_eq!(kept.report["changedSections"], json!([]));
        assert_eq!(kept.report["previewStatus"], "requires-hangul-save");
    }
    #[test]
    fn cells_reserve_all_direct_paragraphs_and_protected_cells_allow_only_preserve() {
        let cell = format!(
            "<hp:tbl rowCnt='1' colCnt='1'><hp:tr><hp:tc><hp:cellAddr rowAddr='0' colAddr='0'/><hp:cellSpan rowSpan='1' colSpan='1'/><hp:subList>{}{}</hp:subList></hp:tc></hp:tr></hp:tbl>{}",
            paragraph("A"),
            paragraph("B"),
            paragraph("keep")
        );
        let original = fixture(&cell);
        let cell_change = change(
            "Contents/section0.xml/tbl0/r0c0",
            json!(["A", "B"]),
            "clear",
            json!(["", ""]),
        );
        let prepared =
            prepare_hwpx(&original, &request(&original, vec![cell_change.clone()])).unwrap();
        assert_eq!(prepared.unassigned[0]["target"], "Contents/section0.xml/p2");
        let duplicate = request(
            &original,
            vec![
                cell_change,
                change(
                    "Contents/section0.xml/p0",
                    json!(["A"]),
                    "preserve",
                    json!(["A"]),
                ),
            ],
        );
        assert!(
            prepare_hwpx(&original, &duplicate)
                .err()
                .unwrap()
                .contains("중복 입력")
        );
        let protected = fixture(&cell.replace("<hp:tc>", "<hp:tc protect='1'>"));
        let set = request(
            &protected,
            vec![change(
                "Contents/section0.xml/tbl0/r0c0",
                json!(["A", "B"]),
                "set",
                json!(["X", "Y"]),
            )],
        );
        assert!(
            prepare_hwpx(&protected, &set)
                .err()
                .unwrap()
                .contains("지원하지 않는 입력 위치")
        );
        let preserve = request(
            &protected,
            vec![change(
                "Contents/section0.xml/tbl0/r0c0",
                json!(["A", "B"]),
                "preserve",
                json!(["A", "B"]),
            )],
        );
        assert_eq!(
            prepare_hwpx(&protected, &preserve).unwrap().output,
            protected
        );
    }
    #[test]
    fn validation_keeps_source_expected_required_and_action_gates() {
        let original = fixture(&paragraph("old"));
        let valid = change(
            "Contents/section0.xml/p0",
            json!(["old"]),
            "set",
            json!(["new"]),
        );
        let req = request(&original, vec![valid.clone()]);
        for (key, value) in [
            ("extra", json!(true)),
            ("schemaVersion", json!(2)),
            ("sourceSha256", json!("changed")),
            ("changes", json!([])),
            ("requiredTargets", json!(["missing"])),
            ("requiredTargets", Value::Null),
            ("unmapped", json!(["unknown"])),
            ("unmapped", Value::Null),
        ] {
            let mut bad = req.clone();
            bad[key] = value;
            assert!(prepare_hwpx(&original, &bad).is_err(), "{key}");
        }
        for (key, value) in [
            ("extra", json!(true)),
            ("target", json!("missing")),
            ("expected", json!(["changed"])),
            ("label", json!(" \u{feff}")),
            ("source", Value::Null),
            ("action", json!("other")),
            ("values", json!([])),
            ("values", json!([null])),
            ("values", json!([""])),
            ("values", json!(["bad\tvalue"])),
            ("values", json!(["bad\rvalue"])),
            ("values", json!(["bad\u{1}value"])),
        ] {
            let mut bad = valid.clone();
            bad[key] = value;
            assert!(
                prepare_hwpx(&original, &request(&original, vec![bad])).is_err(),
                "{key}"
            );
        }
        let mut preserve = valid.clone();
        preserve["action"] = json!("preserve");
        assert!(prepare_hwpx(&original, &request(&original, vec![preserve])).is_err());
        let mut clear = valid;
        clear["action"] = json!("clear");
        assert!(prepare_hwpx(&original, &request(&original, vec![clear])).is_err());
    }
    #[test]
    fn length_counts_utf16_and_sections_follow_request_change_order() {
        let original = fixture(&paragraph("old"));
        let allowed = "😀".repeat(250_001);
        let req = request(
            &original,
            vec![change(
                "Contents/section0.xml/p0",
                json!(["old"]),
                "set",
                json!([allowed]),
            )],
        );
        assert!(prepare_hwpx(&original, &req).is_ok());
        let long = "😀".repeat(500_001);
        let req = request(
            &original,
            vec![change(
                "Contents/section0.xml/p0",
                json!(["old"]),
                "set",
                json!([long]),
            )],
        );
        assert!(
            prepare_hwpx(&original, &req)
                .err()
                .unwrap()
                .contains("과도하게 긴 값")
        );
        let xml = format!("<s xmlns:hp='{HP}'>{}</s>", paragraph("old"));
        let original = create_zip(&[
            ("mimetype".into(), b"application/hwp+zip".to_vec()),
            ("Contents/section0.xml".into(), xml.as_bytes().to_vec()),
            ("Contents/section2.xml".into(), xml.into_bytes()),
        ])
        .unwrap();
        let req = request(
            &original,
            vec![
                change(
                    "Contents/section2.xml/p0",
                    json!(["old"]),
                    "set",
                    json!(["second first"]),
                ),
                change(
                    "Contents/section0.xml/p0",
                    json!(["old"]),
                    "set",
                    json!(["first second"]),
                ),
            ],
        );
        assert_eq!(
            prepare_hwpx(&original, &req).unwrap().report["changedSections"],
            json!(["Contents/section2.xml", "Contents/section0.xml"])
        );
    }
    #[test]
    fn overlapping_ranges_and_structure_loss_fail_before_publication() {
        let original = fixture(
            "<hp:p><hp:run><hp:t><hp:lineBreak><hp:p><hp:run><hp:t>nested</hp:t></hp:run></hp:p></hp:lineBreak></hp:t></hp:run></hp:p>",
        );
        let outer = change(
            "Contents/section0.xml/p0",
            json!(["\n"]),
            "set",
            json!(["outer"]),
        );
        let inner = change(
            "Contents/section0.xml/p1",
            json!(["nested"]),
            "set",
            json!(["inner"]),
        );
        assert!(
            prepare_hwpx(&original, &request(&original, vec![outer.clone(), inner]))
                .err()
                .unwrap()
                .contains("수정 영역이 겹칩니다")
        );
        assert!(
            prepare_hwpx(&original, &request(&original, vec![outer]))
                .err()
                .unwrap()
                .contains("문단·표 구조")
        );
    }
}
