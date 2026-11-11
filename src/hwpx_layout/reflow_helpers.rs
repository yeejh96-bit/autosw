//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Ports render/reflow-cache, reflow-borders and reflow-fragments.
//! See data/kordoc-LICENSE, data/kordoc-NOTICE, data/kordoc-PROVENANCE.json.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    layout::{maximum, to_int32},
    numbering::{add, nullish, truth},
    style_dom::ParsedXml,
    value::{Value, native_json as json, number, string},
};

pub fn cached_paragraph_bottom(
    doc: &ParsedXml,
    node: usize,
    body_flow: bool,
) -> Result<f64, String> {
    let node = doc.nodes.get(node).ok_or("줄 정보 XML 노드가 없습니다.")?;
    let mut bottom = f64::NEG_INFINITY;
    let mut previous = f64::NEG_INFINITY;
    for &child in &node.children {
        let seg = &doc.nodes[child];
        if seg.local != "lineseg" {
            continue;
        }
        let num = |name, fallback| match seg.attr(name) {
            None | Some("") => fallback,
            Some(value) => to_int32(number(&value.into()), fallback),
        };
        let v = num("vertpos", 0.0);
        if body_flow && v < previous {
            bottom = f64::NEG_INFINITY;
        }
        bottom = maximum(
            bottom,
            v + maximum(num("vertsize", 1000.0), num("textheight", 1000.0)) + num("spacing", 0.0),
        );
        previous = v;
    }
    Ok(bottom)
}
fn map_get<'a>(map: &'a Value, key: &Value) -> Result<&'a Value, String> {
    match map {
        Value::Array(entries) => Ok(entries
            .iter()
            .find(|entry| match (&entry[0], key) {
                (Value::Number(a), Value::Number(b)) => a == b || a.is_nan() && b.is_nan(),
                (a, b) => a == b,
            })
            .map(|entry| &entry[1])
            .unwrap_or(&Value::Undefined)),
        Value::Object(values) => Ok(key
            .as_str()
            .and_then(|key| values.get(key))
            .unwrap_or(&Value::Undefined)),
        _ => Err("서식 Map이 없습니다.".into()),
    }
}
fn array_get<'a>(value: &'a Value, index: &Value) -> &'a Value {
    if let Some(array) = value.as_array() {
        let text = string(index);
        if let Ok(index) = text.parse::<usize>()
            && index.to_string() == text
        {
            return array.get(index).unwrap_or(&Value::Undefined);
        }
        &Value::Undefined
    } else if let Some(object) = value.as_object() {
        object.get(&string(index)).unwrap_or(&Value::Undefined)
    } else {
        &Value::Undefined
    }
}
fn checked_index<'a>(value: &'a Value, index: &Value) -> Result<&'a Value, String> {
    if nullish(value) {
        Err("줄 계산 배열이 없습니다.".into())
    } else {
        Ok(array_get(value, index))
    }
}
fn filtered_parts(free: &Value) -> Result<Vec<&Value>, String> {
    let rows = free.as_array().ok_or("빈 구간 배열이 없습니다.")?;
    let mut parts = Vec::new();
    for row in rows {
        if !row.is_array() && !row.is_string() {
            return Err("빈 구간 행을 펼칠 수 없습니다.".into());
        }
        if number(&row[1]) - number(&row[0]) >= 1440.0 {
            parts.push(row);
        }
    }
    Ok(parts)
}
fn lt(a: &Value, b: &Value) -> bool {
    if let (Value::String(a), Value::String(b)) = (a, b) {
        a.encode_utf16().cmp(b.encode_utf16()).is_lt()
    } else {
        number(a) < number(b)
    }
}
fn le(a: &Value, b: &Value) -> bool {
    if let (Value::String(a), Value::String(b)) = (a, b) {
        !a.encode_utf16().cmp(b.encode_utf16()).is_gt()
    } else {
        number(a) <= number(b)
    }
}
fn increment(value: &mut Value) {
    *value = (number(value) + 1.0).into();
}
fn guard(count: &mut usize) -> Result<(), String> {
    *count += 1;
    if *count > 1_048_576 {
        Err("줄 계산 반복이 처리 한도를 넘습니다.".into())
    } else {
        Ok(())
    }
}
pub fn line_border_box(
    model: &Value,
    styles: &Value,
    start: &Value,
    end: &Value,
) -> Result<Value, String> {
    let chars = model["chars"].as_array().ok_or("문자 배열이 없습니다.")?;
    let objs = model["objs"].as_array().ok_or("개체 배열이 없습니다.")?;
    let empty = chars.is_empty() && objs.is_empty();
    let mut top = 0.0;
    let mut all = 0.0;
    let mut include = |pr_id: &Value| -> Result<(), String> {
        let st = if nullish(pr_id) {
            &Value::Undefined
        } else {
            map_get(&styles["charPr"], pr_id)?
        };
        let fill = if nullish(&st["borderFill"]) {
            &Value::Undefined
        } else {
            map_get(&styles["borderFill"], &st["borderFill"])?
        };
        if !truth(fill) || !truth(&fill["top"]) && !truth(&fill["bottom"]) {
            return Ok(());
        }
        let width = |pt: &Value| {
            if empty {
                2.0 * (number(pt) * 50.0).trunc()
            } else {
                (number(pt) * 100.0).trunc()
            }
        };
        let t = if truth(&fill["top"]) {
            width(&fill["top"]["widthPt"])
        } else {
            0.0
        };
        let b = if truth(&fill["bottom"]) {
            width(&fill["bottom"]["widthPt"])
        } else {
            0.0
        };
        let height = add(&st["height"], &json!(t));
        top = maximum(top, number(&height));
        all = maximum(all, number(&add(&height, &json!(b))));
        Ok(())
    };
    if empty {
        for run in model["runs"].as_array().ok_or("run 배열이 없습니다.")? {
            if nullish(run) {
                return Err("문단 run 항목이 없습니다.".into());
            }
            include(&run["prId"])?
        }
    } else {
        let mut i = start.clone();
        let mut count = 0;
        while lt(&i, end) {
            guard(&mut count)?;
            let ch = array_get(&model["chars"], &i);
            if nullish(ch) {
                return Err("문자 배열 범위를 벗어났습니다.".into());
            }
            if truth(&ch["ch"]) {
                include(&ch["prId"])?
            }
            increment(&mut i);
        }
    }
    Ok(json!([top, all, empty && all > 0.0]))
}
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    pub offset: Value,
    pub x: Value,
    pub width: Value,
    /// None leaves the shallow clone's original textpos attribute untouched.
    pub textpos: Option<Value>,
}
fn text_index(text: &[u16], index: &Value) -> Option<u16> {
    let key = string(index);
    let index = key.parse::<usize>().ok()?;
    if index.to_string() != key {
        return None;
    }
    text.get(index).copied()
}
fn slice_index(value: &Value, len: usize) -> usize {
    let n = number(value);
    if n.is_nan() || n == 0.0 {
        return 0;
    }
    if n < 0.0 {
        (len as f64 + n.trunc()).max(0.0) as usize
    } else {
        n.trunc().min(len as f64) as usize
    }
}
fn text_slice<'a>(text: &'a [u16], start: &Value, end: &Value) -> &'a [u16] {
    let start = slice_index(start, text.len());
    let end = if end.is_undefined() {
        text.len()
    } else {
        slice_index(end, text.len())
    };
    &text[start..end.max(start)]
}
fn nullish_zero(value: &Value) -> Value {
    if nullish(value) {
        json!(0)
    } else {
        value.clone()
    }
}
pub fn narrow_line_fragments(
    free: &Value,
    box_value: &Value,
    flow: &Value,
) -> Result<Option<Vec<Fragment>>, String> {
    if !truth(free) || !truth(box_value) {
        return Ok(None);
    }
    let parts = filtered_parts(free)?;
    if parts.len() < 2 {
        return Ok(None);
    }
    let last = parts[parts.len() - 1];
    let last_width = Value::Number(number(&last[1]) - number(&box_value[0]));
    if last[0] != box_value[0] || last_width != box_value[1] {
        return Ok(None);
    }
    let text = flow["text"].as_str().ok_or("줄 문자열이 없습니다.")?;
    narrow_line_fragments_utf16(
        free,
        box_value,
        flow,
        &text.encode_utf16().collect::<Vec<_>>(),
    )
}
/// Pure calculation: no DOM mutation occurs until the caller accepts all parts.
pub fn narrow_line_fragments_utf16(
    free: &Value,
    box_value: &Value,
    flow: &Value,
    text: &[u16],
) -> Result<Option<Vec<Fragment>>, String> {
    if !truth(free) || !truth(box_value) {
        return Ok(None);
    }
    let parts = filtered_parts(free)?;
    if parts.len() < 2 {
        return Ok(None);
    }
    let last = parts[parts.len() - 1];
    let last_width = Value::Number(number(&last[1]) - number(&box_value[0]));
    if last[0] != box_value[0] || last_width != box_value[1] {
        return Ok(None);
    }
    let start = &flow["start"];
    let end = &flow["end"];
    if text_slice(text, start, end)
        .iter()
        .any(|ch| matches!(ch, 9 | 10 | 0x00a0 | 0x2005 | 0xfdd0 | 0xfffc))
    {
        return Ok(None);
    }
    let widths = &flow["widths"];
    let spacings = &flow["spacings"];
    let mut ink = start.clone();
    let mut count = 0;
    while lt(&ink, end)
        && (text_index(text, &ink) == Some(32)
            || text_index(text, &ink) == Some(0x2060) && checked_index(widths, &ink)? == &json!(0))
    {
        guard(&mut count)?;
        increment(&mut ink);
    }
    if text_slice(text, &ink, end).contains(&0x2060) {
        return Ok(None);
    }
    if ink == *end {
        return Ok(None);
    }
    let first_width = number(&nullish_zero(checked_index(widths, &ink)?))
        - number(&nullish_zero(checked_index(spacings, &ink)?));
    if parts[..parts.len() - 1]
        .iter()
        .any(|part| first_width <= number(&part[1]) - number(&part[0]))
    {
        return Ok(None);
    }
    let mut fragments = Vec::new();
    let mut cursor = start.clone();
    for part in &parts[..parts.len() - 1] {
        let offset = cursor.clone();
        let mut w = json!(0);
        let width = number(&part[1]) - number(&part[0]);
        while lt(&cursor, &ink) && le(&add(&w, checked_index(widths, &cursor)?), &json!(width)) {
            guard(&mut count)?;
            w = add(&w, checked_index(widths, &cursor)?);
            increment(&mut cursor);
        }
        fragments.push(Fragment {
            offset,
            x: part[0].clone(),
            width: width.into(),
            textpos: None,
        });
    }
    let mut rest = json!(0);
    let mut i = cursor.clone();
    while lt(&i, end) {
        guard(&mut count)?;
        rest = add(&rest, checked_index(widths, &i)?);
        increment(&mut i);
    }
    let mut last = json!(number(end) - 1.0);
    if lt(&cursor, &last)
        && text_index(text, &last).is_some_and(|ch| (0xdc00..=0xdfff).contains(&ch))
    {
        last = (number(&last) - 1.0).into();
    }
    let rest = number(&rest) - number(&nullish_zero(checked_index(spacings, &last)?));
    if lt(&add(&box_value[1], &json!(0.5)), &json!(rest)) {
        return Ok(None);
    }
    let mut next = end.clone();
    while lt(&next, &json!(text.len())) && text_index(text, &next) == Some(32) {
        guard(&mut count)?;
        increment(&mut next);
    }
    if lt(&next, &json!(text.len())) {
        let with_next = number(&add(
            &json!(rest),
            &nullish_zero(checked_index(widths, &next)?),
        )) - number(&nullish_zero(checked_index(spacings, &next)?));
        if le(&json!(with_next), &add(&box_value[1], &json!(0.5))) {
            return Ok(None);
        }
    }
    fragments.push(Fragment {
        offset: cursor,
        x: box_value[0].clone(),
        width: box_value[1].clone(),
        textpos: None,
    });
    for (i, fragment) in fragments.iter_mut().enumerate() {
        if i > 0 {
            fragment.textpos = Some(checked_index(&flow["slots"], &fragment.offset)?.clone());
        }
    }
    Ok(Some(fragments))
}
#[cfg(test)]
fn numeric_report(n: f64) -> Value {
    json!({"value":n,"isNaN":n.is_nan(),"positiveInfinity":n==f64::INFINITY,"negativeInfinity":n==f64::NEG_INFINITY,"negativeZero":n==0.0&&n.is_sign_negative()})
}
#[cfg(test)]
fn attributes(node: roxmltree::Node<'_, '_>, xml: &str) -> Vec<Value> {
    super::style_dom::ordered_attributes(node, xml)
        .into_iter()
        .map(|(name, value)| json!([name, value]))
        .collect()
}
#[cfg(test)]
fn raw_text_reports(mut raw: &str, out: &mut Vec<Value>) -> Result<(), String> {
    while !raw.is_empty() {
        if let Some(cdata) = raw.strip_prefix("<![CDATA[") {
            let end = cdata.find("]]>").ok_or("CDATA 끝이 없습니다.")?;
            if end != 0 {
                out.push(json!({"type":4,"text":&cdata[..end]}));
            }
            raw = &cdata[end + 3..];
        } else {
            let end = raw.find("<![CDATA[").unwrap_or(raw.len());
            // The source was already validated. Decode character references
            // for this single ordinary-text node, without merging its CDATA
            // neighbours or normalizing numeric CR references a second time.
            let wrapped = format!("<text>{}</text>", &raw[..end]);
            let parsed = roxmltree::Document::parse(&wrapped).map_err(|e| e.to_string())?;
            let text = parsed.root_element().text().unwrap_or("");
            if let Some(previous) = out.last_mut().filter(|v| v["type"] == 3) {
                // xmldom's document normalization rejoins ordinary text on
                // either side of a discarded empty CDATA section.
                if let Value::String(previous_text) = &mut previous["text"] {
                    previous_text.push_str(text);
                }
            } else {
                out.push(json!({"type":3,"text":text}));
            }
            raw = &raw[end..];
        }
    }
    Ok(())
}
#[cfg(test)]
fn child_reports(node: roxmltree::Node<'_, '_>, xml: &str) -> Result<Vec<Value>, String> {
    let range = node.range();
    let source = &xml[range.clone()];
    let mut quote = None;
    let mut start = None;
    for (index, byte) in source.bytes().enumerate() {
        if let Some(q) = quote {
            if byte == q {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'>' {
            start = Some(index + 1);
            break;
        }
    }
    let start = start.ok_or("XML 시작 태그 끝이 없습니다.")?;
    if source.as_bytes().get(start.saturating_sub(2)) == Some(&b'/') {
        return Ok(Vec::new());
    }
    let end = range.start + source.rfind("</").ok_or("XML 끝 태그가 없습니다.")?;
    let mut cursor = range.start + start;
    let mut result = Vec::new();
    // roxmltree merges ordinary text and CDATA. Its non-text child ranges
    // still delimit the exact source gaps, including empty CDATA sections.
    for child in node.children().filter(|child| !child.is_text()) {
        let child_range = child.range();
        raw_text_reports(&xml[cursor..child_range.start], &mut result)?;
        result.push(child_report(child, xml)?);
        cursor = child_range.end;
    }
    raw_text_reports(&xml[cursor..end], &mut result)?;
    Ok(result)
}
#[cfg(test)]
fn child_report(node: roxmltree::Node<'_, '_>, xml: &str) -> Result<Value, String> {
    if node.is_element() {
        let name = &xml[node.range().start + 1..]
            .split(|ch: char| ch.is_ascii_whitespace() || ch == '/' || ch == '>')
            .next()
            .unwrap_or("");
        Ok(
            json!({"type":1,"name":name,"attrs":attributes(node,xml),"children":child_reports(node,xml)?}),
        )
    } else if node.is_comment() {
        Ok(json!({"type":8,"text":node.text().unwrap_or("")}))
    } else if node.is_pi() {
        let pi = node.pi().unwrap();
        Ok(json!({"type":7,"name":pi.target,"text":pi.value.unwrap_or("")}))
    } else {
        Ok(json!({"type":3,"text":node.text().unwrap_or("")}))
    }
}
#[cfg(test)]
fn fragment_report(input: &Value, fragments: Option<Vec<Fragment>>) -> Result<Value, String> {
    let seg_xml = input["segXml"].as_str().unwrap_or("<lineseg/>");
    let parent_xml = input["parentXml"].as_str().unwrap_or("<parent/>");
    let normalized_seg = super::style_dom::normalize_xml_lines(seg_xml);
    let normalized_parent = super::style_dom::normalize_xml_lines(parent_xml);
    let seg_xml = normalized_seg.as_ref();
    let parent_xml = normalized_parent.as_ref();
    parse_style_xml(seg_xml)?;
    parse_style_xml(parent_xml)?;
    let seg = roxmltree::Document::parse(seg_xml).map_err(|e| e.to_string())?;
    let parent = roxmltree::Document::parse(parent_xml).map_err(|e| e.to_string())?;
    let seg = seg.root_element();
    let original = child_report(seg, seg_xml)?;
    let mut children = child_reports(parent.root_element(), parent_xml)?;
    let accepted = fragments.is_some();
    if let Some(fragments) = fragments {
        for fragment in fragments {
            let mut attrs = attributes(seg, seg_xml);
            let mut set = |key: &str, value: &Value| {
                if let Some(entry) = attrs.iter_mut().find(|entry| entry[0] == key) {
                    entry[1] = string(value).into();
                } else {
                    attrs.push(json!([key, string(value)]));
                }
            };
            if let Some(textpos) = fragment.textpos {
                set("textpos", &textpos)
            }
            set("horzpos", &fragment.x);
            set("horzsize", &fragment.width);
            children.push(json!({"type":1,"name":original["name"],"attrs":attrs,"children":[]}));
        }
    }
    Ok(json!({"accepted":accepted,"children":children,"source":original}))
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut input = Value::from_json(input);
    // Probe-only nonfinite fixtures preserve values beyond JSON's number range.
    if let Some(patches) = input["nativeNumbers"].as_array().cloned() {
        for patch in patches {
            let path = patch["path"]
                .as_array()
                .ok_or("수치 fixture 경로가 없습니다.")?;
            let value = match patch["value"].as_str() {
                Some("NaN") => f64::NAN,
                Some("Infinity") => f64::INFINITY,
                Some("-Infinity") => f64::NEG_INFINITY,
                Some("-0") => -0.0,
                _ => return Err("수치 fixture 종류가 잘못되었습니다.".into()),
            };
            let mut target = &mut input;
            for key in path {
                target = if let Some(index) = key.as_u64() {
                    target
                        .get_mut(index as usize)
                        .ok_or("수치 fixture 항목이 없습니다.")?
                } else {
                    target
                        .get_mut(key.as_str().ok_or("수치 fixture 키가 잘못되었습니다.")?)
                        .ok_or("수치 fixture 항목이 없습니다.")?
                };
            }
            *target = Value::Number(value);
        }
    }
    let value = match operation {
        "cachedParagraphBottom" => {
            let doc = parse_style_xml(input["xml"].as_str().ok_or("XML 문자열이 없습니다.")?)?;
            numeric_report(cached_paragraph_bottom(
                &doc,
                doc.root,
                truth(&input["bodyFlow"]),
            )?)
        }
        "lineBorderBox" => {
            let result = line_border_box(
                &input["model"],
                &input["styles"],
                &input["start"],
                &input["end"],
            )?;
            json!({"value":result,"top":numeric_report(number(&result[0])),"all":numeric_report(number(&result[1]))})
        }
        "appendNarrowLineFragments" => {
            let fragments = if let Some(units) = input["textUtf16"].as_array() {
                let text = units
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .filter(|v| *v <= 65535)
                            .map(|v| v as u16)
                            .ok_or("잘못된 UTF16 단위입니다.")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                narrow_line_fragments_utf16(&input["free"], &input["box"], &input["flow"], &text)?
            } else {
                narrow_line_fragments(&input["free"], &input["box"], &input["flow"])?
            };
            fragment_report(&input, fragments)?
        }
        _ => return Err("알 수 없는 줄 보조 계산입니다.".into()),
    };
    Ok(value.to_json())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_rows_and_missing_arrays_keep_lazy_guard_order() {
        let flow = json!({"text":"  가나","widths":Value::Null,"spacings":[],"slots":[],"start":0,"end":3});
        let free = json!([[0, 1440], [2000, 4000]]);
        let box_value = json!([2000, 2000]);
        assert!(narrow_line_fragments(&free, &box_value, &flow).is_err());
        assert!(narrow_line_fragments(&json!([Value::Null]), &box_value, &flow).is_err());
        assert!(
            narrow_line_fragments(&Value::Null, &box_value, &Value::Null)
                .unwrap()
                .is_none()
        );
        let mut count = 1_048_576;
        assert!(guard(&mut count).is_err());
    }
    #[test]
    fn cache_only_uses_direct_lines_and_rewinds_body_flow() {
        let doc=parse_style_xml("<lsa><lineseg vertpos='5000'/><wrap><lineseg vertpos='99999'/></wrap><lineseg vertpos='100' textheight='1200' spacing='10'/></lsa>").unwrap();
        assert_eq!(
            cached_paragraph_bottom(&doc, doc.root, false).unwrap(),
            6000.0
        );
        assert_eq!(
            cached_paragraph_bottom(&doc, doc.root, true).unwrap(),
            1310.0
        );
        let doc = parse_style_xml("<lsa/>").unwrap();
        assert_eq!(
            cached_paragraph_bottom(&doc, doc.root, true).unwrap(),
            f64::NEG_INFINITY
        );
    }
    #[test]
    fn border_empty_rounding_and_native_nan_survive() {
        let model = json!({"chars":[],"objs":[],"runs":[{"prId":"a"}]});
        let mut styles = json!({"charPr":[["a",{"height":1000,"borderFill":"b"}]],"borderFill":[["b",{"top":{"widthPt":0.019},"bottom":{"widthPt":0.029}}]]});
        assert_eq!(
            line_border_box(&model, &styles, &json!(0), &json!(0)).unwrap(),
            json!([1000, 1002, true])
        );
        styles["charPr"][0][1]["height"] = json!(f64::NAN);
        assert!(
            line_border_box(&model, &styles, &json!(0), &json!(0)).unwrap()[0]
                .as_f64()
                .unwrap()
                .is_nan()
        );
    }
    #[test]
    fn fragment_offsets_are_utf16_and_clones_wait_until_complete() {
        let flow = json!({"text":"  😀","widths":[100,100,2000,0],"spacings":[0,0,300,0],"slots":[0,1,2,3],"start":0,"end":4});
        let fragments = narrow_line_fragments(
            &json!([[0, 1440], [2000, 4000]]),
            &json!([2000, 2000]),
            &flow,
        )
        .unwrap()
        .unwrap();
        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[1].offset, 2);
        assert_eq!(fragments[1].textpos, Some(json!(2)));
        assert!(
            narrow_line_fragments(
                &json!([[0, 1440], [2000, 3440]]),
                &json!([2000, 1440]),
                &flow
            )
            .unwrap()
            .is_none()
        );
    }
}
