//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE, data/kordoc-NOTICE and data/kordoc-PROVENANCE.json.
//! Frozen tables retain all original initialization results; measurement and
//! wrapping execute in Rust, using JavaScript UTF-16 positions and f64 order.
use super::layout_text::TextRef;
use super::value::{Value, native_json as json};
use crate::input::trim;
use std::sync::LazyLock;

static DATA: LazyLock<Value> = LazyLock::new(|| {
    Value::from_json(
        &serde_json::from_str(include_str!("data/font-metrics.json"))
            .expect("checked-in font metrics JSON"),
    )
});
pub const SPACE_EM_FIXED: f64 = 500.0;
pub const SPACE_EM_FONT: f64 = 300.0;

pub fn data() -> &'static Value {
    &DATA
}
fn numeric(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Undefined) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(value)) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(value)) => *value,
        Some(value) => super::value::number(value),
    }
}
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Undefined) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => *value != 0.0 && !value.is_nan(),
        Some(Value::String(value)) => !value.is_empty(),
        _ => true,
    }
}
fn field<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    opts.and_then(|v| v.get(key))
}
fn option_num(opts: Option<&Value>, key: &str, fallback: f64) -> f64 {
    match field(opts, key) {
        None | Some(Value::Null | Value::Undefined) => fallback,
        value => numeric(value),
    }
}
fn index(value: Option<&Value>, at: usize) -> Option<&Value> {
    match value {
        Some(Value::Array(a)) => a.get(at),
        Some(Value::Object(o)) => o.get(&at.to_string()),
        _ => None,
    }
}
fn at_num(value: Option<&Value>, at: usize, fallback: f64) -> f64 {
    match index(value, at) {
        None | Some(Value::Null | Value::Undefined) => fallback,
        value => numeric(value),
    }
}
fn property(table: &Value, cp: f64) -> Option<f64> {
    table
        .get(ryu_js::Buffer::new().format(cp))
        .and_then(Value::as_f64)
}
fn ascii(table: &Value, cp: f64) -> f64 {
    if cp.fract() == 0.0 {
        numeric(table.get((cp - 32.0) as usize))
    } else {
        f64::NAN
    }
}
fn font(face: Option<&str>) -> Option<&'static Value> {
    face.filter(|v| !v.is_empty())
        .and_then(|v| DATA["fonts"].get(trim(v)))
}
pub fn font_space_em(face: Option<&str>) -> Option<f64> {
    face.filter(|v| !v.is_empty())
        .and_then(|v| DATA["faceSpace"].get(trim(v)))
        .and_then(Value::as_f64)
}
pub fn has_font_metrics(face: Option<&str>) -> bool {
    font(face).is_some()
}
pub fn font_has_glyph(face: Option<&str>, cp: f64) -> bool {
    font(face).is_some_and(|m| (32.0..=126.0).contains(&cp) || property(&m["ex"], cp).is_some())
}
pub fn is_halfwidth_form(cp: f64) -> bool {
    (0xff61 as f64..=0xffdc as f64).contains(&cp) || (0xffe8 as f64..=0xffee as f64).contains(&cp)
}
fn hanja(cp: f64) -> bool {
    (0x4e00 as f64..=0x9fff as f64).contains(&cp)
        || (0x3400 as f64..=0x4dbf as f64).contains(&cp)
        || (0xf900 as f64..=0xfaff as f64).contains(&cp)
}
fn font_width(m: &Value, cp: f64, fallback: bool) -> f64 {
    if (32.0..=126.0).contains(&cp) {
        return ascii(&m["ascii"], cp);
    }
    if fallback
        && let Some(w) = property(&m["ex"], cp).or_else(|| property(&DATA["hancomFallback"], cp))
    {
        return w;
    }
    if is_halfwidth_form(cp) {
        500.0
    } else if hanja(cp)
        && let Some(han) = m.get("han")
    {
        numeric(Some(han))
    } else {
        numeric(m.get("cjk"))
    }
}
pub fn font_advance_em1000(face: Option<&str>, cp: f64) -> Option<f64> {
    font(face).map(|m| font_width(m, cp, true))
}
pub fn font_width_fn(face: Option<&str>, fallback: bool) -> Option<impl Fn(f64) -> f64> {
    font(face).map(move |m| move |cp| font_width(m, cp, fallback))
}
pub fn char_width_em1000(cp: f64) -> f64 {
    if (32.0..=126.0).contains(&cp) {
        return ascii(&DATA["asciiWidth"], cp);
    }
    if let Some(w) = property(&DATA["symbolWidth"], cp) {
        return w;
    }
    if is_halfwidth_form(cp) {
        return 500.0;
    }
    for (a, b) in [(0xac00, 0xd7a3), (0x1100, 0x11ff), (0x3131, 0x318e)] {
        if (f64::from(a)..=f64::from(b)).contains(&cp) {
            return 970.0;
        }
    }
    if (0x4e00 as f64..=0x9fff as f64).contains(&cp)
        || (0xf900 as f64..=0xfaff as f64).contains(&cp)
    {
        return 1000.0;
    }
    if (0x3008 as f64..=0x3011 as f64).contains(&cp)
        || (0x3014 as f64..=0x301b as f64).contains(&cp)
    {
        return 500.0;
    }
    if cp == 0x3000 as f64 {
        return 970.0;
    }
    for (a, b) in [
        (0x2160, 0x2183),
        (0x2190, 0x22ff),
        (0x2460, 0x24ff),
        (0x25a0, 0x26ff),
        (0x3200, 0x33ff),
        (0xff01, 0xff60),
        (0x2300, 0x23ff),
        (0x2500, 0x259f),
        (0x2700, 0x27bf),
    ] {
        if (f64::from(a)..=f64::from(b)).contains(&cp) {
            return 970.0;
        }
    }
    if cp >= 0xf0000 as f64 {
        485.0
    } else if cp >= 0x2e80 as f64 {
        970.0
    } else {
        550.0
    }
}
fn fixed_face(face: &str) -> bool {
    matches!(
        face,
        "굴림체" | "돋움체" | "바탕체" | "궁서체" | "명조" | "고딕"
    )
}
pub fn face_class_of(face: Option<&str>) -> String {
    if has_font_metrics(face) {
        format!("font:{}", trim(face.unwrap()))
    } else if face.is_some_and(|f| fixed_face(trim(f))) {
        "fixedPitch".into()
    } else {
        "hcr".into()
    }
}
pub fn synthetic_bold_em(face: Option<&str>, hft: bool) -> f64 {
    let Some(face) = face.filter(|v| !v.is_empty()) else {
        return 0.0;
    };
    let f = trim(face);
    if matches!(f, "명조" | "고딕" | "필기")
        || hft
            && (f.starts_with("한양")
                || f.starts_with("휴먼")
                || matches!(f, "#세나루" | "강낭콩" | "HCI Tulip" | "신명 태고딕"))
    {
        50.0
    } else {
        0.0
    }
}
pub fn face_class_for_reflow(face: Option<&str>) -> Result<String, String> {
    // The frozen alias object inherits these non-string properties. Preserve
    // its rejection rather than silently measuring them with a fallback face.
    if face.is_some_and(|f| {
        matches!(
            trim(f),
            "constructor"
                | "__proto__"
                | "toString"
                | "valueOf"
                | "hasOwnProperty"
                | "__defineGetter__"
                | "__defineSetter__"
                | "__lookupGetter__"
                | "__lookupSetter__"
                | "isPrototypeOf"
                | "propertyIsEnumerable"
                | "toLocaleString"
        )
    }) {
        return Err("face.trim is not a function".into());
    }
    let face = face.map(|f| match trim(f) {
        "Haansoft Batang" => "한컴바탕",
        "Haansoft Dotum" => "한컴돋움",
        "HCR Batang" => "함초롬바탕",
        "HCR Dotum" => "함초롬돋움",
        _ => f,
    });
    let class = face_class_of(face);
    if class != "hcr"
        || face.is_none_or(str::is_empty)
        || face.is_some_and(|f| trim(f).starts_with("함초롬"))
    {
        Ok(class)
    } else {
        Ok("cjk".into())
    }
}
pub fn face_class_for_gen(face: Option<&str>) -> String {
    let Some(f) = face.filter(|v| !v.is_empty()).map(trim) else {
        return "hcr".into();
    };
    if has_font_metrics(Some(f)) {
        format!("font:{f}")
    } else if fixed_face(f) {
        "fixedPitch".into()
    } else if f.starts_with("함초롬") {
        "hcr".into()
    } else {
        "gothic".into()
    }
}
fn gothic_width(cp: f64) -> f64 {
    if cp >= 128.0 {
        1000.0
    } else if (65.0..=90.0).contains(&cp) {
        720.0
    } else if (97.0..=122.0).contains(&cp) {
        560.0
    } else if (48.0..=57.0).contains(&cp) {
        580.0
    } else if [40.0, 41.0, 91.0, 93.0].contains(&cp) {
        350.0
    } else if [46.0, 44.0, 58.0, 59.0, 39.0].contains(&cp) {
        280.0
    } else {
        500.0
    }
}
fn base_width(class: Option<&str>, cp: f64, fallback: bool) -> f64 {
    match class {
        Some("fixedPitch") => {
            if cp < 128.0 || is_halfwidth_form(cp) {
                500.0
            } else {
                1000.0
            }
        }
        Some("gothic") => {
            if is_halfwidth_form(cp) {
                500.0
            } else {
                gothic_width(cp)
            }
        }
        Some("cjk") => {
            if is_halfwidth_form(cp) {
                500.0
            } else if cp >= 0x1100 as f64 {
                1000.0
            } else {
                char_width_em1000(cp)
            }
        }
        Some(class) if class.starts_with("font:") => {
            font_width_fn(Some(&class[5..]), fallback).map_or_else(|| gothic_width(cp), |f| f(cp))
        }
        _ => char_width_em1000(cp),
    }
}
fn width_em(class: Option<&str>, cp: f64, fallback: bool) -> f64 {
    if (0x200b as f64..=0x200d as f64).contains(&cp)
        || cp == 0x2060 as f64
        || cp == 0xfeff as f64
        || (0x300 as f64..=0x36f as f64).contains(&cp)
    {
        return 0.0;
    }
    if fallback
        && matches!(class, Some("fixedPitch" | "cjk"))
        && let Some(w) = property(&DATA["hancomFallback"], cp)
    {
        return w;
    }
    if cp >= 0xf0000 as f64 {
        return if cp <= 0xf02fb as f64 || (0xf035f as f64..=0xf03f4 as f64).contains(&cp) {
            1000.0
        } else {
            500.0
        };
    }
    if cp == 0xf0a0 as f64 {
        return base_width(class, 32.0, fallback);
    }
    if !matches!(class, Some("hcr" | "font:함초롬돋움" | "font:함초롬바탕"))
        && (0x2776 as f64..=0x2793 as f64).contains(&cp)
    {
        return 1000.0;
    }
    base_width(class, cp, fallback)
}
fn is_space(cp: u32) -> bool {
    cp == 32 || cp == 160
}
fn character_em(cp: u32, opts: Option<&Value>) -> f64 {
    if cp == 0x2005 {
        250.0
    } else if is_space(cp) {
        option_num(opts, "spaceEm", SPACE_EM_FIXED)
    } else {
        width_em(
            field(opts, "faceClass").and_then(Value::as_str),
            f64::from(cp),
            true,
        )
    }
}
pub fn measure_text_width(text: &str, height: f64, ratio_pct: f64, opts: Option<&Value>) -> f64 {
    measure_text_width_ref(TextRef::Utf8(text), height, ratio_pct, opts)
}
pub fn measure_text_width_units(
    text: &[u16],
    height: f64,
    ratio_pct: f64,
    opts: Option<&Value>,
) -> f64 {
    measure_text_width_ref(TextRef::Utf16(text.into()), height, ratio_pct, opts)
}
/// JS for-of code points, including isolated UTF-16 surrogate values.
pub fn measure_text_width_ref(
    text: TextRef<'_>,
    height: f64,
    ratio_pct: f64,
    opts: Option<&Value>,
) -> f64 {
    let spacing = option_num(opts, "spacingPct", 0.0);
    let mut em = 0.0;
    for cp in text.code_points() {
        em += character_em(cp, opts) * (1.0 + spacing / 100.0);
    }
    em / 1000.0 * height * (ratio_pct / 100.0)
}
pub fn mul_div(a: f64, b: f64, c: f64) -> f64 {
    let e = a * b;
    if e < 0.0 {
        ((e - c / 2.0) / c).trunc()
    } else {
        ((e + c / 2.0) / c).trunc()
    }
}
pub fn hancom_em_width(em: f64, height: f64, ratio_pct: f64, spacing_pct: f64) -> f64 {
    let q = (height / 4.0).trunc();
    let mut w = if ratio_pct == 100.0 {
        4.0 * mul_div(em, q, 1000.0)
    } else {
        4.0 * (em * q * ratio_pct / 100000.0).trunc()
    };
    if spacing_pct != 0.0 && !spacing_pct.is_nan() {
        w += 4.0 * mul_div((w / 4.0).trunc(), spacing_pct, 100.0);
    }
    w
}
#[allow(clippy::too_many_arguments)]
pub fn hancom_char_width(
    ch: &str,
    height: f64,
    ratio_pct: f64,
    spacing_pct: f64,
    face_class: Option<&str>,
    em_add: f64,
    fallback: bool,
) -> f64 {
    let cp = ch.chars().next().map(|c| c as u32);
    hancom_char_width_cp(
        cp,
        height,
        ratio_pct,
        spacing_pct,
        face_class,
        em_add,
        fallback,
    )
}
/// None is the original empty string's undefined codePointAt(0), not NUL.
#[allow(clippy::too_many_arguments)]
pub fn hancom_char_width_cp(
    cp: Option<u32>,
    height: f64,
    ratio_pct: f64,
    spacing_pct: f64,
    face_class: Option<&str>,
    em_add: f64,
    fallback: bool,
) -> f64 {
    let mut w = if cp == Some(0x2005) || cp.is_some_and(is_space) {
        let s = (height / if cp == Some(0x2005) { 16.0 } else { 8.0 }).trunc();
        4.0 * if ratio_pct == 100.0 {
            s
        } else {
            mul_div(s, ratio_pct, 100.0)
        }
    } else {
        return hancom_em_width(
            width_em(face_class, cp.map_or(f64::NAN, f64::from), fallback) + em_add,
            height,
            ratio_pct,
            spacing_pct,
        );
    };
    if spacing_pct != 0.0 && !spacing_pct.is_nan() {
        w += 4.0 * mul_div((w / 4.0).trunc(), spacing_pct, 100.0);
    }
    w
}

