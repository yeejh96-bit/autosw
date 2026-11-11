//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! shared/pua.mjs and shared/symbol-fonts.mjs; see data/kordoc-LICENSE,
//! data/kordoc-NOTICE and data/kordoc-PROVENANCE.json.
//! Decode only known PUA symbols. Unmapped UTF-16 units remain lossless.
use super::{
    layout_text::{LayoutText, TextRef},
    numbering,
    value::{Value, number, string},
};
use std::borrow::Cow;

fn tens_digit(cp: u32) -> Option<u32> {
    match cp {
        0xf0288..=0xf0291 => Some(cp - 0xf0288),
        0xf02ba..=0xf02c2 => Some(cp - 0xf02b9),
        _ => None,
    }
}
fn units_digit(cp: u32) -> Option<u32> {
    match cp {
        0xf0292..=0xf029b => Some(cp - 0xf0292),
        0xf02c3..=0xf02cc => Some(cp - 0xf02c3),
        _ => None,
    }
}
pub fn enclosed_number(n: &Value) -> Result<LayoutText, String> {
    if n.as_f64() == Some(0.0) {
        return Ok(LayoutText::from("⓪"));
    }
    Ok(LayoutText::from(string(&numbering::circled_number(
        &(number(n) - 1.0).into(),
    )?)))
}
fn number_glyph(cp: u32) -> Result<Option<LayoutText>, String> {
    if let Some(t) = tens_digit(cp) {
        return enclosed_number(&t.into()).map(Some);
    }
    if (0xf02b0..=0xf02b9).contains(&cp) {
        return enclosed_number(&(cp - 0xf02b0).into()).map(Some);
    }
    Ok(units_digit(cp).map(|n| LayoutText::from(n.to_string())))
}
pub fn wingdings_char(cp: u32) -> Option<&'static str> {
    if !(0x21..=0xff).contains(&cp) {
        return None;
    }
    WINGDINGS
        .get((cp - 0x21) as usize)
        .copied()
        .filter(|s| !s.is_empty())
}
fn mapped_char(cp: u32) -> Result<Option<LayoutText>, String> {
    if (0xf020..=0xf0ff).contains(&cp) {
        let key = cp - 0xf000;
        return Ok(BMP_SYMBOL_MAP
            .iter()
            .find(|(cp, _)| *cp == key)
            .map(|(_, s)| *s)
            .or_else(|| wingdings_char(key))
            .map(LayoutText::from));
    }
    if (0xf0000..=0xf09ff).contains(&cp) {
        if let Some(number) = number_glyph(cp)? {
            return Ok(Some(number));
        }
        return Ok(SUPPLEMENTARY_MAP
            .iter()
            .find(|(key, _)| *key == cp)
            .map(|(_, s)| LayoutText::from(*s)));
    }
    Ok(None)
}
fn paired_code_points(a: u32, b: u32) -> Result<Option<LayoutText>, String> {
    match (tens_digit(a), units_digit(b)) {
        (Some(t), Some(u)) => enclosed_number(&(t * 10 + u).into()).map(Some),
        _ => Ok(None),
    }
}
pub fn paired_number(a: &TextRef<'_>, b: &TextRef<'_>) -> Result<Option<LayoutText>, String> {
    paired_code_points(
        a.code_points().next().unwrap_or(0),
        b.code_points().next().unwrap_or(0),
    )
}
fn push_code_point(out: &mut Vec<u16>, cp: u32) {
    if cp <= 0xffff {
        out.push(cp as u16);
    } else {
        let v = cp - 0x10000;
        out.push(0xd800 + (v >> 10) as u16);
        out.push(0xdc00 + (v & 0x3ff) as u16);
    }
}
pub fn map_pua_text(text: &TextRef<'_>) -> Result<LayoutText, String> {
    let mut out = LayoutText::default();
    let mut points = text.code_points().peekable();
    while let Some(cp) = points.next() {
        if let Some(&next) = points.peek()
            && let Some(pair) = paired_code_points(cp, next)?
        {
            out.0.extend(pair.0);
            points.next();
            continue;
        }
        if let Some(mapped) = mapped_char(cp)? {
            out.0.extend(mapped.0);
        } else {
            push_code_point(&mut out.0, cp);
        }
    }
    Ok(out)
}
pub fn map_pua_text_utf16(units: &[u16]) -> Result<LayoutText, String> {
    map_pua_text(&TextRef::Utf16(Cow::Borrowed(units)))
}
pub fn map_pua_text_str(text: &str) -> Result<String, String> {
    map_pua_text(&TextRef::Utf8(text))?.to_string_strict()
}
pub fn compose_digits(text: &TextRef<'_>) -> LayoutText {
    let mut out = LayoutText::default();
    for cp in text.code_points() {
        let d = match cp {
            0x2460..=0x2473 => Some(cp - 0x245f),
            0x24ea => Some(0),
            0xf02b0..=0xf02b9 => Some(cp - 0xf02b0),
            _ => tens_digit(cp).or_else(|| units_digit(cp)),
        };
        let Some(d) = d else {
            return text.to_layout_text();
        };
        out.0.extend(d.to_string().encode_utf16());
    }
    if out.0.is_empty() {
        text.to_layout_text()
    } else {
        out
    }
}
pub fn compose_text(text: &TextRef<'_>, enclosed: bool) -> Result<LayoutText, String> {
    if enclosed && matches!(text.len_units(), 1 | 2) && text.units().all(|u| (48..=57).contains(&u))
    {
        let n = text.units().fold(0u32, |n, u| n * 10 + u32::from(u - 48));
        enclosed_number(&n.into())
    } else {
        Ok(text.to_layout_text())
    }
}
#[cfg(test)]
fn read(input: &serde_json::Value) -> Result<LayoutText, String> {
    if let Some(parts) = input.get("parts") {
        let mut out = LayoutText::default();
        for part in parts.as_array().ok_or("parts 배열이 필요합니다.")? {
            out.0.extend(read(part)?.0);
        }
        return Ok(out);
    }
    if let Some(units) = input.get("units") {
        return units
            .as_array()
            .ok_or("units 배열이 필요합니다.")?
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| "units 항목은 0~65535 정수여야 합니다.".into())
            })
            .collect::<Result<Vec<_>, String>>()
            .map(LayoutText);
    }
    Ok(LayoutText::from(
        input["text"].as_str().ok_or("text 문자열이 필요합니다.")?,
    ))
}
#[cfg(test)]
fn report(text: Option<LayoutText>) -> serde_json::Value {
    match text {
        Some(text) => {
            serde_json::json!({"defined":true,"units":text.0,"strictText":text.to_string_strict().ok()})
        }
        None => serde_json::json!({"defined":false}),
    }
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let out = match op {
        "mapPuaText" => Some(map_pua_text(&read(input)?.as_text_ref())?),
        "composeDigits" => Some(compose_digits(&read(input)?.as_text_ref())),
        "composeText" => Some(compose_text(
            &read(input)?.as_text_ref(),
            numbering::truth(&Value::from_json(&input["enclosed"])),
        )?),
        "pairedNumber" => paired_number(
            &read(&input["a"])?.as_text_ref(),
            &read(&input["b"])?.as_text_ref(),
        )?,
        "enclosedNumber" => Some(enclosed_number(&Value::from_json(&input["number"]))?),
        "wingdingsChar" => input["code"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .and_then(wingdings_char)
            .map(LayoutText::from),
        _ => return Err("알 수 없는 PUA 변환입니다.".into()),
    };
    Ok(report(out))
}

// Frozen tables copied from the above MIT sources, without generated data files.
const BMP_SYMBOL_MAP: &[(u32, &str)] = &[
    (0x6c, "●"),
    (0x6d, "●"),
    (0x6e, "■"),
    (0x6f, "□"),
    (0x70, "□"),
    (0x71, "□"),
    (0x72, "□"),
    (0x73, "⬧"),
    (0x74, "⧫"),
    (0x75, "◆"),
    (0x76, "❖"),
    (0x77, "⬥"),
    (0x9e, "·"),
    (0x9f, "•"),
    (0xa0, "·"),
    (0xa1, "⚪"),
    (0xa2, "○"),
    (0xa3, "○"),
    (0xa4, "◉"),
    (0xa5, "◎"),
    (0xa7, "▪"),
    (0xa8, "◻"),
    (0xaa, "✦"),
    (0xab, "★"),
    (0xac, "✶"),
    (0xad, "✴"),
    (0xae, "✹"),
    (0x45, "☜"),
    (0x46, "☞"),
    (0x47, "☝"),
    (0x48, "☟"),
    (0xfb, "✗"),
    (0xfc, "✔"),
    (0xfd, "☒"),
    (0xfe, "☑"),
    (0xe8, "➔"),
    (0xef, "⇦"),
    (0xf0, "⇨"),
    (0xf1, "⇧"),
    (0xf2, "⇩"),
    (0x22, "✂"),
    (0x36, "⌛"),
    (0x4a, "☺"),
    (0x4e, "☠"),
    (0x52, "☼"),
    (0x54, "❄"),
    (0x58, "✠"),
    (0x59, "✡"),
];
const SUPPLEMENTARY_MAP: &[(u32, &str)] = &[
    (0xf003b, "↓"),
    (0xf02ef, "·"),
    (0xf0854, "《"),
    (0xf0855, "》"),
    (0xf00da, "[예]"),
    (0xf080f, "━"),
    (0xf0827, "■"),
    (0xf03c5, "□"),
    (0xf012b, "(인)"),
    (0xf00e1, "(인)"),
    (0xf02fc, "►"),
    (0xf031c, "■"),
    (0xf03a0, "↵"),
    (0xf03ef, "한"),
    (0xf03f0, "글"),
    (0xf03f1, "과"),
    (0xf03f2, "컴"),
    (0xf03f3, "퓨"),
    (0xf03f4, "터"),
    (0xf0090, "✺"),
    (0xf02ec, "◇"),
    (0xf02fb, "▸"),
    (0xf03a7, "⊟"),
    (0xf03a8, "⊞"),
    (0xf03da, "□"),
    (0xf03ff, "□"),
    (0xf0806, "┌"),
    (0xf0807, "┬"),
    (0xf0808, "┐"),
    (0xf080c, "└"),
    (0xf080e, "┘"),
    (0xf0810, "│"),
    (0xf081c, "┈"),
    (0xf0832, "═"),
    (0xf0848, "━"),
];
const WINGDINGS: &[&str] = &[
    "🖉", "✂", "✁", "👓", "🕭", "🕮", "🕯", "🕿", "✆", "🖂", "🖃", "📪", "📫", "📬", "📭", "📁", "📂",
    "📄", "🗏", "🗐", "🗄", "⌛", "🖮", "🖰", "🖲", "🖳", "🖴", "🖫", "🖬", "✇", "✍", "🖎", "✌", "👌", "👍",
    "👎", "☜", "☞", "☝", "☟", "🖐", "☺", "😐", "☹", "💣", "☠", "🏳", "🏱", "✈", "☼", "💧", "❄", "🕆",
    "✞", "🕈", "✠", "✡", "☪", "☯", "ॐ", "☸", "♈", "♉", "♊", "♋", "♌", "♍", "♎", "♏", "♐",
    "♑", "♒", "♓", "🙰", "🙵", "●", "○", "■", "□", "□", "❑", "❒", "⬧", "⧫", "◆", "❖", "⬥", "⌧",
    "⮹", "⌘", "🏵", "🏶", "🙶", "🙷", "", "⓪", "①", "②", "③", "④", "⑤", "⑥", "⑦", "⑧", "⑨", "⑩", "⓿",
    "❶", "❷", "❸", "❹", "❺", "❻", "❼", "❽", "❾", "❿", "🙢", "🙠", "🙡", "🙣", "🙞", "🙜", "🙝", "🙟", "·",
    "•", "▪", "⚪", "○", "◯", "◉", "◎", "🔿", "▪", "◻", "🟂", "✦", "★", "✶", "✴", "✹", "✵", "⯐", "⌖",
    "⟡", "⌑", "⯑", "✪", "✰", "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚",
    "🕛", "⮰", "⮱", "⮲", "⮳", "⮴", "⮵", "⮶", "⮷", "🙪", "🙫", "🙕", "🙔", "🙗", "🙖", "🙐", "🙑", "🙒", "🙓",
    "⌫", "⌦", "⮘", "⮚", "⮙", "⮛", "⮈", "⮊", "⮉", "⮋", "←", "→", "↑", "↓", "↖", "↗", "↙", "↘", "⬅",
    "➔", "⬆", "⬇", "⬉", "⬈", "⬋", "⬊", "⇦", "⇨", "⇧", "⇩", "⬄", "⇳", "⬀", "⬁", "⬃", "⬂", "🢬", "🢭",
    "✗", "✔", "☒", "☑", "",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmatched_units_and_late_pairs_are_lossless() {
        let units = [0xd800, 0, 0xdc00];
        assert_eq!(map_pua_text_utf16(&units).unwrap().0, units);
        let mapped = map_pua_text_utf16(&[0xdb80, 0xdc3b]).unwrap();
        assert_eq!(mapped.to_string_strict().unwrap(), "↓");
    }
    #[test]
    fn overrides_win_and_empty_wingdings_entries_leave_input_unchanged() {
        assert_eq!(map_pua_text_str("\u{f06d}").unwrap(), "●");
        assert_eq!(
            map_pua_text_str("\u{f07f}\u{f0ff}").unwrap(),
            "\u{f07f}\u{f0ff}"
        );
    }
}
