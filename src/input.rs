//! 이전 업무 도구와 동일한 공백·날짜·JSON 값 처리.
use chrono::NaiveDate;
use serde_json::Value;

pub fn is_space(c: char) -> bool {
    matches!(c,
        '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
        '\u{205f}' | '\u{3000}' | '\u{feff}')
}

pub fn trim(value: &str) -> &str {
    value.trim_matches(is_space)
}

pub fn date_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let year: i32 = value[..4].parse().unwrap();
    let month: u32 = value[5..7].parse().unwrap();
    let day: u32 = value[8..].parse().unwrap();
    year > 0 && NaiveDate::from_ymd_opt(year, month, day).is_some()
}

pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n
            .as_f64()
            .map(|n| ryu_js::Buffer::new().format(n).to_owned())
            .unwrap_or_else(|| n.to_string()),
        Value::Array(a) => a
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    js_string(v)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_keep_ecmascript_exponents_and_rounding() {
        for (json, expected) in [
            ("1e21", "1e+21"),
            ("1e-7", "1e-7"),
            ("-0", "0"),
            ("9007199254740993", "9007199254740992"),
            ("1.0000000000000002", "1.0000000000000002"),
        ] {
            assert_eq!(
                js_string(&serde_json::from_str::<Value>(json).unwrap()),
                expected
            );
        }
    }
}