fn wrap_space(ch: u32) -> bool {
    ch == 32 || ch == 0x2005
}
fn latin(ch: u32) -> bool {
    (0x21..=0x7e).contains(&ch) || (0xa1..=0x24f).contains(&ch) || (0x2018..=0x201f).contains(&ch)
}
fn split_units(text: &str, mode: &str, latin_break: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut units = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let start = at;
        if wrap_space(chars[at] as u32) {
            while at < chars.len() && wrap_space(chars[at] as u32) {
                at += 1;
            }
        } else if mode == "keep" {
            if latin_break && (0x21..=0x7e).contains(&(chars[at] as u32)) {
                at += 1;
            } else {
                if !latin_break && chars[at] == '\u{fdd0}' {
                    while at < chars.len() && chars[at] == '\u{fdd0}' {
                        at += 1;
                    }
                    if at < chars.len() && !wrap_space(chars[at] as u32) {
                        units.push(chars[start..at].iter().collect());
                        continue;
                    }
                }
                while at < chars.len()
                    && !wrap_space(chars[at] as u32)
                    && (!latin_break || !(0x21..=0x7e).contains(&(chars[at] as u32)))
                {
                    at += 1;
                }
            }
        } else {
            if !latin_break {
                while at < chars.len() && chars[at] == '\u{2060}' {
                    at += 1;
                }
                if at == chars.len() || wrap_space(chars[at] as u32) {
                    at -= 1;
                }
            }
            if !latin_break && latin(chars[at] as u32) {
                while at < chars.len() && latin(chars[at] as u32) {
                    at += 1;
                }
            } else {
                at += 1;
            }
            while at < chars.len() && chars[at] == '\u{a0}' {
                at += 1;
                if at < chars.len() && !wrap_space(chars[at] as u32) && chars[at] != '\u{a0}' {
                    if !latin_break && latin(chars[at] as u32) {
                        while at < chars.len() && latin(chars[at] as u32) {
                            at += 1;
                        }
                    } else {
                        at += 1;
                    }
                }
            }
        }
        units.push(chars[start..at].iter().collect());
    }
    units
}
pub fn prepare_wrap(text: &str, mode: &str, opts: Option<&Value>) -> Value {
    let mut advances = Vec::new();
    let mut steps = Vec::new();
    let mut at = 0;
    for ch in text.chars() {
        advances.resize(at + 1, Value::Undefined);
        steps.resize(at + 1, Value::Undefined);
        advances[at] = json!(character_em(ch as u32, opts));
        steps[at] = json!(ch.len_utf16());
        at += ch.len_utf16();
    }
    json!({"units":split_units(text,mode,false),"advances":advances,"steps":steps})
}

