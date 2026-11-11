//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Ports hwpx/notes.mjs. See data/kordoc-LICENSE, data/kordoc-NOTICE and
//! data/kordoc-PROVENANCE.json. XML lookups preserve qualified-name precedence.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    equation::EquationText,
    numbering::{self, add, code_point, nullish, parse_int, truth},
    style_dom::ParsedXml,
    value::{Value, native_json as json, string},
};
/// JS String.fromCodePoint also accepts isolated surrogate code units. Keep
/// those units intact until the actual file-writing boundary.
pub fn code_point_attr_utf16(value: &Value) -> Option<EquationText> {
    if nullish(value) || value == "" {
        return None;
    }
    let n = parse_int(value);
    if !n.is_finite() || !(0.0..=0x10ffff as f64).contains(&n) {
        return None;
    }
    let n = n as u32;
    let units = if n == 0 {
        vec![]
    } else if n <= 0xffff {
        vec![n as u16]
    } else {
        vec![
            0xd800 + ((n - 0x10000) >> 10) as u16,
            0xdc00 + ((n - 0x10000) & 0x3ff) as u16,
        ]
    };
    Some(EquationText { units })
}
enum NoteValue {
    Native(Value),
    Text(Vec<u16>),
}
impl NoteValue {
    fn units(&self) -> Vec<u16> {
        match self {
            Self::Native(value) => string(value).encode_utf16().collect(),
            Self::Text(units) => units.clone(),
        }
    }
    fn plus(self, other: Self) -> Self {
        match (&self, &other) {
            (Self::Native(left), Self::Native(right)) => Self::Native(add(left, right)),
            _ => {
                let mut units = self.units();
                units.extend(other.units());
                Self::Text(units)
            }
        }
    }
}
pub fn note_ref_mark_utf16(attrs: &Value, fmt: Option<&Value>) -> Result<EquationText, String> {
    let default = json!({"type":"DIGIT","userChar":"","prefixChar":"","suffixChar":")"});
    let fmt = fmt.filter(|value| !nullish(value)).unwrap_or(&default);
    let field = |key: &str| {
        code_point_attr_utf16(&attrs[key])
            .map(|text| NoteValue::Text(text.units))
            .unwrap_or_else(|| NoteValue::Native(fmt[key].clone()))
    };
    let n = parse_int(&attrs["number"]);
    let core = if fmt["type"] == "USER_CHAR" {
        code_point_attr_utf16(&attrs["userChar"])
            .map(|text| NoteValue::Text(text.units))
            .unwrap_or_else(|| NoteValue::Native(fmt["userChar"].clone()))
    } else {
        NoteValue::Native(note_core(
            &json!(if n.is_finite() { n } else { 1.0 }),
            fmt,
            None,
        )?)
    };
    Ok(EquationText {
        units: field("prefixChar")
            .plus(core)
            .plus(field("suffixChar"))
            .units(),
    })
}
fn child(doc: &ParsedXml, node: usize, name: &str) -> Option<usize> {
    doc.nodes
        .get(node)?
        .children
        .iter()
        .copied()
        .find(|&child| doc.nodes[child].local == name)
}
fn attribute(doc: &ParsedXml, node: Option<usize>, name: &str) -> Value {
    node.and_then(|node| doc.nodes.get(node))
        .and_then(|node| node.attr(name))
        .map(Value::from)
        .unwrap_or(Value::Null)
}
fn code_point_attr(value: &Value) -> Result<Option<Value>, String> {
    if nullish(value) || value == "" {
        return Ok(None);
    }
    let n = parse_int(value);
    if !n.is_finite() || !(0.0..=0x10ffff as f64).contains(&n) {
        return Ok(None);
    }
    if n == 0.0 {
        Ok(Some("".into()))
    } else {
        code_point(n).map(Some)
    }
}
pub fn note_format_from(mut get: impl FnMut(&str) -> Value) -> Value {
    let typ = get("type");
    let field = |value: Value| if nullish(&value) { "".into() } else { value };
    json!({"type":if truth(&typ){typ}else{"DIGIT".into()},"userChar":field(get("userChar")),"prefixChar":field(get("prefixChar")),"suffixChar":field(get("suffixChar"))})
}
fn note_core(num: &Value, fmt: &Value, user_char: Option<&Value>) -> Result<Value, String> {
    if fmt["type"] == "USER_CHAR" {
        Ok(user_char
            .filter(|value| !nullish(value))
            .unwrap_or(&fmt["userChar"])
            .clone())
    } else {
        numbering::format_head_number(num, fmt["type"].as_str().unwrap_or(""))
    }
}
/// Dynamic form also retains the original + coercion for direct API callers.
/// Actual XML-backed note fields are strings and use note_ref_mark below.
pub fn note_ref_value(attrs: &Value, fmt: Option<&Value>) -> Result<Value, String> {
    let default = json!({"type":"DIGIT","userChar":"","prefixChar":"","suffixChar":")"});
    let fmt = fmt.filter(|value| !nullish(value)).unwrap_or(&default);
    let n = parse_int(&attrs["number"]);
    let prefix =
        code_point_attr(&attrs["prefixChar"])?.unwrap_or_else(|| fmt["prefixChar"].clone());
    let suffix =
        code_point_attr(&attrs["suffixChar"])?.unwrap_or_else(|| fmt["suffixChar"].clone());
    let user_char = code_point_attr(&attrs["userChar"])?;
    let core = note_core(
        &json!(if n.is_finite() { n } else { 1.0 }),
        fmt,
        user_char.as_ref(),
    )?;
    Ok(add(&add(&prefix, &core), &suffix))
}
pub fn note_ref_mark(attrs: &Value, fmt: Option<&Value>) -> Result<String, String> {
    note_ref_value(attrs, fmt).map(|value| string(&value))
}
pub fn note_auto_num_value(num: Option<&Value>, fmt: &Value) -> Result<Value, String> {
    let n = parse_int(num.unwrap_or(&Value::Undefined));
    let core = note_core(&json!(if n.is_finite() { n } else { 1.0 }), fmt, None)?;
    Ok(add(&add(&fmt["prefixChar"], &core), &fmt["suffixChar"]))
}
pub fn note_auto_num_text(num: Option<&Value>, fmt: &Value) -> Result<String, String> {
    note_auto_num_value(num, fmt).map(|value| string(&value))
}
pub fn is_rendered_auto_num(num_type: Option<&str>) -> bool {
    matches!(
        num_type,
        Some("FOOTNOTE" | "ENDNOTE" | "PICTURE" | "TABLE" | "EQUATION")
    )
}
pub fn note_attrs_of(doc: &ParsedXml, node: usize) -> Value {
    json!({"number":attribute(doc,Some(node),"number"),"prefixChar":attribute(doc,Some(node),"prefixChar"),"suffixChar":attribute(doc,Some(node),"suffixChar"),"userChar":attribute(doc,Some(node),"userChar")})
}
fn descendant_named(doc: &ParsedXml, node: usize, name: &str) -> Option<usize> {
    for &child in &doc.nodes.get(node)?.children {
        if doc.nodes[child].name == name {
            return Some(child);
        }
        if let Some(found) = descendant_named(doc, child, name) {
            return Some(found);
        }
    }
    None
}
pub fn read_section_note_formats(doc: &ParsedXml, root: usize) -> Value {
    let pick = |names: [&str; 2]| {
        for name in names {
            if let Some(found) = descendant_named(doc, root, name) {
                return child(doc, found, "autoNumFormat")
                    .map(|format| note_format_from(|name| attribute(doc, Some(format), name)))
                    .unwrap_or(Value::Undefined);
            }
        }
        Value::Undefined
    };
    json!({"footnote":pick(["hp:footNotePr","footNotePr"]),"endnote":pick(["hp:endNotePr","endNotePr"])})
}
pub fn note_auto_num_of(doc: &ParsedXml, node: usize) -> Result<String, String> {
    let element = doc
        .nodes
        .get(node)
        .ok_or("자동 번호 XML 노드가 없습니다.")?;
    if !is_rendered_auto_num(element.attr("numType")) {
        return Ok(String::new());
    }
    let format = child(doc, node, "autoNumFormat");
    let fmt = note_format_from(|name| attribute(doc, format, name));
    note_auto_num_text(Some(&attribute(doc, Some(node), "num")), &fmt)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    if operation == "noteRefMarkUtf16" || operation == "codePointAttrUtf16" {
        let text = if operation == "noteRefMarkUtf16" {
            Some(note_ref_mark_utf16(&input["attrs"], input.get("fmt"))?)
        } else {
            code_point_attr_utf16(&input["value"])
        };
        return Ok(text
            .map(|text| serde_json::json!({"text":text.text(),"utf16":text.units}))
            .unwrap_or(serde_json::Value::Null));
    }
    let value = match operation {
        "noteFormatFrom" => note_format_from(|name| input["attrs"][name].clone()),
        "noteRefMark" => note_ref_value(&input["attrs"], input.get("fmt"))?,
        "noteAutoNumText" => note_auto_num_value(input.get("num"), &input["fmt"])?,
        "isRenderedAutoNum" => json!(is_rendered_auto_num(input["numType"].as_str())),
        "noteAttrsOf" | "readSectionNoteFormats" | "noteAutoNumOf" => {
            let doc = parse_style_xml(input["xml"].as_str().ok_or("XML 문자열이 없습니다.")?)?;
            match operation {
                "noteAttrsOf" => note_attrs_of(&doc, doc.root),
                "readSectionNoteFormats" => read_section_note_formats(&doc, doc.root),
                _ => json!(note_auto_num_of(&doc, doc.root)?),
            }
        }
        _ => return Err("알 수 없는 각주 번호 연산입니다.".into()),
    };
    Ok(value.to_json())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn note_defaults_overrides_and_zero_codepoints_are_preserved() {
        assert_eq!(note_ref_mark(&json!({}), None).unwrap(), "1)");
        assert_eq!(
            note_ref_mark(
                &json!({"number":"  -2x","prefixChar":"40","suffixChar":"41"}),
                None
            )
            .unwrap(),
            "(1)"
        );
        let fmt = json!({"type":"USER_CHAR","userChar":"※","prefixChar":"[","suffixChar":"]"});
        assert_eq!(
            note_ref_mark(
                &json!({"number":"2","userChar":"0","prefixChar":"0"}),
                Some(&fmt)
            )
            .unwrap(),
            "]"
        );
        assert_eq!(
            note_ref_mark(&json!({"userChar":"128512"}), Some(&fmt)).unwrap(),
            "[😀]"
        );
        assert!(note_ref_mark(&json!({"prefixChar":"55296"}), None).is_err());
        let exact =
            note_ref_mark_utf16(&json!({"prefixChar":"55296","suffixChar":"57343"}), None).unwrap();
        assert_eq!(exact.units, vec![0xd800, 49, 0xdfff]);
        assert!(exact.text().is_none());
    }
    #[test]
    fn qualified_name_priority_and_first_missing_format_do_not_fall_through() {
        let doc=parse_style_xml("<s xmlns:hp='urn:p' xmlns:q='urn:p'><footNotePr><autoNumFormat suffixChar='U'/></footNotePr><q:footNotePr><autoNumFormat suffixChar='Q'/></q:footNotePr><hp:footNotePr><autoNumFormat suffixChar='H'/></hp:footNotePr><endNotePr><autoNumFormat type='ROMAN_SMALL'/></endNotePr></s>").unwrap();
        let formats = read_section_note_formats(&doc, doc.root);
        assert_eq!(formats["footnote"]["suffixChar"], "H");
        assert_eq!(formats["endnote"]["type"], "ROMAN_SMALL");
        let doc = parse_style_xml(
            "<s xmlns:hp='urn:p'><footNotePr><autoNumFormat/></footNotePr><hp:footNotePr/></s>",
        )
        .unwrap();
        assert!(read_section_note_formats(&doc, doc.root)["footnote"].is_undefined());
        let doc = parse_style_xml("<footNotePr><autoNumFormat/></footNotePr>").unwrap();
        assert!(read_section_note_formats(&doc, doc.root)["footnote"].is_undefined());
    }
    #[test]
    fn auto_numbers_only_render_the_five_original_categories() {
        let doc=parse_style_xml("<autoNum numType='TABLE' num='12tail'><format/><autoNumFormat type='ROMAN_CAPITAL' prefixChar='('/></autoNum>").unwrap();
        assert_eq!(note_auto_num_of(&doc, doc.root).unwrap(), "(XII");
        assert_eq!(note_attrs_of(&doc, doc.root)["number"], Value::Null);
        let doc = parse_style_xml("<autoNum numType='PAGE' num='2'/>").unwrap();
        assert_eq!(note_auto_num_of(&doc, doc.root).unwrap(), "");
        assert_eq!(
            note_auto_num_value(
                Some(&json!("1")),
                &json!({"type":"USER_CHAR","userChar":3,"prefixChar":2,"suffixChar":4})
            )
            .unwrap(),
            9
        );
    }
}
