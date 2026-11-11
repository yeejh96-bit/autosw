//! Lossless text at the layout boundary, including JavaScript lone surrogates.
//! escape_xml derives from kordoc (MIT), Copyright 2026 chrisryugj;
//! see data/kordoc-LICENSE. This module never performs lossy conversion.
use super::{equation::EquationText, value::Value as NativeValue};
use std::{borrow::Cow, iter::Peekable};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutText(pub Vec<u16>);

/// Borrow ordinary UTF-8 or existing units. NativeValue's optional unit array
/// is decoded once into the owned Cow branch; typed callers can borrow &[u16].
#[derive(Clone, Debug)]
pub enum TextRef<'a> {
    Utf8(&'a str),
    Utf16(Cow<'a, [u16]>),
}
pub enum Units<'a> {
    Utf8(std::str::EncodeUtf16<'a>),
    Utf16(std::iter::Copied<std::slice::Iter<'a, u16>>),
}
impl Iterator for Units<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<u16> {
        match self {
            Self::Utf8(i) => i.next(),
            Self::Utf16(i) => i.next(),
        }
    }
}
/// JS string iteration: combine adjacent valid pairs, retain each unpaired
/// surrogate as its original u32 code point (not U+FFFD).
pub struct CodePoints<'a>(Peekable<Units<'a>>);
impl Iterator for CodePoints<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        let first = self.0.next()?;
        if (0xd800..=0xdbff).contains(&first)
            && let Some(&second) = self.0.peek()
            && (0xdc00..=0xdfff).contains(&second)
        {
            self.0.next();
            Some(0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00)
        } else {
            Some(u32::from(first))
        }
    }
}
impl TextRef<'_> {
    pub fn units(&self) -> Units<'_> {
        match self {
            Self::Utf8(s) => Units::Utf8(s.encode_utf16()),
            Self::Utf16(s) => Units::Utf16(s.iter().copied()),
        }
    }
    pub fn code_points(&self) -> CodePoints<'_> {
        CodePoints(self.units().peekable())
    }
    pub fn len_units(&self) -> usize {
        match self {
            Self::Utf8(s) => s.encode_utf16().count(),
            Self::Utf16(s) => s.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Utf8(s) => s.is_empty(),
            Self::Utf16(s) => s.is_empty(),
        }
    }
    /// Mirrors codePointAt(text.length - 1), including a low surrogate when
    /// the text ends in a supplementary character.
    pub fn last_unit(&self) -> Option<u16> {
        self.units().last()
    }
    pub fn to_layout_text(&self) -> LayoutText {
        LayoutText(self.units().collect())
    }
}
impl LayoutText {
    pub fn as_text_ref(&self) -> TextRef<'_> {
        TextRef::Utf16(Cow::Borrowed(&self.0))
    }
    pub fn append(&mut self, other: &TextRef<'_>) {
        self.0.extend(other.units());
    }
    pub fn concat(parts: &[TextRef<'_>]) -> Self {
        let mut result = Self::default();
        for part in parts {
            result.append(part);
        }
        result
    }
    /// Remove exactly the original C0 set, then escape &, <, > and double
    /// quotes. Work on units so removal/concatenation may form a valid pair.
    pub fn escape_xml(&self) -> Self {
        let mut out = Vec::new();
        for &unit in &self.0 {
            let escaped = match unit {
                0..=8 | 11 | 12 | 14..=31 => continue,
                38 => Some("&amp;"),
                60 => Some("&lt;"),
                62 => Some("&gt;"),
                34 => Some("&quot;"),
                _ => None,
            };
            if let Some(s) = escaped {
                out.extend(s.encode_utf16());
            } else {
                out.push(unit);
            }
        }
        Self(out)
    }
    /// Only verifies UTF-16 pairing. NUL and other XML-forbidden scalars can
    /// still be present: the real SVG validator must run after conversion.
    pub fn to_string_strict(&self) -> Result<String, String> {
        String::from_utf16(&self.0).map_err(|_| "UTF-16에 짝이 없는 서로게이트가 있습니다.".into())
    }
}
impl From<&str> for LayoutText {
    fn from(s: &str) -> Self {
        Self(s.encode_utf16().collect())
    }
}
impl From<String> for LayoutText {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}
impl From<Vec<u16>> for LayoutText {
    fn from(units: Vec<u16>) -> Self {
        Self(units)
    }
}
impl From<EquationText> for LayoutText {
    fn from(text: EquationText) -> Self {
        Self(text.units)
    }
}
impl From<&EquationText> for LayoutText {
    fn from(text: &EquationText) -> Self {
        Self(text.units.clone())
    }
}
impl From<LayoutText> for EquationText {
    fn from(text: LayoutText) -> Self {
        Self { units: text.0 }
    }
}