fn cp_at(text: &[u16], at: usize) -> (u32, usize) {
    let Some(&first) = text.get(at) else {
        return (0, 1);
    };
    if (0xd800..=0xdbff).contains(&first)
        && let Some(&second) = text.get(at + 1)
        && (0xdc00..=0xdfff).contains(&second)
    {
        return (
            0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00),
            2,
        );
    }
    (u32::from(first), 1)
}
fn prev_start(text: &[u16], end: usize) -> isize {
    end as isize
        - if end >= 2
            && (0xdc00..=0xdfff).contains(&text[end - 1])
            && (0xd800..=0xdbff).contains(&text[end - 2])
        {
            2
        } else {
            1
        }
}
fn set_contains(key: &str, ch: Option<u16>) -> bool {
    ch.is_some_and(|ch| {
        DATA[key]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().encode_utf16().next() == Some(ch))
    })
}
struct WrapResult {
    starts: Vec<usize>,
    last: f64,
}
impl WrapResult {
    fn value(self) -> Value {
        json!({"lines":self.starts.len(),"starts":self.starts,"lastLineWidth":self.last})
    }
}
struct Wrap<'a, 'b, 'c> {
    text: Vec<u16>,
    opts: Option<&'a Value>,
    prepared: Option<&'a Value>,
    widths: Option<&'a Value>,
    spacings: Option<&'a Value>,
    line_width: &'b mut Option<&'c mut dyn FnMut(usize) -> f64>,
    line_offset: usize,
    k: f64,
    spacing: f64,
    condense: f64,
    hancom: bool,
    starts: Vec<usize>,
    line_w: f64,
    avail: f64,
    cont: f64,
    space_w: f64,
    real_seen: bool,
    line_ended: bool,
    placed: usize,
}
impl Wrap<'_, '_, '_> {
    fn line_start(&self) -> usize {
        *self.starts.last().unwrap()
    }
    fn empty(&self) -> bool {
        self.line_w == 0.0 && (!self.hancom || self.placed == 0)
    }
    fn cap(&self) -> f64 {
        self.avail
            + if self.condense > 0.0 {
                (self.space_w * self.condense / 100.0).trunc()
            } else {
                0.0
            }
            + 0.5
    }
    fn available(&mut self, line: usize) -> f64 {
        if let Some(callback) = self.line_width.as_deref_mut() {
            callback(self.line_offset + line)
        } else {
            self.cont
        }
    }
    fn last_spacing(&self, from: usize, to: usize) -> f64 {
        if self.spacings.is_none() || to <= from {
            return 0.0;
        }
        if let Some(tabs) = field(self.opts, "rightTabs").and_then(Value::as_array)
            && tabs.iter().any(|v| {
                let t = numeric(Some(v));
                t >= self.line_start() as f64 && t < to as f64
            })
        {
            return 0.0;
        }
        let mut at = to - 1;
        if at > from
            && self
                .text
                .get(at)
                .is_some_and(|v| (0xdc00..=0xdfff).contains(v))
        {
            at -= 1;
        }
        at_num(self.spacings, at, 0.0)
    }
    fn char_w(&self, at: usize, cp: u32, len: usize) -> f64 {
        if self.widths.is_some() {
            return at_num(self.widths, at, 0.0)
                + if len == 2 {
                    at_num(self.widths, at + 1, 0.0)
                } else {
                    0.0
                };
        }
        if let Some(prepared) = self.prepared {
            return numeric(index(prepared.get("advances"), at))
                * (1.0 + self.spacing / 100.0)
                * self.k;
        }
        character_em(cp, self.opts) * (1.0 + self.spacing / 100.0) * self.k
    }
    fn range_w(&self, from: usize, to: usize) -> f64 {
        let mut w = 0.0;
        let mut at = from;
        if let Some(prepared) = self.prepared
            && self.widths.is_none()
        {
            while at < to {
                w += numeric(index(prepared.get("advances"), at))
                    * (1.0 + self.spacing / 100.0)
                    * self.k;
                let step = numeric(index(prepared.get("steps"), at));
                // Prepared data created by prepare_wrap always advances by 1/2.
                // Malformed data must not turn a local call into an endless loop.
                if !step.is_finite() || step < 1.0 || step.fract() != 0.0 {
                    return f64::NAN;
                }
                let Some(next) = at.checked_add(step as usize) else {
                    return f64::NAN;
                };
                at = next;
            }
        } else {
            while at < to {
                let (cp, len) = cp_at(&self.text, at);
                w += self.char_w(at, cp, len);
                at += len;
            }
        }
        w
    }
    fn forbid_start(&self, ch: Option<u16>) -> bool {
        set_contains(
            if self.hancom {
                "hancomForbidStart"
            } else {
                "forbidStart"
            },
            ch,
        )
    }
    fn forbid_end(&self, ch: Option<u16>) -> bool {
        set_contains(
            if self.hancom {
                "hancomForbidEnd"
            } else {
                "forbidEnd"
            },
            ch,
        )
    }
    fn is_space_at(&self, at: Option<usize>) -> bool {
        at.and_then(|i| self.text.get(i))
            .is_some_and(|&c| wrap_space(u32::from(c)))
    }
    fn break_before(&mut self, unit_pos: usize, w: f64, split: bool) {
        let mut bp = unit_pos;
        let u = self.text.get(unit_pos).copied();
        let old = truthy(field(self.opts, "before2024"));
        let split_in = self.hancom
            && split
            && u.is_some()
            && !u.is_some_and(|c| wrap_space(u32::from(c)))
            && !self.is_space_at(bp.checked_sub(1));
        let in_word = old && split_in;
        if !split_in
            && u.is_some()
            && self.forbid_start(u)
            && prev_start(&self.text, bp) > self.line_start() as isize
            && !self.is_space_at(bp.checked_sub(1))
        {
            bp = prev_start(&self.text, bp) as usize;
            if old
                && self.forbid_start(self.text.get(bp).copied())
                && prev_start(&self.text, bp) > self.line_start() as isize
                && !self.is_space_at(bp.checked_sub(1))
            {
                bp = prev_start(&self.text, bp) as usize;
            }
        }
        while !in_word
            && bp > 0
            && bp > self.line_start()
            && self.forbid_end(self.text.get(bp - 1).copied())
        {
            bp -= 1;
        }
        if bp <= self.line_start() {
            bp = unit_pos;
        }
        self.starts.push(bp);
        self.avail = self.available(self.starts.len() - 1);
        self.space_w = 0.0;
        self.real_seen = w > 0.0;
        self.line_ended = false;
        self.placed = usize::from(unit_pos > bp);
        for at in bp..unit_pos {
            if !self.is_space_at(Some(at)) {
                self.real_seen = true;
            } else if self.text[at] == 32 && self.condense > 0.0 && (self.real_seen || !self.hancom)
            {
                self.space_w += self.range_w(at, at + 1) - at_num(self.spacings, at, 0.0);
            }
        }
        self.line_w = self.range_w(bp, unit_pos) + w;
    }
    fn break_at(&mut self, at: usize, lead_w: f64) {
        self.starts.push(at);
        self.avail = self.available(self.starts.len() - 1);
        self.space_w = 0.0;
        self.real_seen = false;
        self.line_ended = false;
        self.placed = 0;
        self.line_w = lead_w;
    }
}

