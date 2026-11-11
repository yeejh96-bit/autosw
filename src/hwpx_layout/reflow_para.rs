//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! reflow.mjs reflowPara; see data/kordoc-LICENSE and PROVENANCE.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    glyph::glyph_width_cp,
    head_styles,
    layout::{maximum as max, minimum as min},
    layout_text::{TextRef, control_text},
    line_plan::joins_jamo,
    metrics,
    numbering::truth,
    para_model::{self as pm, Context, child, num},
    para_number::{CounterState, next_para_number},
    reflow_helpers,
    style_dom::ParsedXml,
    table_flow,
    value::{Value, native_json as json, number, string},
};
use std::collections::HashMap;

#[derive(Default)]
pub struct ReflowState {
    pub paper_origins: HashMap<usize, [f64; 3]>,
}
pub struct ReflowArgs<'a> {
    pub area_w: f64,
    pub start_v: f64,
    pub mode: &'a str,
    pub body_flow: bool,
    pub page_h: f64,
}
fn n(v: &Value, k: &str) -> f64 {
    number(&v[k])
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v[k].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn nullish(v: &Value) -> bool {
    matches!(v, Value::Null | Value::Undefined)
}
fn map_get<'a>(v: &'a Value, key: &Value) -> Option<&'a Value> {
    v.as_array()?
        .iter()
        .rev()
        .find(|e| &e[0] == key)
        .map(|e| &e[1])
}
fn st<'a>(styles: &'a Value, slot: &Value, default: &'a Value) -> &'a Value {
    if nullish(&slot["prId"]) {
        default
    } else {
        map_get(&styles["charPr"], &slot["prId"]).unwrap_or(default)
    }
}
fn id(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or("개체 노드가 없습니다.".into())
}
fn round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        x
    } else if (-0.5..0.0).contains(&x) {
        -0.0
    } else {
        let f = x.floor();
        if x - f < 0.5 { f } else { f + 1.0 }
    }
}
fn maxs(a: &[f64]) -> f64 {
    a.iter().copied().fold(f64::NEG_INFINITY, max)
}
fn attr<'a>(d: &'a ParsedXml, i: Option<usize>, key: &str, default: &'a str) -> &'a str {
    i.and_then(|i| d.nodes[i].attr(key)).unwrap_or(default)
}
fn has_ink(c: &Value) -> bool {
    c["ch"]
        .as_str()
        .is_some_and(|s| !crate::input::trim(s).is_empty())
}
fn cp(c: &Value) -> Option<u32> {
    c["ch"]
        .as_str()
        .and_then(|s| s.chars().next())
        .map(u32::from)
}
fn width(c: &Value, style: &Value, fallback: bool) -> Result<(f64, f64), String> {
    glyph_width_cp(cp(c), style, fallback)
}
fn pitch(h: f64, g: &Value) -> f64 {
    let v = n(g, "lineSpacingValue");
    match s(g, "lineSpacingType") {
        "PERCENT" => {
            let e = (h / 4.0).trunc() * (v - 100.0) / 100.0;
            h + 4.0 * if e == 0.0 { e } else { e.signum() } * (e.abs() + 0.5).floor()
        }
        "FIXED" => max(0.0, v),
        "AT_LEAST" => max(v, h),
        _ => round(h * 1.6),
    }
}
fn font_metrics(face: &Value, hft: bool) -> Option<(f64, f64, bool)> {
    let face = face.as_str()?;
    if face.is_empty() {
        return None;
    }
    let f = crate::input::trim(face);
    if hft && f == "한컴바탕" {
        return None;
    }
    if f.starts_with("맑은 고딕") {
        Some((2230.0 / 2048.0, 496.0 / 2048.0, true))
    } else if [
        "바탕",
        "돋움",
        "굴림",
        "궁서",
        "바탕체",
        "돋움체",
        "굴림체",
        "궁서체",
        "한컴바탕",
    ]
    .contains(&f)
        || f.starts_with("HY")
    {
        Some((1758.0 / 2048.0, 290.0 / 2048.0, false))
    } else if f == "휴먼명조" {
        Some((0.859, 0.195, false))
    } else if [
        "KoPub돋움체 Light",
        "KoPub바탕체 Light",
        "KoPubWorld돋움체 Light",
        "KoPubWorld바탕체 Light",
    ]
    .contains(&f)
    {
        Some((0.85, 0.25, false))
    } else {
        None
    }
}
fn stale_table(d: &ParsedXml, t: usize) -> bool {
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for &r in &d.nodes[t].children {
        if d.nodes[r].local != "tr" {
            continue;
        }
        for &c in &d.nodes[r].children {
            if d.nodes[c].local != "tc" {
                continue;
            }
            let ra = num(d, child(d, c, "cellAddr"), "rowAddr", 0.0);
            let rs = num(d, child(d, c, "cellSpan"), "rowSpan", 0.0);
            if (if rs == 0.0 { 1.0 } else { rs }) == 1.0 {
                let h = num(d, child(d, c, "cellSz"), "height", 0.0);
                if let Some((_, old)) = rows.iter_mut().find(|(k, _)| *k == ra) {
                    *old = max(*old, h)
                } else {
                    rows.push((ra, max(0.0, h)))
                }
            }
        }
    }
    !rows.is_empty()
        && rows.iter().map(|(_, h)| h).sum::<f64>() + 1000.0
            < num(d, child(d, t, "sz"), "height", 0.0)
}
fn draw_height(d: &ParsedXml, i: usize) -> f64 {
    let Some(dt) = child(d, i, "drawText") else {
        return 0.0;
    };
    let Some(sub) = child(d, dt, "subList") else {
        return 0.0;
    };
    let mut bottom = 0.0;
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        if let Some(a) = child(d, p, "linesegarray") {
            for &seg in &d.nodes[a].children {
                if d.nodes[seg].local == "lineseg" {
                    bottom = max(
                        bottom,
                        num(d, Some(seg), "vertpos", 0.0) + num(d, Some(seg), "vertsize", 0.0),
                    )
                }
            }
        }
    }
    let tm = child(d, dt, "textMargin");
    if bottom > 0.0 {
        bottom + num(d, tm, "top", 0.0) + num(d, tm, "bottom", 0.0)
    } else {
        0.0
    }
}
fn effective_height(
    d: &ParsedXml,
    o: &Value,
    styles: &Value,
    ctx: &mut Context,
) -> Result<f64, String> {
    let i = id(&o["el"])?;
    let h = if s(o, "tag") == "tbl" {
        // Every original geomMemo(styles) creates fresh extent caches, while the
        // document's deletion/note state remains shared.
        let old_cell = std::mem::take(&mut ctx.cell_memo);
        let old_table = std::mem::take(&mut ctx.table_memo);
        let old_geom = std::mem::take(&mut ctx.para_geom);
        let old_memo = ctx.memoize;
        ctx.memoize = true;
        ctx.para_geom = list(styles, "paraGeom")
            .iter()
            .filter_map(|e| e[0].as_str().map(|s| (s.to_owned(), e[1].clone())))
            .collect();
        let result = pm::measure_table_height(d, i, ctx);
        ctx.cell_memo = old_cell;
        ctx.table_memo = old_table;
        ctx.para_geom = old_geom;
        ctx.memoize = old_memo;
        let measured = result?;
        if stale_table(d, i) {
            measured
        } else {
            max(n(o, "height"), measured)
        }
    } else if truth(&o["inline"]) {
        max(n(o, "height"), draw_height(d, i))
    } else {
        n(o, "height")
    };
    Ok(h + pm::caption_extra(d, i))
}
fn paper_origin(d: &ParsedXml, p: usize, state: &mut ReflowState) -> Result<[f64; 3], String> {
    let root = d.document_root(p)?;
    if let Some(hit) = state.paper_origins.get(&root) {
        return Ok(*hit);
    }
    fn find(d: &ParsedXml, i: usize) -> Option<usize> {
        if d.nodes[i].name == "hp:pagePr" {
            return Some(i);
        }
        d.nodes[i].children.iter().find_map(|&c| find(d, c))
    }
    let p = find(d, root);
    let mg = p.and_then(|p| child(d, p, "margin"));
    let tb = attr(d, p, "gutterType", "") == "TOP_BOTTOM";
    let gut = num(d, mg, "gutter", 0.0);
    let result = [
        num(d, mg, "left", 8504.0) + if tb { 0.0 } else { gut },
        num(d, mg, "top", 5668.0) + num(d, mg, "header", 0.0) + if tb { gut } else { 0.0 },
        num(d, p, "width", 59528.0),
    ];
    state.paper_origins.insert(root, result);
    Ok(result)
}
fn free_parts(
    zones: &[Value],
    y0: f64,
    y1: f64,
    left: f64,
    avail: f64,
) -> (Vec<[f64; 2]>, f64, bool, String) {
    let mut free = vec![[left, left + avail]];
    let mut bottom = -1.0;
    let mut hit = false;
    let mut flow = "BOTH_SIDES".to_owned();
    for z in zones {
        if !(y1 > n(z, "top") && y0 < n(z, "bottom")) {
            continue;
        }
        hit = true;
        flow = s(z, "flow").to_owned();
        bottom = max(bottom, n(z, "bottom"));
        free = free
            .into_iter()
            .flat_map(|[a, b]| {
                if n(z, "right") <= a || n(z, "left") >= b {
                    vec![[a, b]]
                } else {
                    vec![[a, max(a, n(z, "left"))], [min(b, n(z, "right")), b]]
                }
            })
            .filter(|[a, b]| b - a > 0.0)
            .collect();
    }
    (free, bottom, hit, flow)
}
struct Boxes {
    top: f64,
    height: f64,
    pitch: f64,
    left: f64,
    avail: f64,
    before: bool,
    intent: f64,
    bullet: f64,
    margin_intent: f64,
    cont_bullet: f64,
    boxes: HashMap<usize, Option<[f64; 2]>>,
    parts: HashMap<usize, Vec<[f64; 2]>>,
    collision: Vec<f64>,
}
impl Boxes {
    fn at(&mut self, i: usize, zones: &[Value]) -> Option<[f64; 2]> {
        if zones.is_empty() {
            return None;
        }
        if let Some(b) = self.boxes.get(&i) {
            return *b;
        }
        let y = self.top + i as f64 * self.pitch;
        let h = max(self.height, *self.collision.get(i).unwrap_or(&0.0));
        let (free, _, hit, flow) = free_parts(zones, y, y + h, self.left, self.avail);
        let b = if hit {
            let pick = if flow == "LEFT_ONLY" {
                free.first().copied()
            } else if flow == "RIGHT_ONLY" {
                free.last().copied()
            } else {
                Some(
                    free.iter()
                        .copied()
                        .reduce(|a, b| if b[1] - b[0] > a[1] - a[0] { b } else { a })
                        .unwrap_or([self.left, self.left]),
                )
            };
            if flow == "BOTH_SIDES" && free.len() > 1 {
                self.parts.insert(i, free);
            }
            Some(pick.map(|[a, b]| [a, b - a]).unwrap_or([self.left, 0.0]))
        } else {
            None
        };
        self.boxes.insert(i, b);
        b
    }
    fn width(&mut self, i: usize, zones: &[Value]) -> f64 {
        let w = self
            .at(i, zones)
            .map(|b| {
                b[1] - if !self.before && b[0] > self.left {
                    self.left
                } else {
                    0.0
                }
            })
            .unwrap_or(self.avail);
        max(
            500.0,
            if i == 0 {
                w - self.intent - self.bullet
            } else {
                w + min(0.0, self.margin_intent) - self.cont_bullet
            },
        )
    }
    fn clear(&mut self, parts: bool) {
        self.boxes.clear();
        if parts {
            self.parts.clear()
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn wrap(
    text: &str,
    first: f64,
    cont: f64,
    height: f64,
    mode: &str,
    opts: &Value,
    zones: &[Value],
    boxes: &mut Boxes,
) -> Result<Value, String> {
    if text.is_empty() {
        return Ok(json!({"lines":1,"starts":[0],"lastLineWidth":0}));
    }
    if zones.is_empty() {
        metrics::simulate_wrap(text, first, cont, height, 100.0, mode, Some(opts))
    } else {
        metrics::simulate_wrap_with_line_width(
            text,
            first,
            cont,
            height,
            100.0,
            mode,
            Some(opts),
            &mut |i| boxes.width(i, zones),
        )
    }
}
fn starts(v: &Value) -> Vec<usize> {
    list(v, "starts")
        .iter()
        .filter_map(|v| v.as_u64().map(|i| i as usize))
        .collect()
}
fn line_for(starts: &[usize], real: &[usize], slot: usize) -> usize {
    let mut li = 0;
    for (k, &start) in starts.iter().enumerate().skip(1) {
        if real.get(start).is_some_and(|&i| i <= slot) {
            li = k
        }
    }
    li
}
fn put(d: &mut ParsedXml, i: usize, k: &str, v: f64) {
    d.nodes[i].set_attr(k, string(&v.into()))
}
fn check_size_limits(
    chars: usize,
    zones: usize,
    char_limit: usize,
    zone_limit: usize,
) -> Result<(), String> {
    if chars > char_limit || zones > zone_limit {
        return Err("문단 재배치 처리 한도를 넘었습니다.".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn reflow_para(
    d: &mut ParsedXml,
    p: usize,
    styles: &Value,
    args: &ReflowArgs<'_>,
    zones: &mut Vec<Value>,
    nums: &mut CounterState,
    ctx: &mut Context,
    state: &mut ReflowState,
) -> Result<Option<Value>, String> {
    let m = pm::build_para(d, p, ctx)?;
    let default_geom = head_styles::default_para_geom();
    let default_char = head_styles::default_char();
    let key = if nullish(&m["paraPrId"]) {
        Value::from("")
    } else {
        m["paraPrId"].clone()
    };
    let geom = map_get(&styles["paraGeom"], &key).unwrap_or(&default_geom);
    let num_text = next_para_number(nums, Some(geom))?;
    if !list(&m, "segs").is_empty() {
        return Ok(None);
    }
    let chars = list(&m, "chars");
    let objs = list(&m, "objs");
    check_size_limits(chars.len(), zones.len(), 1_000_000, 100_000)?;
    let mut real = Vec::new();
    let mut right_tabs = Vec::new();
    let mut right_fit = Vec::new();
    let mut widths = Vec::new();
    let mut spacings = Vec::new();
    let mut text = String::new();
    let mut text_units = 0;
    let mut x = max(n(geom, "marginIntent"), 0.0);
    let inline: HashMap<usize, &Value> = objs
        .iter()
        .filter(|o| truth(&o["inline"]))
        .filter_map(|o| o["index"].as_u64().map(|i| (i as usize, o)))
        .collect();
    for (i, c) in chars.iter().enumerate() {
        if let Some(o) = inline.get(&i) {
            let adv = n(o, "omL") + n(o, "width") + n(o, "omR");
            real.push(i);
            widths.push(adv);
            spacings.push(0.0);
            text.push('\u{fffc}');
            text_units += 1;
            x += adv;
            continue;
        }
        if truth(&c["tab"]) {
            let adv = pm::tab_advance(x, true, Some(geom), c.get("tabW").map(number));
            if truth(&c["tabRight"]) {
                right_tabs.push(text_units);
                let mut after = 0.0;
                for ck in chars.iter().skip(i + 1).take_while(|c| !truth(&c["tab"])) {
                    if truth(&ck["ch"]) {
                        after += width(ck, st(styles, ck, &default_char), true)?.0
                    }
                }
                right_fit.push((widths.len(), x, after));
            }
            real.push(i);
            widths.push(adv);
            spacings.push(0.0);
            text.push('\t');
            text_units += 1;
            x += adv;
            continue;
        }
        if truth(&c["ctrl"]) {
            let mut w = 0.0;
            if let Some(t) = control_text(c)?
                && !t.is_empty()
            {
                let mut style = st(styles, c, &default_char).clone();
                if !truth(&styles["before2024"]) {
                    style["bold"] = false.into()
                }
                if !truth(&c["ctrlSup"]) {
                    style["spacing"] = 0.into();
                    if truth(&style["latin"]) {
                        style["latin"]["spacing"] = 0.into()
                    }
                } else {
                    style["height"] = metrics::mul_div(n(&style, "height"), 3.0, 4.0).into()
                }
                for cp in t.code_points() {
                    w += glyph_width_cp(Some(cp), &style, true)?.0
                }
            }
            real.push(i);
            widths.push(w);
            spacings.push(0.0);
            text.push(if w > 0.0 {
                '\u{fffc}'
            } else if truth(&c["fieldEnd"]) {
                '\u{fdd0}'
            } else {
                '\u{2060}'
            });
            text_units += 1;
            x += w;
            continue;
        }
        if truth(&c["br"]) {
            real.push(i);
            widths.push(0.0);
            spacings.push(0.0);
            text.push('\n');
            text_units += 1;
            x = max(0.0, -n(geom, "marginIntent"));
            continue;
        }
        let ch = c["ch"].as_str().ok_or("문단 문자가 문자열이 아닙니다.")?;
        if ch.is_empty() {
            continue;
        }
        let joined = joins_jamo(
            text.chars()
                .next_back()
                .map(|ch| {
                    let mut units = [0; 2];
                    u32::from(*ch.encode_utf16(&mut units).last().unwrap())
                })
                .unwrap_or(0),
            cp(c).unwrap_or(0),
        );
        let (w, sp) = if joined {
            (0.0, 0.0)
        } else {
            width(c, st(styles, c, &default_char), true)?
        };
        for u in 0..ch.encode_utf16().count() {
            real.push(i);
            widths.push(if u == 0 { w } else { 0.0 });
            spacings.push(if u == 0 { sp } else { 0.0 });
        }
        let addition = if truth(&c["nb"]) { "\u{a0}" } else { ch };
        text.push_str(addition);
        text_units += addition.encode_utf16().count();
        x += w;
    }
    let mut dom = None;
    for c in chars {
        if c["ch"] != ""
            && !nullish(&c["prId"])
            && let Some(v) = map_get(&styles["charPr"], &c["prId"])
        {
            dom = Some(v);
            break;
        }
    }
    if dom.is_none() {
        for &r in &d.nodes[p].children {
            if d.nodes[r].local == "run"
                && let Some(v) = map_get(
                    &styles["charPr"],
                    &Value::from(d.nodes[r].attr("charPrIDRef").unwrap_or("")),
                )
            {
                dom = Some(v);
                break;
            }
        }
    }
    let dom = dom.unwrap_or(&default_char);
    let height = if truth(&dom["height"]) {
        n(dom, "height")
    } else {
        1000.0
    };
    let left = n(geom, "marginLeft");
    let avail = max(
        1440.0,
        (args.area_w / 4.0).floor() * 4.0 - left - n(geom, "marginRight"),
    );
    let generated = if !num_text.is_empty() && truth(&geom["number"]) {
        let mut b = json!({"ch":num_text});
        if let Some(fields) = geom["number"].as_object() {
            for (key, value) in fields.iter() {
                b[key.as_str()] = value.clone();
            }
        }
        b
    } else {
        Value::Undefined
    };
    let bu = if nullish(&geom["bullet"]) {
        &generated
    } else {
        &geom["bullet"]
    };
    let bst = if !nullish(&bu["charPrId"]) {
        map_get(&styles["charPr"], &bu["charPrId"]).unwrap_or(dom)
    } else {
        dom
    };
    let mut head_w = 0.0;
    if truth(bu) {
        let bt = bu["ch"]
            .as_str()
            .ok_or("글머리표 문자가 문자열이 아닙니다.")?;
        for cp in TextRef::Utf8(bt).code_points() {
            head_w += glyph_width_cp(Some(cp), bst, bu["useInstWidth"] != false)?.0
        }
    }
    let bullet = if truth(bu) {
        pm::para_head_advance(bu, height, head_w)
    } else {
        0.0
    };
    let cont_bullet = if truth(&bu["autoIndent"]) {
        bullet
    } else {
        0.0
    };
    let squeeze =
        attr(d, d.nodes[p].parent, "lineWrap", "") == "SQUEEZE" || truth(&geom["squeeze"]);
    let intent = if n(geom, "marginIntent") >= avail {
        0.0
    } else {
        max(0.0, n(geom, "marginIntent"))
    };
    let before = truth(&styles["before2024"]);
    let first = if squeeze {
        1e9
    } else {
        max(
            500.0,
            avail
                - if before && bullet > 0.0 {
                    max(intent, bullet)
                } else {
                    intent + bullet
                },
        )
    };
    let condense = if !before
        && !truth(&styles["early2024"])
        && (truth(&geom["number"]) || truth(&geom["bullet"]))
    {
        Value::from(0)
    } else {
        geom["condense"].clone()
    };
    for (wi, xx, after) in right_fit {
        let edge = avail - bullet;
        if xx + widths[wi] + after > edge && edge - xx - after >= 0.0 {
            widths[wi] = edge - xx - after
        }
    }
    let hang = avail + min(0.0, n(geom, "marginIntent")) - cont_bullet;
    let cont = if squeeze {
        1e9
    } else if hang < height {
        avail - cont_bullet
    } else {
        hang
    };
    let mode = geom["wrapMode"].as_str().unwrap_or(args.mode);
    let line_pitch = pitch(height, geom);
    let mut float_below = 0.0;
    let mut first_float_vo = 0.0;
    let mut line_floats = Vec::new();
    let mut obj_bottom = args.start_v;
    let mut cell_float_bottom = 0.0;
    let mut cell_text_below = 0.0;
    let spaces_only = table_flow::spaces_host(d, p, ctx)?;
    let zone_start = zones.len();
    let ovp = args.body_flow
        && !objs.is_empty()
        && !chars.iter().any(has_ink)
        && objs.iter().all(|o| {
            !truth(&o["inline"])
                && s(o, "tag") == "pic"
                && id(&o["el"]).ok().is_some_and(|i| {
                    attr(d, Some(i), "textWrap", "") == "TOP_AND_BOTTOM"
                        && attr(d, child(d, i, "pos"), "allowOverlap", "") == "1"
                })
        });
    let spaces_text =
        args.body_flow && (spaces_only || ovp || table_flow::bullet_empty_host(d, p, geom, ctx)?);
    let textish = chars.iter().any(|c| !truth(&c["nb"]) && has_ink(c))
        || objs.iter().any(|o| truth(&o["inline"]))
        || spaces_text;
    for o in objs {
        if truth(&o["inline"]) {
            continue;
        }
        let el = id(&o["el"])?;
        let pos = child(d, el, "pos");
        let om = child(d, el, "outMargin");
        let ot = num(d, om, "top", 0.0);
        let ob = num(d, om, "bottom", 0.0);
        let wrap_attr = attr(d, Some(el), "textWrap", "");
        if ["BEHIND_TEXT", "IN_FRONT_OF_TEXT"].contains(&wrap_attr) {
            continue;
        }
        let vr = attr(d, pos, "vertRelTo", "PARA");
        let paper_band = wrap_attr == "TOP_AND_BOTTOM" && attr(d, pos, "vertAlign", "TOP") == "TOP";
        let hr = attr(d, pos, "horzRelTo", "COLUMN");
        let ha = attr(d, pos, "horzAlign", "LEFT");
        let ho = num(d, pos, "horzOffset", 0.0);
        let ow = n(o, "width");
        let oml = num(d, om, "left", 0.0);
        let omr = num(d, om, "right", 0.0);
        if vr == "PAPER" && args.body_flow && (wrap_attr != "TOP_AND_BOTTOM" || paper_band) {
            let [ox, oy, pw] = paper_origin(d, p, state)?;
            let base = if hr == "PAPER" {
                -ox
            } else if hr == "PARA" {
                left
            } else {
                0.0
            };
            let span = if hr == "PAPER" {
                pw
            } else if hr == "PARA" {
                avail
            } else {
                args.area_w
            };
            let x0 = base
                + if ha == "RIGHT" {
                    span - ho - ow
                } else if ha == "CENTER" {
                    (span - ow) / 2.0 + ho
                } else {
                    ho
                };
            let top = max(0.0, num(d, pos, "vertOffset", 0.0)) - oy;
            let xs = if ha == "RIGHT" {
                x0 - omr
            } else if ha == "CENTER" {
                x0
            } else {
                x0 + oml
            };
            zones.push(json!({"top":top,"bottom":top+ot+effective_height(d,o,styles,ctx)?+ob,"left":if paper_band{-1e9}else{xs-oml},"right":if paper_band{1e9}else{xs+ow+omr},"flow":attr(d,Some(el),"textFlow","BOTH_SIDES"),"anchor":o["index"]}));
            continue;
        }
        if vr == "PAGE" || vr == "PAPER" {
            continue;
        }
        let vo = max(0.0, num(d, pos, "vertOffset", 0.0));
        let raw = num(d, pos, "vertOffset", 0.0);
        let signed = max(
            0.0,
            if raw >= 2147483648.0 {
                raw - 4294967296.0
            } else {
                raw
            },
        );
        let eh = effective_height(d, o, styles, ctx)?;
        let base = if hr == "PARA" { left } else { 0.0 };
        let span = if hr == "PARA" { avail } else { args.area_w };
        let x0 = base
            + if ha == "RIGHT" {
                span - ho - ow
            } else if ha == "CENTER" {
                (span - ow) / 2.0 + ho
            } else {
                ho
            };
        if wrap_attr == "TOP_AND_BOTTOM" {
            if (hr == "COLUMN" || hr == "PARA") && (x0 >= args.area_w || x0 + ow <= 0.0) {
                continue;
            }
            if args.body_flow {
                if signed == 0.0 && textish {
                    line_floats.push((id(&o["index"])?, ot + eh + ob));
                    continue;
                }
                if float_below == 0.0 {
                    first_float_vo = signed
                }
                float_below = max(float_below, signed) + ot + eh + ob;
            } else if spaces_only {
                cell_text_below = max(cell_text_below, signed + ot + eh + ob)
            } else {
                cell_float_bottom = max(cell_float_bottom, signed + ot + eh + ob)
            }
            continue;
        }
        let top = args.start_v
            - if args.body_flow && vr == "PARA" && wrap_attr == "SQUARE" {
                n(geom, "spaceBefore")
            } else {
                0.0
            }
            + vo;
        let xs = if ha == "RIGHT" {
            x0 - omr
        } else if ha == "CENTER" {
            x0
        } else {
            x0 + oml
        };
        let next = args.page_h > 0.0 && top + eh + ob > args.page_h;
        let mut z = json!({"top":if next{-1.0}else{top},"bottom":if next{-1.0}else{top+eh+ob},"left":xs-oml,"right":xs+ow+omr,"flow":attr(d,Some(el),"textFlow","BOTH_SIDES"),"anchor":if next{Value::Undefined}else{o["index"].clone()},"carry":ot+eh+ob});
        if next {
            z["nextPage"] = true.into()
        }
        zones.push(z);
    }
    let n_float = objs
        .iter()
        .filter(|o| {
            !truth(&o["inline"])
                && id(&o["el"]).ok().is_some_and(|i| {
                    attr(d, Some(i), "textWrap", "") == "TOP_AND_BOTTOM"
                        && attr(d, child(d, i, "pos"), "vertRelTo", "PARA") == "PARA"
                })
        })
        .count();
    let area_top = if args.body_flow {
        args.start_v
            - geom
                .get("spaceBefore")
                .filter(|v| !nullish(v))
                .map_or(0.0, number)
    } else {
        args.start_v
    };
    let mut float_floor = 0.0;
    if !textish && n_float < 2 {
        obj_bottom = area_top + float_below;
        if args.body_flow {
            float_floor = obj_bottom
        }
        float_below = 0.0
    }
    let mut band = None;
    if float_below > 0.0 && textish && area_top + first_float_vo > args.start_v {
        band = Some([area_top + first_float_vo, area_top + float_below]);
        float_below = 0.0
    }
    let mut top = if float_below > 0.0 {
        max(args.start_v, area_top + float_below)
    } else {
        args.start_v
    };
    if cell_text_below > 0.0 {
        top = max(top, args.start_v + cell_text_below)
    }
    let first_inline = objs.iter().find(|o| truth(&o["inline"]));
    let need = first_inline
        .filter(|o| !chars.iter().take(id(&o["index"]).unwrap_or(0)).any(has_ink))
        .map(|o| max(1440.0, n(o, "omL") + n(o, "width") + n(o, "omR")))
        .unwrap_or(1440.0);
    for _ in 0..8 {
        if zone_start == 0 {
            break;
        }
        let (free, bottom, _, _) = free_parts(&zones[..zone_start], top, top + height, left, avail);
        if bottom < 0.0 || free.iter().any(|[a, b]| b - a >= need) {
            break;
        }
        top = bottom;
    }
    let mut boxes = Boxes {
        top,
        height,
        pitch: line_pitch,
        left,
        avail,
        before,
        intent,
        bullet,
        margin_intent: n(geom, "marginIntent"),
        cont_bullet,
        boxes: HashMap::new(),
        parts: HashMap::new(),
        collision: Vec::new(),
    };
    let opts = json!({"widths":widths,"spacings":spacings,"condensePct":condense,"kinsoku":"hancom","before2024":styles["before2024"],"latinBreak":geom["latinBreak"],"rightTabs":right_tabs});
    if zones.len() > zone_start && !text.is_empty() {
        let keep = zones[..zone_start].to_vec();
        let saved = std::mem::replace(zones, keep);
        boxes.clear(false);
        // On failure the original leaves the temporary zone list in place.
        let pre = wrap(&text, first, cont, height, mode, &opts, zones, &mut boxes)?;
        *zones = saved;
        boxes.clear(false);
        let pre_starts = starts(&pre);
        for z in zones.iter_mut().skip(zone_start) {
            let Some(k) = real
                .iter()
                .position(|&i| z["anchor"].as_u64() == Some(i as u64))
            else {
                continue;
            };
            let mut li = 0;
            for (j, &start) in pre_starts.iter().enumerate().skip(1) {
                if start <= k {
                    li = j
                }
            }
            let h = n(z, "bottom") - n(z, "top");
            let lt = top + li as f64 * line_pitch;
            if lt > n(z, "top") {
                z["top"] = lt.into();
                z["bottom"] = (lt + h).into()
            } else if n(z, "top") > lt && k > pre_starts[li] {
                z["top"] = max(n(z, "top"), lt + line_pitch).into()
            }
        }
    }
    if zones.len() > zone_start && textish {
        for _ in 0..8 {
            let (free, bottom, _, _) = free_parts(zones, top, top + height, left, avail);
            if bottom < 0.0 || free.iter().any(|[a, b]| b - a >= 1440.0) {
                break;
            }
            top = bottom;
            boxes.top = top;
            boxes.clear(true);
        }
    }
    let mut wrapped = wrap(&text, first, cont, height, mode, &opts, zones, &mut boxes)?;
    if !zones.is_empty() {
        for _ in 0..text_units {
            let mut changed = false;
            let ws = starts(&wrapped);
            for o in objs.iter().filter(|o| truth(&o["inline"])) {
                let Some(k) = real
                    .iter()
                    .position(|&i| o["index"].as_u64() == Some(i as u64))
                else {
                    continue;
                };
                let mut li = 0;
                for (j, &start) in ws.iter().enumerate().skip(1) {
                    if start <= k {
                        li = j
                    }
                }
                let el = id(&o["el"])?;
                let om = child(d, el, "outMargin");
                let h = num(d, om, "top", 0.0)
                    + effective_height(d, o, styles, ctx)?
                    + num(d, om, "bottom", 0.0);
                if boxes.collision.len() <= li {
                    boxes.collision.resize(li + 1, 0.0)
                }
                if h > max(height, boxes.collision[li]) {
                    boxes.collision[li] = h;
                    changed = true
                }
            }
            if !changed {
                break;
            }
            boxes.clear(true);
            wrapped = wrap(&text, first, cont, height, mode, &opts, zones, &mut boxes)?;
        }
    }
    let ws = starts(&wrapped);
    let mut obj_h = vec![0.0; ws.len()];
    let mut obj_asc = vec![0.0; ws.len()];
    let mut obj_ls = vec![false; ws.len()];
    let units = text.encode_utf16().collect::<Vec<_>>();
    for o in objs.iter().filter(|o| truth(&o["inline"])) {
        let el = id(&o["el"])?;
        let om = child(d, el, "outMargin");
        let li = line_for(&ws, &real, id(&o["index"])?);
        let h = num(d, om, "top", 0.0)
            + effective_height(d, o, styles, ctx)?
            + num(d, om, "bottom", 0.0);
        let prior_width = if s(o, "tag") == "tbl" && li > 0 {
            if !zones.is_empty() {
                boxes.width(li - 1, zones)
            } else if li == 1 {
                first
            } else {
                cont
            }
        } else {
            0.0
        };
        if s(o, "tag") == "tbl"
            && li > 0
            && n(o, "omL") + n(o, "width") + n(o, "omR") > prior_width
            && ws[li] > ws[li - 1]
            && units[ws[li - 1]..ws[li]]
                .iter()
                .all(|&u| u == 0x2060 || u == 0xfdd0)
        {
            if boxes.collision.len() < li {
                boxes.collision.resize(li, 0.0)
            }
            boxes.collision[li - 1] = max(boxes.collision[li - 1], h)
        }
        obj_h[li] = max(obj_h[li], h);
        if attr(d, child(d, el, "pos"), "affectLSpacing", "") == "1" {
            obj_ls[li] = true
        }
        let baseline = if s(o, "tag") == "equation" {
            d.nodes[el]
                .attr("baseLine")
                .map(|s| number(&s.into()))
                .unwrap_or(0.0)
        } else {
            f64::NAN
        };
        obj_asc[li] = max(
            obj_asc[li],
            round(
                h * if baseline.is_finite() {
                    baseline / 100.0
                } else {
                    0.85
                },
            ),
        );
    }
    let lsa = d.create_line_array_for(p)?;
    let parent = d.nodes[p].parent;
    let vertical_center = attr(d, parent, "vertAlign", "") == "CENTER"
        && ["VERTICAL", "VERTICALALL"].contains(&attr(d, parent, "textDirection", ""));
    let mut v = top;
    let mut first_acc = 0.0;
    let textpos = ws
        .iter()
        .enumerate()
        .map(|(li, &start)| {
            if li == 0 {
                0
            } else {
                real.get(start)
                    .copied()
                    .unwrap_or(if real.is_empty() { 0 } else { chars.len() })
            }
        })
        .collect::<Vec<_>>();
    let height_of = |key: &Value| {
        let v = if nullish(key) {
            None
        } else {
            map_get(&styles["charPr"], key)
        };
        v.filter(|v| truth(&v["height"]))
            .map_or(0.0, |v| n(v, "height"))
    };
    let nat_w = widths.iter().fold(0.0, |a, w| a + w) + spacings.iter().fold(0.0, |a, w| a + w);
    for li in 0..ws.len() {
        let start = textpos[li];
        let end = textpos.get(li + 1).copied().unwrap_or(chars.len());
        let last = li == ws.len() - 1;
        let mut line_text_h = 0.0;
        for c in &chars[start..end] {
            line_text_h = max(line_text_h, height_of(&c["prId"]))
        }
        for r in list(&m, "runs") {
            let at = n(r, "at");
            if at >= start as f64
                && if last {
                    at <= end as f64
                } else {
                    at < (end as f64)
                }
            {
                line_text_h = max(line_text_h, height_of(&r["prId"]))
            }
        }
        if li == 0 && !nullish(&bu["charPrId"]) {
            line_text_h = max(line_text_h, height_of(&bu["charPrId"]))
        }
        if line_text_h == 0.0 || line_text_h.is_nan() {
            line_text_h = height
        }
        let mut fl = None;
        if truth(&geom["fontLineHeight"])
            && chars[start..end]
                .iter()
                .any(|c| truth(&c["ch"]) && c["ch"] != " ")
        {
            let mut h = 0.0;
            let mut b = 0.0;
            let mut any = false;
            for c in &chars[start..end] {
                if !truth(&c["ch"]) || nullish(&c["prId"]) {
                    continue;
                }
                let Some(style) = map_get(&styles["charPr"], &c["prId"]) else {
                    continue;
                };
                let latin = cp(c).is_some_and(|cp| {
                    cp < 0x250 || (0x1e00..0x2000).contains(&cp) || (0x2018..0x2020).contains(&cp)
                });
                let font = if c["ch"] != " " && truth(&style["latin"]) && latin {
                    &style["latin"]
                } else {
                    style
                };
                let fm = font_metrics(&font["face"], truth(&font["hft"]));
                any = true;
                let ht = n(style, "height");
                h = max(
                    h,
                    fm.map(|(a, de, up)| {
                        round(ht * a)
                            + if up {
                                round(ht * de)
                            } else {
                                (ht * de).trunc()
                            }
                    })
                    .unwrap_or(ht),
                );
                b = max(
                    b,
                    fm.map(|(a, _, _)| round(ht * a))
                        .unwrap_or(round(ht * 0.85)),
                );
            }
            if any {
                fl = Some([h, b])
            }
        }
        let border = reflow_helpers::line_border_box(&m, styles, &json!(start), &json!(end))?;
        let box_top = number(&border[0]);
        let box_all = number(&border[1]);
        let empty_border = truth(&border[2]);
        let text_h0 = fl.map(|f| max(f[0], line_text_h)).unwrap_or(line_text_h);
        let text_h = max(text_h0, box_all);
        let obj_only = obj_h[li] > 0.0
            && obj_h[li] < text_h
            && !chars[start..end].iter().any(has_ink)
            && objs.iter().any(|o| {
                truth(&o["inline"])
                    && s(o, "tag") == "equation"
                    && line_for(&ws, &real, id(&o["index"]).unwrap_or(0)) == li
            });
        let end_h = if last
            && obj_h[li] > 0.0
            && obj_h[li] < text_h
            && !chars[start..].iter().any(has_ink)
            && objs.iter().any(|o| {
                truth(&o["inline"])
                    && s(o, "tag") == "tbl"
                    && line_for(&ws, &real, id(&o["index"]).unwrap_or(0)) == li
            }) {
            height_of(
                list(&m, "runs")
                    .last()
                    .map(|r| &r["prId"])
                    .unwrap_or(&Value::Null),
            )
        } else {
            -1.0
        };
        let line_h = if obj_only {
            obj_h[li]
        } else if end_h >= 0.0 {
            max(obj_h[li], end_h)
        } else {
            max(text_h, obj_h[li])
        };
        let sp_base = if obj_ls[li] || s(geom, "lineSpacingType") == "FIXED" {
            line_h
        } else {
            text_h
        };
        let spacing = pitch(sp_base, geom) - sp_base;
        if let Some([a, b]) = band
            && v < b
            && v + line_h > a
        {
            v = b
        }
        for &(index, h) in &line_floats {
            if line_for(&ws, &real, index) == li {
                if li > 0 {
                    v += h
                } else {
                    first_acc += h;
                    v = max(v, area_top + first_acc)
                }
            }
        }
        let vert = v;
        let next = textpos.get(li + 1).and_then(|&i| chars.get(i));
        let boundary = if objs.is_empty()
            && !truth(&geom["fontLineHeight"])
            && next.is_some_and(has_ink)
            && list(&m, "runs").iter().any(|r| {
                r["at"].as_u64() == textpos.get(li + 1).map(|&i| i as u64)
                    && r["prId"] == next.unwrap()["prId"]
            }) {
            height_of(&next.unwrap()["prId"])
        } else {
            0.0
        };
        let occupied = maxs(&[line_h, *boxes.collision.get(li).unwrap_or(&0.0), boundary]);
        v += line_h + spacing;
        let seg = d.create_line_segment_for(p)?;
        put(d, seg, "textpos", start as f64);
        put(d, seg, "vertpos", vert);
        put(
            d,
            seg,
            "vertsize",
            if empty_border {
                text_h0
            } else if occupied == text_h && box_all > text_h0 {
                max(text_h0, box_top)
            } else {
                occupied
            },
        );
        put(d, seg, "textheight", line_h);
        let baseline = if s(geom, "vertAlign") == "TOP" {
            0.0
        } else if s(geom, "vertAlign") == "CENTER"
            || vertical_center && (!truth(&geom["vertAlign"]) || s(geom, "vertAlign") == "BASELINE")
        {
            (line_h / 2.0).floor()
        } else if s(geom, "vertAlign") == "BOTTOM" {
            line_h
        } else if end_h >= 0.0 {
            max(round(end_h * 0.85), obj_asc[li])
        } else {
            maxs(&[
                fl.map(|f| f[1]).unwrap_or(round(text_h0 * 0.85)),
                if box_top > text_h0 {
                    round(box_top * 0.85)
                } else {
                    0.0
                },
                obj_asc[li],
            ])
        };
        put(d, seg, "baseline", baseline);
        put(d, seg, "spacing", spacing);
        let mut box_value = boxes.at(li, zones);
        let fit = if ws.len() == 1 && obj_h[li] == 0.0 {
            boxes.parts.get(&li).map(|p| {
                p.iter()
                    .copied()
                    .filter(|[a, b]| b - a >= 1440.0)
                    .collect::<Vec<_>>()
            })
        } else {
            None
        };
        let parts = fit.filter(|p| {
            p.len() > 1 && nat_w <= p[0][1] - p[0][0] - max(0.0, n(geom, "marginIntent"))
        });
        if let Some(parts) = &parts {
            box_value = Some([parts[0][0], parts[0][1] - parts[0][0]])
        }
        put(d, seg, "horzpos", box_value.map(|b| b[0]).unwrap_or(left));
        put(d, seg, "horzsize", box_value.map(|b| b[1]).unwrap_or(avail));
        d.nodes[seg].set_attr("flags", "393216".into());
        if mode == "charAll"
            && inline.is_empty()
            && !truth(&geom["marginIntent"])
            && bullet == 0.0
            && !truth(&condense)
        {
            let free = boxes
                .parts
                .get(&li)
                .map(|p| json!(p.iter().map(|v| json!([v[0], v[1]])).collect::<Vec<_>>()))
                .unwrap_or(Value::Undefined);
            let bv = box_value
                .map(|b| json!([b[0], b[1]]))
                .unwrap_or(Value::Undefined);
            let flow = json!({"widths":widths,"spacings":spacings,"slots":real,"start":ws[li],"end":ws.get(li+1).copied().unwrap_or(text_units)});
            if let Some(fragments) =
                reflow_helpers::narrow_line_fragments_utf16(&free, &bv, &flow, &units)?
            {
                for f in fragments {
                    let more = d.clone_shallow(seg)?;
                    if let Some(tp) = f.textpos {
                        d.nodes[more].set_attr("textpos", string(&tp))
                    }
                    d.nodes[more].set_attr("horzpos", string(&f.x));
                    d.nodes[more].set_attr("horzsize", string(&f.width));
                    d.append_child(lsa, more)?
                }
                continue;
            }
        }
        d.append_child(lsa, seg)?;
        if let Some(parts) = parts {
            for [a, b] in parts.into_iter().skip(1) {
                let more = d.clone_shallow(seg)?;
                put(d, more, "textpos", (chars.len() + 1) as f64);
                put(d, more, "horzpos", a);
                put(d, more, "horzsize", b - a);
                d.append_child(lsa, more)?
            }
        }
    }
    d.append_child(p, lsa)?;
    if cell_float_bottom > 0.0 {
        float_floor = max(float_floor, args.start_v + cell_float_bottom)
    }
    if let Some([top, bottom]) = band {
        zones.push(json!({"top":top,"bottom":bottom,"left":-1e9,"right":1e9,"flow":"BOTH_SIDES"}))
    }
    Ok(Some(
        json!({"paraBottom":max(v,obj_bottom),"floatFloor":float_floor,"textBottom":v,"spaceAfter":geom["spaceAfter"]}),
    ))
}

#[cfg(test)]
fn snapshot(d: &ParsedXml, i: usize) -> Value {
    json!({"name":d.nodes[i].name,"attrs":d.nodes[i].attributes().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"children":d.nodes[i].children.iter().map(|&c|snapshot(d,c)).collect::<Vec<_>>()})
}
/// Read-only synthetic bridge. Returned DOM includes only attached elements,
/// exactly as the original ownerDocument tree; detached allocation IDs are not observable.
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    if operation != "reflowPara" && operation != "sequence" {
        return Err("알 수 없는 문단 재배치 연산입니다.".into());
    }
    let input = Value::from_json(input);
    let mut d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let mut ctx = Context::default();
    if truth(&input["prepare"]) {
        pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx)
    }
    let styles = if let Some(xml) = input["header"].as_str() {
        head_styles::parse_render_styles(xml, input.get("styleOpts"))?
    } else {
        input["styles"].clone()
    };
    let mut nums = CounterState::from_value(&input["nums"])?;
    let mut zones = list(&input, "zones").to_vec();
    let mut state = ReflowState::default();
    let mut output = Vec::new();
    let steps = if operation == "sequence" {
        list(&input, "steps").to_vec()
    } else {
        vec![input.clone()]
    };
    for step in steps {
        let mut p = d.root;
        for index in list(&step, "path") {
            p = *d.nodes[p]
                .children
                .get(id(index)?)
                .ok_or("문단 경로가 올바르지 않습니다.")?;
        }
        if truth(&step["removeLines"]) {
            let remove = d.nodes[p]
                .children
                .iter()
                .copied()
                .filter(|&i| d.nodes[i].local == "linesegarray")
                .collect::<Vec<_>>();
            for i in remove {
                d.remove_child(p, i)?
            }
        }
        let args = ReflowArgs {
            area_w: n(&step, "areaW"),
            start_v: n(&step, "startV"),
            mode: step["mode"].as_str().unwrap_or("keep"),
            body_flow: step.get("bodyFlow").is_none_or(truth),
            page_h: step.get("pageH").map_or(0.0, number),
        };
        let result = reflow_para(
            &mut d, p, &styles, &args, &mut zones, &mut nums, &mut ctx, &mut state,
        );
        let value = match result {
            Ok(v) => json!({"ok":true,"result":v.unwrap_or(Value::Null)}),
            Err(_) => json!({"ok":false}),
        };
        output.push(
            json!({"value":value,"zones":zones,"nums":nums.to_value(),"dom":snapshot(&d,d.root)}),
        );
    }
    Ok(json!(output).to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_limits_accept_boundary_and_reject_each_overflow_without_large_input() {
        assert!(check_size_limits(3, 2, 3, 2).is_ok());
        assert!(check_size_limits(0, 0, 0, 0).is_ok());
        assert!(check_size_limits(4, 2, 3, 2).is_err());
        assert!(check_size_limits(3, 3, 3, 2).is_err());
        assert!(check_size_limits(usize::MAX, 0, 3, 2).is_err());
    }
    #[test]
    fn saved_lines_still_advance_the_number_counter() {
        let input = serde_json::json!({"xml":"<p paraPrIDRef='p'><linesegarray><lineseg/></linesegarray></p>","styles":{"paraGeom":[["p",{"number":{"id":"n","level":0,"heads":[[0,{"text":"^1","format":"DIGIT"}]]}}]],"charPr":[]},"areaW":10000,"startV":0});
        let out = run("reflowPara", &input).unwrap();
        assert_eq!(out[0]["value"]["ok"], true);
        assert!(out[0]["value"]["result"].is_null());
        let counters = out[0]["nums"].as_array().unwrap();
        assert_eq!(counters.len(), 1);
        assert_eq!(counters[0][0], "n");
        assert_eq!(counters[0][1]["length"].as_f64(), Some(1.0));
        let entries = counters[0][1]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0][0], "0");
        // JavaScript counters contain Numbers; serde's integer/f64 JSON
        // storage distinction must not change their numerical contract.
        assert_eq!(entries[0][1].as_f64(), Some(1.0));
        assert_eq!(out[0]["dom"]["children"].as_array().unwrap().len(), 1);
        assert_eq!(out[0]["dom"]["children"][0]["name"], "linesegarray");
    }
    #[test]
    fn line_attributes_have_original_order() {
        let input = serde_json::json!({"xml":"<p><run><t>A</t></run></p>","styles":{"paraGeom":[],"charPr":[],"borderFill":[]},"areaW":10000,"startV":0});
        let out = run("reflowPara", &input).unwrap();
        assert_eq!(out[0]["value"]["ok"], true);
        let attrs = out[0]["dom"]["children"][1]["children"][0]["attrs"]
            .as_array()
            .unwrap();
        assert_eq!(
            attrs
                .iter()
                .map(|v| v[0].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "textpos",
                "vertpos",
                "vertsize",
                "textheight",
                "baseline",
                "spacing",
                "horzpos",
                "horzsize",
                "flags"
            ]
        );
    }
}
