//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Ports shared/numbering.mjs and the formatter in hwpx/para-heading.mjs.
//! See data/kordoc-LICENSE, data/kordoc-NOTICE and data/kordoc-PROVENANCE.json.
use super::value::{Value, native_json as json, number, string};

pub(crate) fn truth(value: &Value) -> bool {
    match value {
        Value::Null | Value::Undefined => false,
        Value::Bool(value) => *value,
        Value::Number(value) => *value != 0.0 && !value.is_nan(),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
pub(crate) fn nullish(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Undefined)
}
pub(crate) fn primitive(value: &Value) -> Value {
    match value {
        Value::Array(_) | Value::Object(_) => string(value).into(),
        _ => value.clone(),
    }
}
pub(crate) fn add(left: &Value, right: &Value) -> Value {
    let left = primitive(left);
    let right = primitive(right);
    if left.is_string() || right.is_string() {
        (string(&left) + &string(&right)).into()
    } else {
        (number(&left) + number(&right)).into()
    }
}
pub(crate) fn code_point(value: f64) -> Result<Value, String> {
    if value.fract() != 0.0 || !(0.0..=0x10ffff as f64).contains(&value) {
        return Err("잘못된 Unicode 코드 포인트입니다.".into());
    }
    char::from_u32(value as u32)
        .map(|ch| ch.to_string().into())
        .ok_or_else(|| "짝이 없는 UTF-16 서로게이트는 지원하지 않습니다.".into())
}
pub(crate) fn char_code(value: f64) -> Result<Value, String> {
    let value = if value.is_finite() {
        value.trunc().rem_euclid(65536.0)
    } else {
        0.0
    };
    code_point(value)
}
pub(crate) fn cycle(sequence: &str, index: f64) -> Value {
    if index.fract() != 0.0 || index < 0.0 {
        return Value::Undefined;
    }
    sequence
        .chars()
        .nth(index as usize)
        .map(|ch| ch.to_string().into())
        .unwrap_or(Value::Undefined)
}
pub(crate) fn parse_int(value: &Value) -> f64 {
    let text = string(value);
    let text = crate::input::trim(&text);
    let sign = usize::from(text.starts_with(['+', '-']));
    let end = sign + text[sign..].bytes().take_while(u8::is_ascii_digit).count();
    if end == sign {
        f64::NAN
    } else {
        text[..end].parse().unwrap_or(f64::NAN)
    }
}
pub fn hangul_ordinal(n: &Value) -> Result<Value, String> {
    const INITIALS: [f64; 14] = [
        0.0, 2.0, 3.0, 5.0, 6.0, 7.0, 9.0, 11.0, 12.0, 14.0, 15.0, 16.0, 17.0, 18.0,
    ];
    const MEDIALS: [f64; 6] = [0.0, 4.0, 8.0, 13.0, 18.0, 20.0];
    let n = number(n);
    let vowel_index = (n / 14.0).floor().min(5.0);
    let initial_index = n % 14.0;
    let at = |values: &[f64], i: f64| {
        if !i.is_finite() || i.fract() != 0.0 || i < 0.0 {
            f64::NAN
        } else {
            values.get(i as usize).copied().unwrap_or(f64::NAN)
        }
    };
    code_point(
        0xac00 as f64 + at(&INITIALS, initial_index) * 588.0 + at(&MEDIALS, vowel_index) * 28.0,
    )
}
pub fn circled_number(n: &Value) -> Result<Value, String> {
    let value = number(n);
    if value < 20.0 {
        code_point(number(&add(&json!(0x2460), n)))
    } else if value < 35.0 {
        code_point(0x3251 as f64 + (value - 20.0))
    } else if value < 50.0 {
        code_point(0x32b1 as f64 + (value - 35.0))
    } else {
        Ok(format!("({})", string(&add(n, &json!(1)))).into())
    }
}
pub fn circled_hangul(n: &Value) -> Result<Value, String> {
    if number(n) < 14.0 {
        code_point(number(&add(&json!(0x326e), n)))
    } else {
        hangul_ordinal(n)
    }
}
pub fn hangul_amount(n: &Value) -> String {
    const DIGITS: [&str; 10] = ["영", "일", "이", "삼", "사", "오", "육", "칠", "팔", "구"];
    const SMALL: [&str; 4] = ["", "십", "백", "천"];
    const BIG: [&str; 6] = ["", "만", "억", "조", "경", "해"];
    let raw: String = string(n).chars().filter(char::is_ascii_digit).collect();
    let digits = raw.trim_start_matches('0');
    if digits.is_empty() {
        return "영".into();
    }
    let mut groups = Vec::new();
    let mut end = digits.len();
    while end > 0 {
        let start = end.saturating_sub(4);
        groups.push(&digits[start..end]);
        end = start;
    }
    let mut out = String::new();
    for (group_index, group) in groups.iter().enumerate().rev() {
        let padded = format!("{group:0>4}");
        let mut part = String::new();
        for (index, byte) in padded.bytes().enumerate() {
            let digit = usize::from(byte - b'0');
            if digit != 0 {
                part.push_str(DIGITS[digit]);
                part.push_str(SMALL[3 - index]);
            }
        }
        if !part.is_empty() {
            out.push_str(&part);
            out.push_str(BIG.get(group_index).copied().unwrap_or(""));
        }
    }
    out
}
/// The renderer's unbounded Roman formatter shares these subtraction steps,
/// but its public range policy is deliberately kept in para_number.
pub(crate) fn roman(mut n: f64, upper: bool) -> Result<String, String> {
    // The JS loop never terminates for +Infinity or numbers too large for n-1000
    // to change n. Reject those and output above one MiB, rather than hanging.
    if n == f64::INFINITY || n >= 1_048_577_000.0 {
        return Err("로마 숫자 출력이 1MiB 한도를 넘습니다.".into());
    }
    let mut out = String::new();
    for (value, symbol) in [
        (1000.0, "M"),
        (900.0, "CM"),
        (500.0, "D"),
        (400.0, "CD"),
        (100.0, "C"),
        (90.0, "XC"),
        (50.0, "L"),
        (40.0, "XL"),
        (10.0, "X"),
        (9.0, "IX"),
        (5.0, "V"),
        (4.0, "IV"),
        (1.0, "I"),
    ] {
        while n >= value {
            if out.len() + symbol.len() > 1_048_576 {
                return Err("로마 숫자 출력이 1MiB 한도를 넘습니다.".into());
            }
            out.push_str(symbol);
            n -= value;
        }
    }
    Ok(if upper { out } else { out.to_lowercase() })
}
pub fn roman_numeral(n: &Value, upper: bool) -> Result<Value, String> {
    let value = number(n);
    if value <= 0.0 || value > 3999.0 {
        Ok(string(n).into())
    } else {
        Ok(roman(value, upper)?.into())
    }
}
/// The hwpx/para-heading formatter (not render/para-number's different rules).
pub fn format_head_number(n: &Value, num_format: &str) -> Result<Value, String> {
    let mut n = n.clone();
    if n.as_f64() == Some(0.0) && num_format == "DIGIT" {
        return Ok("0".into());
    }
    if number(&n) <= 0.0 {
        n = 1.into();
    }
    let value = number(&n);
    match num_format {
        "DIGIT" => Ok(string(&n).into()),
        "CIRCLED_DIGIT" => {
            if value <= 50.0 {
                circled_number(&json!(value - 1.0))
            } else {
                Ok(format!("({})", string(&n)).into())
            }
        }
        "HANGUL_SYLLABLE" => hangul_ordinal(&json!(value - 1.0)),
        "CIRCLED_HANGUL_SYLLABLE" => {
            if value <= 14.0 {
                circled_hangul(&json!(value - 1.0))
            } else {
                hangul_ordinal(&json!(value - 1.0))
            }
        }
        "HANGUL_JAMO" => Ok(cycle("ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ", (value - 1.0) % 14.0)),
        "CIRCLED_HANGUL_JAMO" => {
            if value <= 14.0 {
                code_point(number(&add(&json!(0x3260), &n)) - 1.0)
            } else {
                Ok(cycle("ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ", (value - 1.0) % 14.0))
            }
        }
        "LATIN_CAPITAL" => char_code(0x41 as f64 + (value - 1.0) % 26.0),
        "LATIN_SMALL" => char_code(0x61 as f64 + (value - 1.0) % 26.0),
        "CIRCLED_LATIN_CAPITAL" => {
            if value <= 26.0 {
                code_point(number(&add(&json!(0x24b6), &n)) - 1.0)
            } else {
                char_code(0x41 as f64 + (value - 1.0) % 26.0)
            }
        }
        "CIRCLED_LATIN_SMALL" => {
            if value <= 26.0 {
                code_point(number(&add(&json!(0x24d0), &n)) - 1.0)
            } else {
                char_code(0x61 as f64 + (value - 1.0) % 26.0)
            }
        }
        "ROMAN_CAPITAL" => roman_numeral(&n, true),
        "ROMAN_SMALL" => roman_numeral(&n, false),
        _ => Ok(string(&n).into()),
    }
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    let n = &input["n"];
    let value = match operation {
        "hangulOrdinal" => hangul_ordinal(n)?,
        "circledNumber" => circled_number(n)?,
        "circledHangul" => circled_hangul(n)?,
        "hangulAmount" => hangul_amount(n).into(),
        "romanNumeral" => roman_numeral(n, truth(&input["upper"]))?,
        "formatHeading" => format_head_number(n, input["format"].as_str().unwrap_or(""))?,
        _ => return Err("알 수 없는 번호 연산입니다.".into()),
    };
    Ok(value.to_json())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_keep_distinct_ranges_and_non_numeric_results() {
        assert_eq!(format_head_number(&json!(0), "DIGIT").unwrap(), "0");
        assert_eq!(format_head_number(&json!(-1), "HANGUL_JAMO").unwrap(), "ㄱ");
        assert_eq!(
            format_head_number(&json!(51), "CIRCLED_DIGIT").unwrap(),
            "(51)"
        );
        assert_eq!(hangul_ordinal(&json!(14)).unwrap(), "거");
        assert_eq!(roman_numeral(&json!(4000), true).unwrap(), "4000");
        assert_eq!(roman_numeral(&json!(f64::NAN), true).unwrap(), "");
        assert!(
            format_head_number(&json!(1.5), "HANGUL_JAMO")
                .unwrap()
                .is_undefined()
        );
        assert!(hangul_ordinal(&json!(-1)).is_err());
    }
    #[test]
    fn amount_and_coercion_preserve_original_digit_extraction() {
        assert_eq!(hangul_amount(&json!("0001,234.50")), "일십이만삼천사백오십");
        assert_eq!(hangul_amount(&json!(1e21)), "일백이십일");
        assert_eq!(hangul_amount(&json!(-100)), "일백");
        assert_eq!(hangul_amount(&json!("１２３")), "영");
        assert_eq!(parse_int(&json!("\u{feff}-12px")), -12.0);
        assert_eq!(parse_int(&json!("0x20")), 0.0);
        assert_eq!(string(&add(&json!("12"), &json!(1))), "121");
    }
}