#[allow(clippy::too_many_arguments)]
fn simulate_inner(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    ratio_pct: f64,
    mode: &str,
    opts: Option<&Value>,
    line_width: &mut Option<&mut dyn FnMut(usize) -> f64>,
    line_offset: usize,
) -> Result<WrapResult, String> {
    let prepared = field(opts, "prepared").filter(|v| truthy(Some(v)));
    if prepared.is_none() && text.contains('\n') {
        let mut starts = Vec::new();
        let mut off = 0;
        let mut last = 0.0;
        for part in text.split('\n') {
            let stop = off + part.encode_utf16().count();
            let line = starts.len();
            let mut subopts = opts.and_then(Value::as_object).cloned().unwrap_or_default();
            for key in ["widths", "spacings"] {
                if let Some(array) = field(opts, key).and_then(Value::as_array) {
                    subopts.insert(
                        key.into(),
                        json!(&array[off.min(array.len())..stop.min(array.len())]),
                    );
                }
            }
            let sub = simulate_inner(
                part,
                if line == 0 { first_width } else { cont_width },
                cont_width,
                height,
                ratio_pct,
                mode,
                Some(&Value::Object(subopts)),
                line_width,
                line_offset + line,
            )?;
            starts.extend(sub.starts.iter().map(|start| off + start));
            last = sub.last;
            off = stop + 1;
        }
        return Ok(WrapResult { starts, last });
    }
    let units = if let Some(units) = prepared
        .and_then(|p| p.get("units"))
        .filter(|v| !v.is_null())
    {
        units
            .as_array()
            .ok_or("준비된 줄바꿈 단위가 배열이 아닙니다.")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("준비된 줄바꿈 단위가 문자열이 아닙니다.")
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        split_units(text, mode, truthy(field(opts, "latinBreak")))
    };
    let initial = if let Some(callback) = line_width.as_deref_mut() {
        callback(line_offset)
    } else {
        first_width
    };
    let mut wrap = Wrap {
        text: text.encode_utf16().collect(),
        opts,
        prepared,
        widths: field(opts, "widths").filter(|v| truthy(Some(v))),
        spacings: field(opts, "spacings").filter(|v| truthy(Some(v))),
        line_width,
        line_offset,
        k: height * ratio_pct / 100.0 / 1000.0,
        spacing: option_num(opts, "spacingPct", 0.0),
        condense: option_num(opts, "condensePct", 0.0),
        hancom: field(opts, "kinsoku").and_then(Value::as_str) == Some("hancom"),
        starts: vec![0],
        line_w: 0.0,
        avail: initial,
        cont: cont_width,
        space_w: 0.0,
        real_seen: false,
        line_ended: false,
        placed: 0,
    };
    let mut pos = 0;
    for unit in units {
        let u: Vec<u16> = unit.encode_utf16().collect();
        if u.first().is_some_and(|&c| wrap_space(u32::from(c))) {
            if wrap.hancom {
                let mut k = 0;
                while k < u.len() {
                    if !wrap.line_ended && wrap.line_w >= wrap.avail {
                        wrap.line_ended = true;
                    }
                    if wrap.line_ended && (k >= 2 || u[k] == 0x2005) && pos + k > wrap.line_start()
                    {
                        break;
                    }
                    if !wrap.line_ended {
                        let sw = wrap.range_w(pos + k, pos + k + 1);
                        wrap.line_w += sw;
                        if wrap.real_seen && u[k] == 32 {
                            wrap.space_w += sw - at_num(wrap.spacings, pos + k, 0.0);
                        }
                    }
                    k += 1;
                }
                if k < u.len() {
                    let lead = wrap.range_w(pos + k, pos + u.len());
                    wrap.break_at(pos + k, lead);
                }
                pos += u.len();
                continue;
            }
            wrap.line_w += wrap.range_w(pos, pos + u.len());
            for (k, &ch) in u.iter().enumerate() {
                if ch == 32 {
                    wrap.space_w += wrap.range_w(pos + k, pos + k + 1);
                }
            }
            pos += u.len();
            continue;
        }
        if unit == "\u{fdd0}" {
            wrap.placed += 1;
            pos += u.len();
            continue;
        }
        if unit == "\u{3000}"
            && !wrap.empty()
            && wrap.line_w + wrap.range_w(pos, pos + 1) - wrap.last_spacing(pos, pos + 1)
                > wrap.cap()
        {
            wrap.line_w += wrap.range_w(pos, pos + 1);
            wrap.placed += 1;
            pos += u.len();
            continue;
        }
        if wrap.line_ended {
            wrap.break_at(pos, 0.0);
        }
        let w = wrap.range_w(pos, pos + u.len());
        if wrap.line_w + w - wrap.last_spacing(pos, pos + u.len()) <= wrap.cap() {
            wrap.line_w += w;
            wrap.real_seen = true;
            wrap.placed += 1;
            pos += u.len();
            continue;
        }
        // JS short-circuiting is observable when lineWidth is a callback.
        let oversized = if wrap.empty() {
            true
        } else {
            let available = wrap.available(wrap.starts.len());
            w > available + 0.5
        };
        if oversized {
            let mut initial_latin = unit.chars().skip_while(|&c| c == '\u{2060}');
            let starts_latin = initial_latin.next().is_some_and(|c| latin(c as u32))
                && initial_latin.next().is_some_and(|c| latin(c as u32));
            if (mode == "keep" || starts_latin) && !wrap.empty() {
                wrap.break_before(pos, 0.0, false);
            }
            let mut sub = 0;
            for ch in unit.chars() {
                let len = ch.len_utf16();
                let c = wrap.char_w(pos + sub, ch as u32, len);
                if wrap.line_w + c - wrap.last_spacing(pos + sub, pos + sub + len) > wrap.cap()
                    && !wrap.empty()
                    && ch != '\u{3000}'
                    && ch != '\u{fdd0}'
                {
                    wrap.break_before(pos + sub, 0.0, true);
                }
                wrap.line_w += c;
                wrap.real_seen = true;
                wrap.placed += 1;
                sub += len;
            }
            pos += u.len();
            continue;
        }
        wrap.break_before(pos, w, false);
        wrap.placed += 1;
        pos += u.len();
    }
    Ok(WrapResult {
        starts: wrap.starts,
        last: wrap.line_w,
    })
}
pub fn simulate_wrap(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    ratio_pct: f64,
    mode: &str,
    opts: Option<&Value>,
) -> Result<Value, String> {
    Ok(simulate_inner(
        text,
        first_width,
        cont_width,
        height,
        ratio_pct,
        mode,
        opts,
        &mut None,
        0,
    )?
    .value())
}
#[allow(clippy::too_many_arguments)]
pub fn simulate_wrap_with_line_width(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    ratio_pct: f64,
    mode: &str,
    opts: Option<&Value>,
    callback: &mut dyn FnMut(usize) -> f64,
) -> Result<Value, String> {
    Ok(simulate_inner(
        text,
        first_width,
        cont_width,
        height,
        ratio_pct,
        mode,
        opts,
        &mut Some(callback),
        0,
    )?
    .value())
}
pub fn simulate_wrap_keep_word(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    ratio_pct: f64,
    opts: Option<&Value>,
) -> Result<Value, String> {
    simulate_wrap(
        text,
        first_width,
        cont_width,
        height,
        ratio_pct,
        "keep",
        opts,
    )
}
pub fn fit_ratio_for_fewer_lines(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    base_ratio: f64,
    min_ratio: f64,
    opts: Option<&Value>,
) -> Result<Option<f64>, String> {
    fit_inner(
        text,
        first_width,
        cont_width,
        height,
        base_ratio,
        min_ratio,
        opts,
        &mut None,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn fit_ratio_for_fewer_lines_with_line_width(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    base_ratio: f64,
    min_ratio: f64,
    opts: Option<&Value>,
    callback: &mut dyn FnMut(usize) -> f64,
) -> Result<Option<f64>, String> {
    fit_inner(
        text,
        first_width,
        cont_width,
        height,
        base_ratio,
        min_ratio,
        opts,
        &mut Some(callback),
    )
}
#[allow(clippy::too_many_arguments)]
fn fit_inner(
    text: &str,
    first_width: f64,
    cont_width: f64,
    height: f64,
    base_ratio: f64,
    min_ratio: f64,
    opts: Option<&Value>,
    callback: &mut Option<&mut dyn FnMut(usize) -> f64>,
) -> Result<Option<f64>, String> {
    let base = simulate_inner(
        text,
        first_width,
        cont_width,
        height,
        base_ratio,
        "keep",
        opts,
        callback,
        0,
    )?;
    if base.starts.len() < 2 {
        return Ok(None);
    }
    let mut ratio = base_ratio - 1.0;
    while ratio >= min_ratio {
        let simulation = simulate_inner(
            text,
            first_width,
            cont_width,
            height,
            ratio,
            "keep",
            opts,
            callback,
            0,
        )?;
        if simulation.starts.len() < base.starts.len() {
            return Ok(Some(ratio));
        }
        ratio -= 1.0;
    }
    Ok(None)
}

/// Read-only differential-test bridge; all results are produced by Rust.
/// Input: {operation: snake_case, input: object}, or batch with input requests.
#[cfg(test)]
pub(super) fn run(request: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(&Value::from_json(request)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(request: &Value) -> Result<Value, String> {
    let op = request["operation"]
        .as_str()
        .ok_or("측정 연산 이름이 없습니다.")?;
    if op == "data" {
        return Ok(DATA.clone());
    }
    if op == "batch" {
        return request["input"]
            .as_array()
            .ok_or("측정 요청 배열이 없습니다.")?
            .iter()
            .map(run_native)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    let input = &request["input"];
    let face = match input.get("face") {
        Some(Value::String(face)) => Some(face.as_str()),
        other if !truthy(other) => None,
        _ => return Err("글꼴 이름이 문자열이 아닙니다.".into()),
    };
    let n = |key: &str| numeric(input.get(key));
    let cp = n("cp");
    if matches!(
        op,
        "measure_text_width_units" | "hancom_char_width_units" | "glyph_width_units"
    ) {
        let units = input["units"]
            .as_array()
            .ok_or("UTF-16 배열이 없습니다.")?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or("UTF-16 단위가 올바르지 않습니다.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cp = TextRef::Utf16(units.as_slice().into()).code_points().next();
        let report = |n: f64| json!({"value":n,"nan":n.is_nan(),"positiveInfinity":n == f64::INFINITY,"negativeInfinity":n == f64::NEG_INFINITY,"negativeZero":n == 0.0 && n.is_sign_negative()});
        let fallback = input.get("fallback").is_none_or(|v| truthy(Some(v)));
        return match op {
            "measure_text_width_units" => {
                let opts = input.get("opts");
                if field(opts, "faceClass").is_some_and(|v| !v.is_null() && !v.is_string()) {
                    return Err("글꼴 분류가 문자열이 아닙니다.".into());
                }
                Ok(report(measure_text_width_units(
                    &units,
                    n("height"),
                    n("ratioPct"),
                    opts,
                )))
            }
            "hancom_char_width_units" => Ok(report(hancom_char_width_cp(
                cp,
                n("height"),
                n("ratioPct"),
                n("spacingPct"),
                input.get("faceClass").and_then(Value::as_str),
                input.get("emAdd").map_or(0.0, |v| numeric(Some(v))),
                fallback,
            ))),
            _ => {
                let (w, sp) = super::glyph::glyph_width_cp(cp, &input["style"], fallback)?;
                Ok(json!({"w":report(w),"sp":report(sp)}))
            }
        };
    }
    match op {
        "font_space_em" => return Ok(json!(font_space_em(face))),
        "has_font_metrics" => return Ok(json!(has_font_metrics(face))),
        "font_has_glyph" => return Ok(json!(font_has_glyph(face, cp))),
        "font_advance_em1000" => return Ok(json!(font_advance_em1000(face, cp))),
        "font_width_fn" => {
            return Ok(json!(
                font_width_fn(face, input.get("fallback").is_none_or(|v| truthy(Some(v))))
                    .map(|f| f(cp))
            ));
        }
        "is_halfwidth_form" => return Ok(json!(is_halfwidth_form(cp))),
        "char_width_em1000" => return Ok(json!(char_width_em1000(cp))),
        "face_class_of" => return Ok(json!(face_class_of(face))),
        "face_class_for_reflow" => return Ok(json!(face_class_for_reflow(face)?)),
        "face_class_for_gen" => return Ok(json!(face_class_for_gen(face))),
        "synthetic_bold_em" => return Ok(json!(synthetic_bold_em(face, truthy(input.get("hft"))))),
        "mul_div" => return Ok(json!(mul_div(n("a"), n("b"), n("c")))),
        "hancom_em_width" => {
            return Ok(json!(hancom_em_width(
                n("em"),
                n("height"),
                n("ratioPct"),
                n("spacingPct")
            )));
        }
        _ => {}
    }
    let text = input["text"].as_str().ok_or("측정할 문자열이 없습니다.")?;
    let opts = input.get("opts");
    if field(opts, "faceClass").is_some_and(|v| !v.is_null() && !v.is_string()) {
        return Err("글꼴 분류가 문자열이 아닙니다.".into());
    }
    let mode = input.get("mode").and_then(Value::as_str).unwrap_or("keep");
    match op {
        "measure_text_width" => Ok(json!(measure_text_width(
            text,
            n("height"),
            n("ratioPct"),
            opts
        ))),
        "hancom_char_width" => Ok(json!(hancom_char_width(
            text,
            n("height"),
            n("ratioPct"),
            n("spacingPct"),
            input.get("faceClass").and_then(Value::as_str),
            input.get("emAdd").map_or(0.0, |v| numeric(Some(v))),
            input.get("fallback").is_none_or(|v| truthy(Some(v)))
        ))),
        "prepare_wrap" => Ok(prepare_wrap(text, mode, opts)),
        "simulate_wrap" | "simulate_wrap_keep_word" | "fit_ratio_for_fewer_lines" => {
            let mode = if op == "simulate_wrap_keep_word" {
                "keep"
            } else {
                mode
            };
            if let Some(widths) = input.get("lineWidths").and_then(Value::as_array) {
                let mut calls = Vec::new();
                let mut callback = |i: usize| {
                    calls.push(i);
                    at_num(Some(&Value::Array(widths.clone())), i, n("contWidth"))
                };
                let result = if op == "fit_ratio_for_fewer_lines" {
                    json!(fit_ratio_for_fewer_lines_with_line_width(
                        text,
                        n("firstWidth"),
                        n("contWidth"),
                        n("height"),
                        n("baseRatio"),
                        n("minRatio"),
                        opts,
                        &mut callback
                    )?)
                } else {
                    simulate_wrap_with_line_width(
                        text,
                        n("firstWidth"),
                        n("contWidth"),
                        n("height"),
                        n("ratioPct"),
                        mode,
                        opts,
                        &mut callback,
                    )?
                };
                Ok(json!({"result":result,"calls":calls}))
            } else if op == "fit_ratio_for_fewer_lines" {
                Ok(json!(fit_ratio_for_fewer_lines(
                    text,
                    n("firstWidth"),
                    n("contWidth"),
                    n("height"),
                    n("baseRatio"),
                    n("minRatio"),
                    opts
                )?))
            } else {
                simulate_wrap(
                    text,
                    n("firstWidth"),
                    n("contWidth"),
                    n("height"),
                    n("ratioPct"),
                    mode,
                    opts,
                )
            }
        }
        _ => Err(format!("알 수 없는 측정 연산: {op}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_tables_preserve_faces_aliases_and_fallback_switch() {
        assert_eq!(DATA["fonts"].as_object().unwrap().len(), 181);
        assert_eq!(
            face_class_for_reflow(Some(" HCR Batang ")).unwrap(),
            face_class_of(Some("함초롬바탕"))
        );
        assert_eq!(face_class_for_gen(Some("없는 가상 글꼴")), "gothic");
        assert_eq!(
            face_class_for_reflow(Some("없는 가상 글꼴")).unwrap(),
            "cjk"
        );
        assert_eq!(font_advance_em1000(Some("없는 가상 글꼴"), 65.0), None);
        assert!(font_has_glyph(Some("굴림"), 65.0));
        assert_eq!(
            font_width_fn(Some("굴림"), false).unwrap()(0xff61 as f64),
            500.0
        );
        assert_eq!(synthetic_bold_em(Some("한양신명조"), true), 50.0);
        assert_eq!(synthetic_bold_em(Some("한양신명조"), false), 0.0);
    }
    #[test]
    fn wrapping_preserves_utf16_sparse_arrays_and_explicit_empty_lines() {
        let prepared = prepare_wrap("😀A😀", "keep", None);
        assert_eq!(prepared["steps"], json!([2, Value::Undefined, 1, 2]));
        assert_eq!(prepared["advances"].as_array().unwrap().len(), 4);
        let result = simulate_wrap("😀😀", 10.0, 10.0, 10.0, 100.0, "keep", None).unwrap();
        assert_eq!(result["starts"], json!([0, 2]));
        let result = simulate_wrap("A\n\nB\n", 100.0, 100.0, 10.0, 100.0, "keep", None).unwrap();
        assert_eq!(result["starts"], json!([0, 2, 3, 5]));
        assert_eq!(result["lastLineWidth"], 0.0);
    }
    #[test]
    fn prepared_and_explicit_widths_keep_spacing_and_callbacks() {
        let text = "A😀 B한";
        let opts = json!({"faceClass":"hcr","spacingPct":7.5});
        let direct = simulate_wrap(text, 15.0, 20.0, 12.0, 93.0, "break", Some(&opts)).unwrap();
        let mut prepared = opts.clone();
        prepared["prepared"] = prepare_wrap(text, "break", Some(&opts));
        assert_eq!(
            simulate_wrap(text, 15.0, 20.0, 12.0, 93.0, "break", Some(&prepared)).unwrap(),
            direct
        );
        let widths = json!({"widths":[2,3,4,1,2,9],"spacings":[0,1,0,0,0,1],"kinsoku":"hancom","rightTabs":[1]});
        let mut calls = Vec::new();
        let mut callback = |index| {
            calls.push(index);
            10.0
        };
        let result = simulate_wrap_with_line_width(
            text,
            1.0,
            1.0,
            10.0,
            100.0,
            "break",
            Some(&widths),
            &mut callback,
        )
        .unwrap();
        assert_eq!(calls.first(), Some(&0));
        assert!(result["lines"].as_u64().unwrap() > 1);
    }
    #[test]
    fn utf16_measurement_keeps_lone_units_and_combines_only_valid_pairs() {
        let units = [0xd800, 0x41, 0xdc00, 0xd83d, 0xde00, 0x1100, 0x1161];
        let mut expected = 0.0;
        for cp in [0xd800, 0x41, 0xdc00, 0x1f600, 0x1100, 0x1161] {
            expected += character_em(cp, None);
        }
        assert_eq!(
            measure_text_width_units(&units, 1000.0, 100.0, None),
            expected / 1000.0 * 1000.0
        );
        let valid = "😀가가 A\u{2005}";
        let opts = json!({"faceClass":"font:굴림","spacingPct":7.5});
        assert_eq!(
            measure_text_width_units(
                &valid.encode_utf16().collect::<Vec<_>>(),
                1100.5,
                93.3,
                Some(&opts)
            ),
            measure_text_width(valid, 1100.5, 93.3, Some(&opts))
        );
        assert_eq!(measure_text_width_units(&[], 0.0, 100.0, None), 0.0);
        assert!(measure_text_width_units(&[0xd800], f64::NAN, 100.0, None).is_nan());
    }
    #[test]
    fn quantization_and_special_widths_follow_original_arithmetic() {
        assert_eq!(mul_div(-5.0, 1.0, 2.0), -3.0);
        assert_eq!(mul_div(5.0, 1.0, 2.0), 3.0);
        assert_eq!(
            hancom_char_width(" ", 1100.0, 100.0, 0.0, Some("hcr"), 500.0, true),
            548.0
        );
        assert_eq!(
            hancom_char_width("\u{2005}", 1100.0, 100.0, 0.0, None, 0.0, true),
            272.0
        );
        assert_eq!(
            measure_text_width("\u{200b}\u{2060}\u{301}", 10.0, 100.0, None),
            0.0
        );
        assert_eq!(
            measure_text_width("\u{f0000}\u{f0300}", 10.0, 100.0, None),
            15.0
        );
    }
}