fn native_units(value: &NativeValue) -> Result<Vec<u16>, String> {
    value
        .as_array()
        .ok_or("ctrlTextUnits 배열이 필요합니다.")?
        .iter()
        .map(|n| {
            n.as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| "ctrlTextUnits 항목은 0~65535 정수여야 합니다.".into())
        })
        .collect()
}
/// ctrlTextUnits, when present, is authoritative, including an empty array.
/// Producers should store only ctrlText for ordinary text, only ctrlTextUnits
/// when strict UTF-8 conversion is impossible. Consumers must use this accessor
/// and has_control_text rather than testing the legacy field directly.
pub fn control_text(slot: &NativeValue) -> Result<Option<TextRef<'_>>, String> {
    if let Some(units) = slot.get("ctrlTextUnits") {
        return Ok(Some(TextRef::Utf16(Cow::Owned(native_units(units)?))));
    }
    match &slot["ctrlText"] {
        NativeValue::Undefined | NativeValue::Null => Ok(None),
        NativeValue::String(s) => Ok(Some(TextRef::Utf8(s))),
        _ => Err("ctrlText 문자열이 필요합니다.".into()),
    }
}
pub fn has_control_text(slot: &NativeValue) -> Result<bool, String> {
    Ok(control_text(slot)?.is_some_and(|text| !text.is_empty()))
}

#[cfg(test)]
fn read(input: &serde_json::Value) -> Result<LayoutText, String> {
    if let Some(units) = input.get("units") {
        let units = units
            .as_array()
            .ok_or("units 배열이 필요합니다.")?
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or("units 항목은 0~65535 정수여야 합니다.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LayoutText(units))
    } else {
        Ok(LayoutText::from(
            input["text"].as_str().ok_or("text 문자열이 필요합니다.")?,
        ))
    }
}
#[cfg(test)]
fn report(text: &LayoutText) -> serde_json::Value {
    let strict = text.to_string_strict().ok();
    let points = text.as_text_ref().code_points().collect::<Vec<_>>();
    serde_json::json!({"units":text.0,"codePoints":points,"unitLength":text.0.len(),"pointLength":points.len(),
        "strictText":strict,"utf8Hex":strict.as_ref().map(|s|data_encoding::HEXLOWER.encode(s.as_bytes()))})
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    if op == "controlText" {
        let slot = NativeValue::from_json(&input["slot"]);
        let value = control_text(&slot)?;
        return Ok(
            serde_json::json!({"present":value.is_some(),"hasText":has_control_text(&slot)?,"text":value.map(|v|report(&v.to_layout_text()))}),
        );
    }
    let text = match op {
        "concat" => {
            let mut out = LayoutText::default();
            for part in input["parts"]
                .as_array()
                .ok_or("parts 배열이 필요합니다.")?
            {
                out.append(&read(part)?.as_text_ref());
            }
            out
        }
        "note" => super::notes::note_ref_mark_utf16(
            &NativeValue::from_json(&input["attrs"]),
            input.get("fmt").map(NativeValue::from_json).as_ref(),
        )?
        .into(),
        "equation" => {
            let source = read(input)?;
            let latex = if input["chain"].as_bool() == Some(true) {
                super::equation::hwp_equation_to_latex_utf16(&source.0)?.units
            } else {
                source.0
            };
            super::equation::latex_to_plain_utf16(&latex)?.into()
        }
        "inspect" | "escapeXml" | "validateSvg" => read(input)?,
        _ => return Err("알 수 없는 layout 문자 연산입니다.".into()),
    };
    if op == "validateSvg" {
        let text = if input["wrap"].as_bool() == Some(true) {
            let mut svg = LayoutText::from("<svg xmlns=\"http://www.w3.org/2000/svg\"><text>");
            svg.append(&text.escape_xml().as_text_ref());
            svg.append(&TextRef::Utf8("</text></svg>"));
            svg
        } else {
            text
        };
        // The strict conversion is only the representability gate. Preserve
        // the original XML-invalid-text failure, then call the real validator.
        let valid = text
            .to_string_strict()
            .map_err(|_| "XML에 쓸 수 없는 문자입니다.".to_owned())
            .and_then(|svg| super::svg_safety::validate_rendered_svg(&svg));
        return Ok(serde_json::json!({"accepted":valid.is_ok(),"error":valid.err()}));
    }
    Ok(if op == "escapeXml" {
        report(&text.escape_xml())
    } else {
        report(&text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_and_concatenation_pair_units_before_strict_conversion() {
        let text = LayoutText(vec![0xd800, 0, 0xdc00]);
        assert!(text.to_string_strict().is_err());
        assert_eq!(text.escape_xml().to_string_strict().unwrap(), "\u{10000}");
        let high = [0xd83d];
        let low = [0xde00];
        let joined = LayoutText::concat(&[
            TextRef::Utf16(Cow::Borrowed(&high)),
            TextRef::Utf16(Cow::Borrowed(&low)),
        ]);
        assert_eq!(joined.to_string_strict().unwrap(), "😀");
        assert_eq!(
            joined.as_text_ref().code_points().collect::<Vec<_>>(),
            [0x1f600]
        );
        assert_eq!(joined.as_text_ref().last_unit(), Some(0xde00));
    }
    #[test]
    fn strict_unicode_is_separate_from_xml_safety_and_units_take_precedence() {
        assert_eq!(LayoutText(vec![0]).to_string_strict().unwrap(), "\0");
        assert!(super::super::svg_safety::validate_rendered_svg("<svg>\0</svg>").is_err());
        let slot = super::super::value::native_json!({"ctrlText":"unused","ctrlTextUnits":[]});
        assert!(!has_control_text(&slot).unwrap());
        assert!(
            control_text(&super::super::value::native_json!({"ctrlTextUnits":[65536]})).is_err()
        );
    }
}
