//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Port of reflow.mjs glyphWidth; see data/kordoc-LICENSE and PROVENANCE.
use super::{metrics as m, value::Value};

fn truth(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn num(v: &Value) -> f64 {
    super::value::number(v)
}
fn coalesce<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if matches!(a, Value::Null | Value::Undefined) {
        b
    } else {
        a
    }
}
pub fn hancom_lang_of(cp: u32) -> &'static str {
    if cp < 0x250 || (0x1e00..0x2000).contains(&cp) || (0x2018..0x2020).contains(&cp) {
        "latin"
    } else if (0x1100..0x1200).contains(&cp)
        || (0x3130..0x3190).contains(&cp)
        || (0xa960..0xa980).contains(&cp)
        || (0xac00..0xd800).contains(&cp)
    {
        "hangul"
    } else if (0x3400..0x4dc0).contains(&cp)
        || (0x4e00..0xa000).contains(&cp)
        || (0xf900..0xfb00).contains(&cp)
        || (0x20000..0x30000).contains(&cp)
    {
        "hanja"
    } else if (0x3040..0x3100).contains(&cp) || (0x31f0..0x3200).contains(&cp) || cp == 0x3012 {
        "japanese"
    } else if (0xe000..0xf900).contains(&cp) || cp >= 0xf0000 {
        "hangul"
    } else {
        "symbol"
    }
}
pub fn glyph_width(ch: &str, st: &Value, fallback: bool) -> Result<(f64, f64), String> {
    let cp = ch.chars().next().map(|c| c as u32);
    glyph_width_cp(cp, st, fallback)
}
/// Measures a JS code point without replacing isolated UTF-16 surrogates.
/// None preserves codePointAt(0) on an empty string.
pub fn glyph_width_cp(cp: Option<u32>, st: &Value, fallback: bool) -> Result<(f64, f64), String> {
    let space = matches!(cp, Some(0x20 | 0xa0));
    let lang = if space {
        "hangul"
    } else {
        cp.map(hancom_lang_of).unwrap_or("symbol")
    };
    let l = if space || lang == "hangul" {
        &Value::Undefined
    } else {
        &st[lang]
    };
    let rel = if !space && truth(l) {
        num(&l["rel"])
    } else {
        num(coalesce(&st["rel"], &Value::Number(100.0)))
    };
    let mut h = if rel != 100.0 {
        m::mul_div(num(&st["height"]), rel, 100.0)
    } else {
        num(&st["height"])
    };
    if !space && truth(&st["script"]) {
        h = m::mul_div(h, 16.0, 25.0);
    }
    let ratio = num(if truth(l) { &l["ratio"] } else { &st["ratio"] });
    let spacing = num(if space {
        coalesce(&st["latin"]["spacing"], &st["spacing"])
    } else if truth(l) {
        &l["spacing"]
    } else {
        &st["spacing"]
    });
    let face1 = coalesce(&l["face"], &st["face"]).as_str();
    let face0 = face1.map(|f| {
        if f == "Batang" {
            if truth(&st["before2024"]) {
                "바탕"
            } else {
                "한컴바탕"
            }
        } else {
            f
        }
    });
    let hft = if truth(l) {
        truth(&l["hft"])
    } else {
        truth(&st["hft"])
    };
    let bold_face = if !space && truth(&st["bold"]) && hft {
        face0
            .filter(|f| !f.is_empty())
            .map(|f| format!("{f} Bold"))
            .filter(|f| m::font_has_glyph(Some(f), cp.map(f64::from).unwrap_or(f64::NAN)))
    } else {
        None
    };
    let old_table = || {
        face0
            .and_then(|face| m::data()["before2024Width"].get(face))
            .and_then(|table| cp.and_then(|cp| table.get(cp.to_string())))
            .and_then(Value::as_f64)
    };
    let old_em = if truth(&st["halfQuote"])
        && face0 == Some("휴먼명조")
        && matches!(cp, Some(0x201c | 0x201d))
    {
        Some(500.0)
    } else if truth(&st["halfQuote"])
        && !truth(&st["before2024"])
        && face0 == Some("휴먼명조")
        && matches!(cp, Some(0x2018 | 0x2019))
    {
        Some(312.0)
    } else if (truth(&st["before2024"])
        && face0.is_some_and(|f| !f.is_empty())
        && !(face0 == Some("신명조")
            && (truth(&st["halfQuote"])
                || if truth(&l["face"]) {
                    truth(&l["hft"])
                } else {
                    truth(&st["hft"])
                })))
        || (!truth(&st["halfQuote"]) && face0 == Some("휴먼명조") && cp == Some(0x2027))
    {
        old_table()
    } else {
        None
    };
    let fc = m::face_class_for_reflow(bold_face.as_deref().or(face0))?;
    if let Some(fs_em) = if space && truth(&st["fontSpace"]) {
        m::font_space_em(st["face"].as_str())
    } else {
        None
    } {
        let q = (h / 4.0).trunc();
        let b = if ratio == 100.0 {
            4.0 * m::mul_div(fs_em, q, 1000.0)
        } else {
            4.0 * (fs_em * q * ratio / 100000.0).trunc()
        };
        let s = if spacing != 0.0 && !spacing.is_nan() {
            4.0 * m::mul_div((b / 4.0).trunc(), spacing, 100.0)
        } else {
            0.0
        };
        return Ok((b + s, s));
    }
    let b_add = if !space && truth(&st["bold"]) && bold_face.is_none() {
        m::synthetic_bold_em(coalesce(&l["face"], &st["face"]).as_str(), hft)
    } else {
        0.0
    };
    if let Some(old) = old_em {
        let b0 = m::hancom_em_width(old + b_add, h, ratio, 0.0);
        let w0 = if spacing != 0.0 && !spacing.is_nan() {
            m::hancom_em_width(old + b_add, h, ratio, spacing)
        } else {
            b0
        };
        return Ok((w0, w0 - b0));
    }
    let base = m::hancom_char_width_cp(cp, h, ratio, 0.0, Some(&fc), b_add, fallback);
    let w = if spacing != 0.0 && !spacing.is_nan() {
        m::hancom_char_width_cp(cp, h, ratio, spacing, Some(&fc), b_add, fallback)
    } else {
        base
    };
    Ok((w, w - base))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonfinite_style_numbers_reach_actual_measurement_without_json_loss() {
        let styles = super::super::head_styles::parse_render_styles(
            "<head><charPr id='0'><relSz hangul='NaN'/></charPr></head>",
            None,
        )
        .unwrap();
        let st = &styles["charPr"][0][1];
        assert!(st["rel"].as_f64().unwrap().is_nan());
        let (w, sp) = glyph_width("가", st, true).unwrap();
        assert!(w.is_nan() && sp.is_nan());
        // Report serialization happens only after measurement.
        assert!(styles.to_json()["charPr"][0][1]["rel"].is_null());
    }
    #[test]
    fn code_points_keep_empty_and_surrogate_measurement_distinct() {
        let st = super::super::value::native_json!({"height":1100,"ratio":100,"spacing":0,"face":"굴림"});
        for ch in ["", "가", "😀", " ", "\u{2005}"] {
            assert_eq!(
                glyph_width(ch, &st, true).unwrap(),
                glyph_width_cp(ch.chars().next().map(u32::from), &st, true).unwrap()
            );
        }
        for cp in [0xd800, 0xdbff, 0xdc00, 0xdfff] {
            let (w, sp) = glyph_width_cp(Some(cp), &st, true).unwrap();
            assert_eq!(
                w,
                m::hancom_char_width_cp(Some(cp), 1100.0, 100.0, 0.0, Some("font:굴림"), 0.0, true)
            );
            assert_eq!(sp, 0.0);
        }
    }
    #[test]
    fn language_boundaries_keep_private_use_and_hangul_ranges() {
        for (cp, lang) in [
            (0x20, "latin"),
            (0x250, "symbol"),
            (0x1100, "hangul"),
            (0x3012, "japanese"),
            (0x3400, "hanja"),
            (0xe000, "hangul"),
            (0xf900, "hanja"),
            (0xf0000, "hangul"),
        ] {
            assert_eq!(hancom_lang_of(cp), lang);
        }
    }
}
