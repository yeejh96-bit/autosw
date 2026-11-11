//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Ports render/para-number.mjs and hwpx/para-heading.mjs state transitions.
//! See data/kordoc-LICENSE, data/kordoc-NOTICE and data/kordoc-PROVENANCE.json.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    numbering::{self, add, char_code, code_point, cycle, nullish, roman, truth},
    style_dom::ParsedXml,
    value::{Map, Value, native_json as json, number, string},
};
use regex::Regex;
use std::sync::LazyLock;

pub fn format_head_number(n: &Value, format: &str) -> Result<Value, String> {
    let value = number(n);
    match format {
        "HANGUL_SYLLABLE" => Ok(cycle("가나다라마바사아자차카타파하", (value - 1.0) % 14.0)),
        "HANGUL_JAMO" => Ok(cycle("ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ", (value - 1.0) % 14.0)),
        "CIRCLED_DIGIT" => {
            if (1.0..=20.0).contains(&value) {
                code_point(number(&add(&json!(0x245f), n)))
            } else {
                Ok(string(n).into())
            }
        }
        "CIRCLED_HANGUL_SYLLABLE" => code_point(0x326e as f64 + (value - 1.0) % 14.0),
        "CIRCLED_HANGUL_JAMO" => code_point(0x3260 as f64 + (value - 1.0) % 14.0),
        "LATIN_SMALL" => char_code(0x61 as f64 + (value - 1.0) % 26.0),
        "LATIN_CAPITAL" => char_code(0x41 as f64 + (value - 1.0) % 26.0),
        "ROMAN_SMALL" => Ok(roman(value, false)?.into()),
        "ROMAN_CAPITAL" => Ok(roman(value, true)?.into()),
        "IDEOGRAPH" => {
            if (1.0..=10.0).contains(&value) {
                Ok(cycle("一二三四五六七八九十", value - 1.0))
            } else {
                Ok(string(n).into())
            }
        }
        _ => Ok(string(n).into()),
    }
}
fn same_key(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b || a.is_nan() && b.is_nan(),
        _ => left == right,
    }
}
/// Map-like data is exchanged as [key,value] entries, preserving numeric keys.
fn lookup<'a>(map: &'a Value, key: &Value) -> Option<&'a Value> {
    match map {
        Value::Array(entries) => entries
            .iter()
            .find(|entry| same_key(&entry[0], key))
            .map(|entry| &entry[1]),
        Value::Object(entries) => key.as_str().and_then(|key| entries.get(key)),
        _ => None,
    }
}
fn require_map(map: &Value) -> Result<(), String> {
    if map.is_array() || map.is_object() {
        Ok(())
    } else {
        Err("번호 정의 Map이 없습니다.".into())
    }
}
fn index_key(key: &str) -> Option<u32> {
    let value = key.parse::<u32>().ok()?;
    (value != u32::MAX && value.to_string() == key).then_some(value)
}
/// A JS array's sparse indexed values, custom property names and length.
/// Large numbering levels therefore never allocate a huge dense Vec.
#[derive(Clone, Debug, Default)]
pub struct CounterArray {
    length: u32,
    properties: Map<String, Value>,
}
impl CounterArray {
    fn get(&self, key: &Value) -> Value {
        self.properties
            .get(&string(key))
            .cloned()
            .unwrap_or(Value::Undefined)
    }
    fn set(&mut self, key: &Value, value: Value) {
        let key = string(key);
        if let Some(index) = index_key(&key) {
            self.length = self.length.max(index + 1);
        }
        self.properties.insert(key, value);
    }
    fn set_length(&mut self, value: &Value) -> Result<(), String> {
        let length = number(value);
        if !length.is_finite()
            || length.fract() != 0.0
            || !(0.0..=f64::from(u32::MAX)).contains(&length)
        {
            return Err("잘못된 번호 배열 길이입니다.".into());
        }
        let length = length as u32;
        if length < self.length {
            let remove: Vec<_> = self
                .properties
                .keys()
                .filter(|key| index_key(key).is_some_and(|i| i >= length))
                .cloned()
                .collect();
            for key in remove {
                self.properties.shift_remove(&key);
            }
        }
        self.length = length;
        Ok(())
    }
    fn filled() -> Self {
        let mut value = Self::default();
        for i in 0..11 {
            value.set(&json!(i), json!(-1));
        }
        value
    }
    fn from_value(value: &Value) -> Result<Self, String> {
        let mut result = Self::default();
        if let Some(values) = value.as_array() {
            for (index, value) in values.iter().enumerate() {
                result.set(&json!(index), value.clone());
            }
        } else if let Some(entries) = value["entries"].as_array() {
            for entry in entries {
                let key = entry[0]
                    .as_str()
                    .ok_or("번호 속성 이름이 문자열이 아닙니다.")?;
                result.set(&json!(key), entry[1].clone());
            }
            result.set_length(&value["length"])?;
        } else {
            return Err("번호 상태 배열이 없습니다.".into());
        }
        Ok(result)
    }
    fn value(&self) -> Value {
        let mut numbered: Vec<_> = self
            .properties
            .iter()
            .filter_map(|(key, value)| index_key(key).map(|index| (index, key, value)))
            .collect();
        numbered.sort_by_key(|(index, _, _)| *index);
        let mut entries: Vec<Value> = numbered
            .into_iter()
            .map(|(_, key, value)| json!([key, value]))
            .collect();
        entries.extend(
            self.properties
                .iter()
                .filter(|(key, _)| index_key(key).is_none())
                .map(|(key, value)| json!([key, value])),
        );
        json!({"length":self.length,"entries":entries})
    }
}
#[derive(Clone, Debug, Default)]
pub struct CounterState {
    entries: Vec<(Value, CounterArray)>,
}
impl CounterState {
    fn position(&self, key: &Value) -> Option<usize> {
        self.entries.iter().position(|(old, _)| same_key(old, key))
    }
    fn set(&mut self, key: Value, value: CounterArray) {
        if let Some(index) = self.position(&key) {
            self.entries[index].1 = value;
        } else {
            self.entries.push((key, value));
        }
    }
    pub fn from_value(value: &Value) -> Result<Self, String> {
        if nullish(value) {
            return Ok(Self::default());
        }
        let mut state = Self::default();
        for entry in value
            .as_array()
            .ok_or("번호 상태 Map 항목 배열이 없습니다.")?
        {
            state.set(entry[0].clone(), CounterArray::from_value(&entry[1])?);
        }
        Ok(state)
    }
    pub fn to_value(&self) -> Value {
        json!(
            self.entries
                .iter()
                .map(|(key, value)| json!([key, value.value()]))
                .collect::<Vec<_>>()
        )
    }
}
fn fallback<'a>(value: &'a Value, default: &'a Value) -> &'a Value {
    if nullish(value) { default } else { value }
}
fn replace_refs(
    text: &str,
    heading: bool,
    mut resolve: impl FnMut(u32) -> Result<Value, String>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('^') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let width = if heading && rest.starts_with("10") {
            2
        } else if rest.as_bytes().first().is_some_and(|c| {
            if heading {
                (b'1'..=b'9').contains(c)
            } else {
                c.is_ascii_digit()
            }
        }) {
            1
        } else {
            0
        };
        if width > 0 {
            let level = rest[..width].parse().unwrap();
            out.push_str(&string(&resolve(level)?));
            rest = &rest[width..];
        } else if heading {
            out.push('^');
        }
    }
    out.push_str(rest);
    if heading {
        static LETTER_NUMBER: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}]$").unwrap());
        let mut cleaned = String::new();
        let mut chars = out.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '^'
                || chars
                    .peek()
                    .is_some_and(|next| LETTER_NUMBER.is_match(&next.to_string()))
            {
                cleaned.push(ch);
            }
        }
        Ok(crate::input::trim(&cleaned).to_owned())
    } else {
        Ok(out)
    }
}
pub fn next_para_number(
    counters: &mut CounterState,
    geom: Option<&Value>,
) -> Result<String, String> {
    let num = geom
        .map(|geom| &geom["number"])
        .unwrap_or(&Value::Undefined);
    if !truth(num) {
        return Ok(String::new());
    }
    require_map(&num["heads"])?;
    let existing = counters.position(&num["id"]);
    let mut cur = existing
        .map(|index| counters.entries[index].1.clone())
        .unwrap_or_default();
    let head = lookup(&num["heads"], &num["level"]).unwrap_or(&Value::Undefined);
    let previous = cur.get(&num["level"]);
    let start = number(fallback(&head["start"], &json!(1))) - 1.0;
    cur.set(
        &num["level"],
        add(fallback(&previous, &json!(start)), &json!(1)),
    );
    if let Err(error) = cur.set_length(&add(&num["level"], &json!(1))) {
        // Existing JS arrays are mutated before the failing length assignment.
        if let Some(index) = existing {
            counters.entries[index].1 = cur;
        }
        return Err(error);
    }
    counters.set(num["id"].clone(), cur.clone());
    let text = fallback(&head["text"], &Value::String(String::new()))
        .as_str()
        .ok_or("번호 머리말이 문자열이 아닙니다.")?
        .to_owned();
    replace_refs(&text, false, |level| {
        let h = lookup(&num["heads"], &json!(level)).unwrap_or(&Value::Undefined);
        let current = cur.get(&json!(level));
        let n = fallback(&current, fallback(&h["start"], &json!(1))).clone();
        format_head_number(
            &n,
            fallback(&h["format"], &json!("DIGIT"))
                .as_str()
                .unwrap_or(""),
        )
    })
}
fn min_number(value: f64, max: f64) -> f64 {
    if value.is_nan() {
        value
    } else {
        value.min(max)
    }
}
pub fn resolve_para_heading(
    doc: &ParsedXml,
    node: usize,
    ctx: &Value,
    counters: &mut CounterState,
) -> Result<Value, String> {
    let element = doc.nodes.get(node).ok_or("문단 XML 노드가 없습니다.")?;
    let sm = &ctx["styleMap"];
    if !truth(sm) {
        return Ok(Value::Null);
    }
    let mut style_heading = Value::Undefined;
    if let Some(style_id) = element
        .attr("styleIDRef")
        .filter(|id| !id.is_empty() && *id != "0")
    {
        require_map(&sm["styles"])?;
        if let Some(style) = lookup(&sm["styles"], &json!(style_id)).filter(|value| truth(value)) {
            let name = string(&style["name"]);
            if let Some(tail) = name.strip_prefix("개요") {
                let tail = tail.trim_start_matches(crate::input::is_space);
                if tail.len() == 1 && (b'1'..=b'6').contains(&tail.as_bytes()[0]) {
                    style_heading = json!(tail.parse::<u32>().unwrap());
                }
            }
        }
    }
    let style_fallback = || {
        if truth(&style_heading) {
            json!({"headingLevel":style_heading})
        } else {
            Value::Null
        }
    };
    let Some(pr_id) = element.attr("paraPrIDRef").filter(|id| !id.is_empty()) else {
        return Ok(style_fallback());
    };
    require_map(&sm["paraHeadings"])?;
    let Some(reference) = lookup(&sm["paraHeadings"], &json!(pr_id)).filter(|v| truth(v)) else {
        return Ok(style_fallback());
    };
    if reference["type"] == "BULLET" {
        require_map(&sm["bullets"])?;
        return Ok(lookup(&sm["bullets"], &reference["idRef"])
            .filter(|v| truth(v))
            .map(|ch| json!({"prefix":ch}))
            .unwrap_or(Value::Null));
    }
    let outline = reference["type"] == "OUTLINE";
    let num_id = if outline {
        if truth(&ctx["outlineNumId"]) {
            ctx["outlineNumId"].clone()
        } else {
            json!("1")
        }
    } else {
        reference["idRef"].clone()
    };
    let plus = number(&add(&reference["level"], &json!(1)));
    let level = min_number(plus, 10.0);
    let heading_level = if outline {
        json!(min_number(plus, 6.0))
    } else {
        Value::Undefined
    };
    require_map(&sm["numberings"])?;
    let Some(definition) = lookup(&sm["numberings"], &num_id).filter(|v| truth(v)) else {
        return Ok(if truth(&heading_level) {
            json!({"headingLevel":heading_level})
        } else {
            Value::Null
        });
    };
    require_map(&definition["heads"])?;
    let mut cur = counters
        .position(&num_id)
        .map(|index| counters.entries[index].1.clone())
        .unwrap_or_else(CounterArray::filled);
    let head = lookup(&definition["heads"], &json!(level)).unwrap_or(&Value::Undefined);
    let old = cur.get(&json!(level));
    let current = if number(&old) < 0.0 {
        fallback(&head["start"], &json!(1)).clone()
    } else {
        add(&old, &json!(1))
    };
    cur.set(&json!(level), current);
    let mut reset = level + 1.0;
    if reset == f64::NEG_INFINITY || reset < -1_048_576.0 {
        return Err("번호 초기화 범위가 처리 한도를 넘습니다.".into());
    }
    while reset <= 10.0 {
        cur.set(&json!(reset), json!(-1));
        reset += 1.0;
    }
    counters.set(num_id, cur.clone());
    let fmt_text = if truth(head) {
        crate::input::trim(
            head["text"]
                .as_str()
                .ok_or("번호 머리말이 문자열이 아닙니다.")?,
        )
        .to_owned()
    } else {
        format!("^{}.", string(&json!(level)))
    };
    let prefix = replace_refs(&fmt_text, true, |level| {
        let ref_head = lookup(&definition["heads"], &json!(level)).unwrap_or(&Value::Undefined);
        let current = cur.get(&json!(level));
        let n = if number(&current) >= 0.0 {
            current
        } else {
            fallback(&ref_head["start"], &json!(1)).clone()
        };
        numbering::format_head_number(
            &n,
            if truth(&ref_head["numFormat"]) {
                ref_head["numFormat"].as_str().unwrap_or("")
            } else {
                "DIGIT"
            },
        )
    })?;
    Ok(
        json!({"prefix":if prefix.is_empty(){Value::Undefined}else{prefix.into()},"headingLevel":heading_level}),
    )
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    if operation == "formatRender" {
        return format_head_number(&input["n"], input["format"].as_str().unwrap_or(""))
            .map(|value| value.to_json());
    }
    let mut state = CounterState::from_value(&input["state"])?;
    let mut results = Vec::new();
    match operation {
        "nextParaNumber" => {
            for geom in input["geoms"].as_array().ok_or("문단 배열이 없습니다.")? {
                results.push(json!(next_para_number(&mut state, Some(geom))?));
            }
        }
        "resolveParaHeading" => {
            for step in input["steps"].as_array().ok_or("문단 배열이 없습니다.")? {
                let doc = parse_style_xml(step["xml"].as_str().ok_or("XML 문자열이 없습니다.")?)?;
                results.push(resolve_para_heading(
                    &doc,
                    doc.root,
                    &step["ctx"],
                    &mut state,
                )?);
            }
        }
        _ => return Err("알 수 없는 문단 번호 연산입니다.".into()),
    }
    Ok(json!({"results":results,"state":state.to_value()}).to_json())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_formatter_is_not_the_heading_formatter() {
        assert!(
            format_head_number(&json!(0), "HANGUL_JAMO")
                .unwrap()
                .is_undefined()
        );
        assert_eq!(
            format_head_number(&json!(21), "CIRCLED_DIGIT").unwrap(),
            "21"
        );
        assert_eq!(
            format_head_number(&json!(4000), "ROMAN_CAPITAL").unwrap(),
            "MMMM"
        );
        assert!(format_head_number(&json!(f64::INFINITY), "ROMAN_CAPITAL").is_err());
        assert!(format_head_number(&json!(1_048_577_000.0), "ROMAN_CAPITAL").is_err());
    }
    #[test]
    fn render_counters_reset_deeper_levels_and_keep_sparse_lengths() {
        let heads = json!([[1,{"start":3,"text":"^1.","format":"DIGIT"}],[2,{"start":1,"text":"^1-^2 ^10 ^x","format":"LATIN_SMALL"}]]);
        let mut state = CounterState::default();
        let geom = |level| json!({"number":{"id":"n","level":level,"heads":heads}});
        assert_eq!(next_para_number(&mut state, Some(&geom(1))).unwrap(), "3.");
        assert_eq!(
            next_para_number(&mut state, Some(&geom(2))).unwrap(),
            "3-a 30 x"
        );
        assert_eq!(next_para_number(&mut state, Some(&geom(1))).unwrap(), "4.");
        assert_eq!(
            next_para_number(&mut state, Some(&geom(2))).unwrap(),
            "4-a 40 x"
        );
        let sparse = json!({"number":{"id":"large","level":1_000_000_000,"heads":[]}});
        assert_eq!(next_para_number(&mut state, Some(&sparse)).unwrap(), "");
        assert_eq!(state.to_value()[1][1]["length"], 1_000_000_001);
    }
    #[test]
    fn outline_uses_style_fallback_and_ten_digit_reference() {
        let doc = parse_style_xml("<p styleIDRef='s'/>").unwrap();
        let mut state = CounterState::default();
        assert_eq!(
            resolve_para_heading(
                &doc,
                doc.root,
                &json!({"styleMap":{"styles":[["s",{"name":"개요\u{feff}3"}]]}}),
                &mut state
            )
            .unwrap()["headingLevel"],
            3
        );
        let doc = parse_style_xml("<p paraPrIDRef='p'/>").unwrap();
        let ctx = json!({"styleMap":{"paraHeadings":[["p",{"type":"OUTLINE","level":9}]],"numberings":[["1",{"heads":[[10,{"start":1,"numFormat":"DIGIT","text":" ^10. ^한 ^! " }]]}]]}});
        assert_eq!(
            resolve_para_heading(&doc, doc.root, &ctx, &mut state).unwrap(),
            json!({"prefix":"1. ^한 !","headingLevel":6})
        );
    }
}
