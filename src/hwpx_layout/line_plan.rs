//! Derived from kordoc (MIT), Copyright 2026 chrisryugj; data/kordoc-LICENSE.
//! svg-content.mjs line planning. Native numbers survive until the probe boundary.
//! Character entries retain build_para's UTF-16 slots (including empty fillers).
use super::{
    glyph, head_styles,
    layout::{maximum, minimum},
    layout_text::{TextRef, control_text},
    metrics, para_model,
    value::{Value, native_json as json, number},
};
use std::collections::HashMap;

const WORK_LIMIT: usize = 2_000_000;
struct Work(usize);
impl Work {
    fn step(&mut self) -> Result<(), String> {
        self.0 += 1;
        if self.0 > WORK_LIMIT {
            Err("줄 계획 계산 한도를 넘었습니다.".into())
        } else {
            Ok(())
        }
    }
}
fn truth(v: &Value) -> bool {
    super::numbering::truth(v)
}
fn nullish(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], String> {
    v[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} 배열이 필요합니다."))
}
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b || a.is_nan() && b.is_nan(),
        _ => a == b,
    }
}
fn number_key(n: f64) -> u64 {
    if n.is_nan() {
        f64::NAN.to_bits()
    } else if n == 0.0 {
        0
    } else {
        n.to_bits()
    }
}
fn get<'a>(styles: &'a Value, name: &str, key: &Value) -> &'a Value {
    if nullish(key) {
        return &Value::Undefined;
    }
    styles[name]
        .as_array()
        .and_then(|entries| entries.iter().rev().find(|e| same(&e[0], key)))
        .map_or(&Value::Undefined, |e| &e[1])
}
fn style(styles: &Value, key: &Value) -> Value {
    let st = get(styles, "charPr", key);
    if nullish(st) {
        head_styles::default_char()
    } else {
        st.clone()
    }
}
fn at(chars: &[Value], index: f64) -> Result<&Value, String> {
    if !index.is_finite() || index < 0.0 || index.fract() != 0.0 {
        return Err("문자 위치에 해당하는 항목이 없습니다.".into());
    }
    chars
        .get(index as usize)
        .ok_or_else(|| "문자 위치에 해당하는 항목이 없습니다.".into())
}
fn prev(chars: &[Value], index: f64) -> Option<&Value> {
    if index >= 1.0 && index.fract() == 0.0 {
        chars.get((index - 1.0) as usize)
    } else {
        None
    }
}
fn text(c: &Value) -> Result<&str, String> {
    c["ch"]
        .as_str()
        .ok_or_else(|| "문자열 ch가 필요합니다.".into())
}
fn jamo_l(cp: u32) -> bool {
    (0x1100..=0x115f).contains(&cp) || (0xa960..=0xa97c).contains(&cp)
}
fn jamo_v(cp: u32) -> bool {
    (0x1160..=0x11a7).contains(&cp) || (0xd7b0..=0xd7c6).contains(&cp)
}
fn jamo_t(cp: u32) -> bool {
    (0x11a8..=0x11ff).contains(&cp) || (0xd7cb..=0xd7fb).contains(&cp)
}
pub fn joins_jamo(previous: u32, cp: u32) -> bool {
    (jamo_v(cp) || jamo_t(cp)) && (jamo_l(previous) || jamo_v(previous))
}
fn round_js(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 || x.abs() >= 4503599627370496.0 {
        return x;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    let floor = x.floor();
    if x - floor < 0.5 { floor } else { floor + 1.0 }
}
fn measured(text: &str, st: &Value, superscript: bool) -> f64 {
    measured_text(&TextRef::Utf8(text), st, superscript)
}
fn measured_text(text: &TextRef<'_>, st: &Value, superscript: bool) -> f64 {
    let h = number(&st["height"]);
    // Math.round is floor(x+.5), with a negative-zero result near zero.
    let h = if superscript {
        round_js(h * 3.0 / 4.0)
    } else {
        h
    };
    let opts =
        json!({"spacingPct":st["spacing"],"faceClass":metrics::face_class_of(st["face"].as_str())});
    match text {
        TextRef::Utf8(text) => {
            metrics::measure_text_width(text, h, number(&st["ratio"]), Some(&opts))
        }
        TextRef::Utf16(units) => {
            metrics::measure_text_width_units(units, h, number(&st["ratio"]), Some(&opts))
        }
    }
}
fn char_w_inner(
    c: &Value,
    styles: &Value,
    previous: Option<&Value>,
    work: &mut Work,
) -> Result<f64, String> {
    let st = style(styles, &c["prId"]);
    let s = text(c)?;
    if s.is_empty()
        && let Some(t) = control_text(c)?.filter(|text| !text.is_empty())
    {
        // JS iteration retains lone surrogate values and combines only adjacent
        // pairs. Never decode lossily or remove XML controls during measurement.
        for _ in t.code_points() {
            work.step()?;
        }
        return Ok(measured_text(&t, &st, truth(&c["ctrlSup"])));
    }
    // JS codePointAt(prev.ch.length-1) observes the final UTF-16 code unit,
    // not the final scalar: a supplementary previous character yields its low surrogate.
    let mut last = previous
        .and_then(|p| p["ch"].as_str())
        .and_then(|s| s.encode_utf16().last())
        .map_or(0, u32::from);
    let mut width = 0.0;
    for ch in s.chars() {
        work.step()?;
        if !joins_jamo(last, ch as u32) {
            width += glyph::glyph_width(ch.encode_utf8(&mut [0; 4]), &st, true)?.0;
        }
        last = ch as u32;
    }
    Ok(width)
}
pub fn char_w(c: &Value, styles: &Value, previous: Option<&Value>) -> Result<f64, String> {
    char_w_inner(c, styles, previous, &mut Work(0))
}
#[derive(Clone, Debug)]
pub struct NaturalWidth {
    pub text: f64,
    pub obj: f64,
}
fn object_width(o: &Value) -> f64 {
    number(&o["omL"]) + number(&o["width"]) + number(&o["omR"])
}
fn natural(
    m: &Value,
    styles: &Value,
    start: f64,
    end: f64,
    work: &mut Work,
) -> Result<NaturalWidth, String> {
    let chars = array(m, "chars")?;
    let mut text = 0.0;
    let mut i = start;
    while i < end && i < (chars.len() as f64) {
        work.step()?;
        text += char_w_inner(at(chars, i)?, styles, prev(chars, i), work)?;
        i += 1.0;
    }
    let mut obj = 0.0;
    for o in array(m, "objs")? {
        work.step()?;
        let index = number(&o["index"]);
        if truth(&o["inline"]) && index >= start && index < end {
            obj += object_width(o);
        }
    }
    Ok(NaturalWidth { text, obj })
}
pub fn line_natural_width(
    m: &Value,
    styles: &Value,
    start: f64,
    end: f64,
) -> Result<NaturalWidth, String> {
    natural(m, styles, start, end, &mut Work(0))
}
#[derive(Clone, Debug)]
pub struct LinePlan {
    pub seg_index: usize,
    pub seg: Value,
    pub xoff: f64,
    pub scale: f64,
    pub space_add: f64,
    pub sp_from: f64,
    pub char_add: Option<f64>,
    pub start: f64,
    pub end: f64,
    pub tabs: Vec<(f64, f64)>,
    pub fixed_to: f64,
    pub text_end: f64,
    pub bullet: Option<Value>,
}
impl LinePlan {
    pub fn to_value(&self) -> Value {
        let mut v = json!({"seg":self.seg,"xoff":self.xoff,"scale":self.scale,"spaceAdd":self.space_add,"spFrom":self.sp_from,
            "start":self.start,"end":self.end,"tabs":self.tabs.iter().map(|(a,b)|json!([a,b])).collect::<Vec<_>>(),"fixedTo":self.fixed_to,"textEnd":self.text_end});
        if let Some(n) = self.char_add {
            v["charAdd"] = n.into();
        }
        if let Some(b) = &self.bullet {
            v["bullet"] = b.clone();
        }
        v
    }
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let tabs = array(v, "tabs")?
            .iter()
            .map(|e| (number(&e[0]), number(&e[1])))
            .collect();
        Ok(Self {
            seg_index: 0,
            seg: v["seg"].clone(),
            xoff: number(&v["xoff"]),
            scale: number(&v["scale"]),
            space_add: number(&v["spaceAdd"]),
            sp_from: number(&v["spFrom"]),
            char_add: v.get("charAdd").map(number),
            start: number(&v["start"]),
            end: number(&v["end"]),
            tabs,
            fixed_to: number(&v["fixedTo"]),
            text_end: number(&v["textEnd"]),
            bullet: v.get("bullet").cloned(),
        })
    }
}
struct Squeeze<'a> {
    chars: &'a [Value],
    styles: &'a Value,
    text_end: f64,
    short: f64,
}
impl Squeeze<'_> {
    fn apply(&self, from: f64, work: &mut Work) -> Result<(f64, Option<f64>, f64), String> {
        let (mut spaces, mut cap, mut glyphs) = (0.0, 0.0, 0.0);
        let mut k = from;
        while k < self.text_end {
            work.step()?;
            let c = at(self.chars, k)?;
            if c["ch"] == " " {
                spaces += 1.0;
                cap += char_w_inner(c, self.styles, prev(self.chars, k), work)? * 0.2;
            } else if truth(&c["ch"]) && !truth(&c["tab"]) {
                glyphs += 1.0;
            }
            k += 1.0;
        }
        Ok((
            if spaces > 0.0 {
                -minimum(self.short, cap) / spaces
            } else {
                0.0
            },
            if spaces > 0.0 { Some(from) } else { None },
            if self.short > cap && glyphs > 0.0 {
                -(self.short - if spaces > 0.0 { cap } else { 0.0 }) / glyphs
            } else {
                0.0
            },
        ))
    }
}
pub fn plan_lines(m: &Value, styles: &Value, num_text: &str) -> Result<Vec<LinePlan>, String> {
    let mut work = Work(0);
    let chars = array(m, "chars")?;
    let objs = array(m, "objs")?;
    let segs = array(m, "segs")?;
    let align = get(styles, "paraAlign", &m["paraPrId"]);
    let align = if nullish(align) {
        "JUSTIFY"
    } else {
        align.as_str().unwrap_or("")
    };
    let geom = get(styles, "paraGeom", &m["paraPrId"]);
    let intent = number(&geom["marginIntent"]);
    let hang = if intent < 0.0 { -intent } else { 0.0 };
    let bu = if !nullish(&geom["bullet"]) {
        geom["bullet"].clone()
    } else if !num_text.is_empty() && truth(&geom["number"]) {
        let mut v = json!({"ch":num_text});
        if let Some(map) = geom["number"].as_object() {
            for (k, value) in map.iter() {
                v[k.as_str()] = value.clone();
            }
        }
        v
    } else {
        Value::Undefined
    };
    let first_pr = chars
        .iter()
        .find(|c| c["ch"] != "")
        .map_or(Value::Null, |c| {
            if nullish(&c["prId"]) {
                Value::Null
            } else {
                c["prId"].clone()
            }
        });
    let bst = style(
        styles,
        if !nullish(&bu["charPrId"]) {
            &bu["charPrId"]
        } else {
            &first_pr
        },
    );
    let head_w = if bu["useInstWidth"] == Value::Bool(false) {
        let s = bu["ch"].as_str().ok_or("글머리 문자열이 필요합니다.")?;
        let mut w = 0.0;
        for ch in s.chars() {
            work.step()?;
            w += glyph::glyph_width(ch.encode_utf8(&mut [0; 4]), &bst, false)?.0;
        }
        w
    } else if truth(&bu) {
        let s = bu["ch"].as_str().ok_or("글머리 문자열이 필요합니다.")?;
        for _ in s.chars() {
            work.step()?;
        }
        measured(s, &bst, false)
    } else {
        0.0
    };
    let bullet_w = if truth(&bu) {
        para_model::para_head_advance(&bu, number(&bst["height"]), head_w)
    } else {
        0.0
    };
    let mut inline = HashMap::new();
    for o in objs {
        work.step()?;
        if truth(&o["inline"])
            && let Value::Number(index) = o["index"]
        {
            inline.insert(number_key(index), object_width(o));
        }
    }
    let mut plans = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        work.step()?;
        let start = number(&seg["textpos"]);
        let end = if i + 1 < segs.len() {
            number(&segs[i + 1]["textpos"])
        } else {
            maximum(chars.len() as f64, start)
        };
        let mut tabs = Vec::new();
        let indent = if i == 0 {
            maximum(
                if nullish(&geom["marginIntent"]) {
                    0.0
                } else {
                    intent
                },
                0.0,
            )
        } else {
            hang
        };
        let lead = indent
            + if i == 0 || truth(&bu["autoIndent"]) {
                bullet_w
            } else {
                0.0
            };
        let mut x = lead;
        let mut fixed_to = start;
        let mut fixed_w = 0.0;
        let mut k = start;
        while k < end && k < (chars.len() as f64) {
            work.step()?;
            let c = at(chars, k)?;
            if let Some(w) = inline.get(&number_key(k)) {
                x += w;
                k += 1.0;
                continue;
            }
            if !truth(&c["tab"]) {
                x += char_w_inner(c, styles, prev(chars, k), &mut work)?;
                k += 1.0;
                continue;
            }
            let adv = para_model::tab_advance(
                x,
                i == 0,
                Some(geom),
                if nullish(&c["tabW"]) {
                    None
                } else {
                    Some(number(&c["tabW"]))
                },
            );
            tabs.push((if k == 0.0 { 0.0 } else { k }, adv));
            x += adv;
            fixed_to = k + 1.0;
            k += 1.0;
        }
        if fixed_to > start {
            fixed_w = natural(m, styles, start, fixed_to, &mut work)?.text
                + tabs.iter().fold(0.0, |a, (_, b)| a + b);
        }
        let mut text_end = minimum(end, chars.len() as f64);
        while text_end > fixed_to && truth(&at(chars, text_end - 1.0)?["br"]) {
            work.step()?;
            text_end -= 1.0;
        }
        while text_end > fixed_to && at(chars, text_end - 1.0)?["ch"] == " " {
            work.step()?;
            text_end -= 1.0;
        }
        let mut nat = natural(m, styles, fixed_to, text_end, &mut work)?;
        for o in objs {
            work.step()?;
            let index = number(&o["index"]);
            if truth(&o["inline"]) && index >= text_end && index < end {
                nat.obj += object_width(o);
            }
        }
        nat.obj += fixed_w;
        for o in objs {
            work.step()?;
            let index = number(&o["index"]);
            if truth(&o["inline"]) && index >= start && index < fixed_to {
                nat.obj += object_width(o);
            }
        }
        let mut br_end = false;
        let mut k = start;
        while k < end && k < (chars.len() as f64) {
            work.step()?;
            if truth(&at(chars, k)?["br"]) {
                br_end = true;
            }
            k += 1.0;
        }
        let is_last = i == segs.len() - 1 || br_end;
        let (mut xoff, mut scale) = (0.0, 1.0);
        let line_w = number(&seg["horzsize"]) - lead;
        let avail = line_w - nat.obj;
        let mut last_sp = 0.0;
        if fixed_to == start {
            let mut k = text_end - 1.0;
            while k >= start && at(chars, k)?["ch"] == "" {
                work.step()?;
                k -= 1.0;
            }
            if k >= start {
                let c = at(chars, k)?;
                let st = style(styles, &c["prId"]);
                if let Some(last) = text(c)?.chars().last() {
                    last_sp = glyph::glyph_width(last.encode_utf8(&mut [0; 4]), &st, true)?.1;
                }
            }
        }
        let stretch = matches!(align, "JUSTIFY" | "DISTRIBUTE" | "DISTRIBUTE_SPACE");
        let (mut space_add, mut space_from, mut char_add) = (0.0, fixed_to, 0.0);
        let skip_leading = |work: &mut Work| -> Result<f64, String> {
            let mut f = fixed_to;
            while f < text_end {
                let c = at(chars, f)?;
                if !(c["ch"] == " " || c["ch"] == "") || truth(&c["ctrl"]) {
                    break;
                }
                work.step()?;
                f += 1.0;
            }
            Ok(f)
        };
        let sq = Squeeze {
            chars,
            styles,
            text_end,
            short: nat.text - last_sp - avail,
        };
        if nat.text > 0.0
            && stretch
            && (!is_last || align == "DISTRIBUTE" || align == "DISTRIBUTE_SPACE")
        {
            let from = skip_leading(&mut work)?;
            let mut spaces = 0.0;
            let mut k = from;
            while k < text_end {
                work.step()?;
                if at(chars, k)?["ch"] == " " {
                    spaces += 1.0;
                }
                k += 1.0;
            }
            if spaces > 0.0 && align != "DISTRIBUTE" {
                space_add = (avail - nat.text + last_sp) / spaces;
                space_from = from;
                if space_add < 0.0 {
                    let (a, f, b) = sq.apply(from, &mut work)?;
                    space_add = a;
                    if let Some(f) = f {
                        space_from = f;
                    }
                    char_add = b;
                }
            } else {
                scale = if avail > 0.0 { avail / nat.text } else { 1.0 };
            }
        } else if nat.text + nat.obj > 0.0 && (is_last || !stretch) {
            let w = nat.text + nat.obj - last_sp;
            if align == "CENTER" {
                xoff = maximum(0.0, (line_w - w) / 2.0);
            } else if align == "RIGHT" {
                xoff = maximum(0.0, line_w - w);
            }
        }
        if space_add == 0.0 && char_add == 0.0 && scale == 1.0 && nat.text - last_sp > avail {
            let from = skip_leading(&mut work)?;
            let (a, f, b) = sq.apply(from, &mut work)?;
            space_add = a;
            if let Some(f) = f {
                space_from = f;
            }
            char_add = b;
        }
        if !scale.is_finite() || scale <= 0.0 {
            scale = 1.0;
        }
        scale = minimum(4.0, maximum(0.25, scale));
        plans.push(LinePlan{seg_index:i,seg:seg.clone(),xoff:xoff+lead,scale,space_add,sp_from:space_from,char_add:if char_add!=0.0&&!char_add.is_nan(){Some(char_add)}else{None},start,end,tabs,fixed_to,text_end,bullet:if i==0&&truth(&bu){Some(json!({"ch":bu["ch"],"w":bullet_w,"prId":if nullish(&bu["charPrId"]){&first_pr}else{&bu["charPrId"]}}))}else{None}});
    }
    Ok(plans)
}
pub fn advance_to(m: &Value, styles: &Value, plan: &LinePlan, upto: f64) -> Result<f64, String> {
    let mut work = Work(0);
    let chars = array(m, "chars")?;
    let mut x = 0.0;
    let mut i = plan.start;
    while i < upto && i < (chars.len() as f64) {
        work.step()?;
        let c = at(chars, i)?;
        let stretch = i >= plan.sp_from && i < plan.text_end;
        x += if truth(&c["tab"]) {
            plan.tabs
                .iter()
                .rev()
                .find(|(k, _)| *k == i)
                .map_or(0.0, |(_, v)| *v)
        } else {
            char_w_inner(c, styles, prev(chars, i), &mut work)?
                * if i < plan.fixed_to { 1.0 } else { plan.scale }
                + if stretch && c["ch"] == " " {
                    plan.space_add
                } else {
                    0.0
                }
                + if stretch
                    && plan.char_add.is_some_and(|n| n != 0.0 && !n.is_nan())
                    && truth(&c["ch"])
                    && c["ch"] != " "
                {
                    plan.char_add.unwrap()
                } else {
                    0.0
                }
        };
        i += 1.0;
    }
    for o in array(m, "objs")? {
        work.step()?;
        let index = number(&o["index"]);
        if truth(&o["inline"]) && index >= plan.start && index < upto {
            x += object_width(o);
        }
    }
    Ok(x)
}
#[cfg(test)]
fn report(v: &Value) -> serde_json::Value {
    match v {
        Value::Undefined => serde_json::json!({"$undefined":true}),
        Value::Number(n) if !n.is_finite() || *n == 0.0 && n.is_sign_negative() => {
            serde_json::json!({"$number":if n.is_nan(){"NaN"}else if *n==f64::INFINITY{"Infinity"}else if *n==f64::NEG_INFINITY{"-Infinity"}else{"-0"}})
        }
        Value::Array(a) => a.iter().map(report).collect(),
        Value::Object(o) => {
            serde_json::Value::Object(o.iter().map(|(k, v)| (k.clone(), report(v))).collect())
        }
        _ => v.to_json(),
    }
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut i = Value::from_json(input);
    if let Some(xml) = input["headerXml"].as_str() {
        i["styles"] = head_styles::parse_render_styles(xml, None)?;
    }
    if let Some(xml) = input["xml"].as_str() {
        let d = super::style_dom::parse_style_xml(xml)?;
        let mut p = d.root;
        for step in input["path"].as_array().into_iter().flatten() {
            p = *d.nodes[p]
                .children
                .get(step.as_u64().ok_or("path 번호 오류")? as usize)
                .ok_or("path 범위 오류")?;
        }
        let mut ctx = para_model::Context::default();
        para_model::prepare_deleted_ranges(&d, d.root, 0, &mut ctx);
        i["model"] = para_model::build_para(&d, p, &mut ctx)?;
    }
    if let Some(patches) = input["nativeNumbers"].as_array() {
        for p in patches {
            let n = match p["value"].as_str() {
                Some("NaN") => f64::NAN,
                Some("Infinity") => f64::INFINITY,
                Some("-Infinity") => f64::NEG_INFINITY,
                Some("-0") => -0.0,
                _ => return Err("특수 숫자 오류".into()),
            };
            let mut target = &mut i;
            for k in p["path"].as_array().ok_or("특수 숫자 경로 오류")? {
                target = if let Some(k) = k.as_str() {
                    target.get_mut(k)
                } else {
                    target.get_mut(k.as_u64().ok_or("특수 숫자 키 오류")? as usize)
                }
                .ok_or("특수 숫자 항목 없음")?;
            }
            *target = n.into();
        }
    }
    let m = &i["model"];
    let styles = &i["styles"];
    let value = match op {
        "joinsJamo" => joins_jamo(number(&i["previous"]) as u32, number(&i["cp"]) as u32).into(),
        "charW" => char_w(&i["char"], styles, i.get("previous"))?.into(),
        "lineNaturalWidth" => {
            let w = line_natural_width(m, styles, number(&i["start"]), number(&i["end"]))?;
            json!({"text":w.text,"obj":w.obj})
        }
        "planLines" => plan_lines(m, styles, i["numText"].as_str().unwrap_or(""))?
            .iter()
            .map(LinePlan::to_value)
            .collect::<Vec<_>>()
            .into(),
        "advanceTo" => advance_to(
            m,
            styles,
            &LinePlan::from_value(&i["plan"])?,
            number(&i["upto"]),
        )?
        .into(),
        "planAndAdvance" => {
            let plans = plan_lines(m, styles, i["numText"].as_str().unwrap_or(""))?;
            let mut values = Vec::new();
            for p in &plans {
                let mut advances = Vec::new();
                for upto in array(&i, "uptos")? {
                    advances.push(advance_to(m, styles, p, number(upto))?);
                }
                values.push(json!({"plan":p.to_value(),"advances":advances}));
            }
            values.into()
        }
        _ => return Err("알 수 없는 줄 계획 연산입니다.".into()),
    };
    Ok(if input["native"].as_bool() == Some(true) {
        report(&value)
    } else {
        value.to_json()
    })
}
