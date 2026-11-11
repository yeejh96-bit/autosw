//! kordoc svg-content.mjs drawPara/cellParaTop, MIT, Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE. Output remains UTF-16 until the complete SVG boundary.
use super::{
    glyph, head_styles,
    layout::{maximum, minimum},
    layout_text::{LayoutText, TextRef, control_text},
    line_plan, metrics,
    numbering::add,
    object_anchor::{self, AnchorArgs, AnchorContext},
    para_model::{self as pm, child, num},
    para_number::{self, CounterState},
    pua,
    scene::RegionCollector,
    shape_geometry,
    style_dom::ParsedXml,
    value::{Value, native_json as json, number, string},
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

pub type BodyDx = dyn FnMut(&Value) -> Result<Value, String>;
pub struct RenderContext {
    pub styles: Value,
    pub nums: CounterState,
    pub extent: pm::Context,
    pub pages: Vec<Vec<LayoutText>>,
    pub page: Value,
    pub page_base: Value,
    pub page_h: f64,
    pub regions: RegionCollector,
    pub parent_stack: Vec<String>,
    pub region_rotations: Vec<Value>,
    pub warnings: Vec<String>,
    pub warned: HashSet<String>,
    pub warned_order: Vec<String>,
    pub stats: Value,
    pub highlights: Vec<LayoutText>,
    pub geom: Value,
    pub body_dx: Option<Box<BodyDx>>,
    pub para_tops: HashMap<usize, Value>,
    pub obj_place: HashMap<usize, Value>,
    pub defs: Vec<Value>,
    pub images: super::value::Map<String, Value>,
    pub paint_jobs: Option<Vec<super::svg_objects::PaintJob>>,
    pub split_cell: Value,
    pub split_cont_mt: Value,
    pub cols: Value,
    pub esc_origin: Value,
}
impl RenderContext {
    pub fn new(styles: Value, page_count: usize) -> Self {
        Self {
            styles,
            nums: CounterState::default(),
            extent: pm::Context::default(),
            pages: vec![Vec::new(); page_count],
            page: 0.into(),
            page_base: 0.into(),
            page_h: 100000.0,
            regions: RegionCollector::default(),
            parent_stack: Vec::new(),
            region_rotations: Vec::new(),
            warnings: Vec::new(),
            warned: HashSet::new(),
            warned_order: Vec::new(),
            stats: json!({"texts":0,"images":0,"tables":0,"shapes":0}),
            highlights: Vec::new(),
            geom: json!({}),
            body_dx: None,
            para_tops: HashMap::new(),
            obj_place: HashMap::new(),
            defs: Vec::new(),
            images: Default::default(),
            paint_jobs: None,
            split_cell: Value::Undefined,
            split_cont_mt: Value::Undefined,
            cols: Value::Undefined,
            esc_origin: Value::Undefined,
        }
    }
    pub fn emit(&mut self, text: LayoutText) -> Result<(), String> {
        let page = self
            .page
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("출력 쪽 번호 오류")?;
        self.emit_on(page, text)
    }
    pub fn emit_on(&mut self, page: usize, text: LayoutText) -> Result<(), String> {
        self.pages
            .get_mut(page)
            .ok_or("출력 쪽 범위 오류")?
            .push(text);
        Ok(())
    }
    pub fn warn_once(&mut self, key: &str, message: &str) {
        if self.warned.insert(key.into()) {
            self.warned_order.push(key.into());
            self.warnings.push(message.into());
        }
    }
    pub fn page_no(&self) -> f64 {
        number(&self.page_base) + number(&self.page) + 1.0
    }
    pub fn bbox(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<Value, String> {
        self.bbox_value(&x.into(), &y.into(), &w.into(), &h.into())
    }
    pub fn bbox_value(
        &mut self,
        x: &Value,
        y: &Value,
        w: &Value,
        h: &Value,
    ) -> Result<Value, String> {
        let mut b = shape_geometry::rotated_bounds(
            &json!({"x":x,"y":y,"w":w,"h":h}),
            &self.region_rotations,
        );
        if let Some(dx) = self.body_dx.as_mut() {
            b["x"] = super::numbering::add(&b["x"], &dx(&self.page)?);
        }
        let h = maximum(0.0, minimum(number(&b["h"]), self.page_h - number(&b["y"])));
        Ok(
            json!({"page":self.page_no(),"x":round(number(&b["x"]))/100.0,"y":round(number(&b["y"]))/100.0,"width":round(maximum(0.0,number(&b["w"])))/100.0,"height":round(h)/100.0}),
        )
    }
    fn anchor(
        &mut self,
        d: &ParsedXml,
        o: &Value,
        args: &AnchorArgs,
    ) -> Result<(Value, Value), String> {
        let result = if let Some(body) = self.body_dx.as_mut() {
            let mut callback = |page: &Value| body(page);
            object_anchor::anchor_object(
                d,
                o,
                args,
                &mut AnchorContext {
                    geom: &self.geom,
                    page: &self.page,
                    extent: &mut self.extent,
                    body_dx: Some(&mut callback),
                },
            )?
        } else {
            object_anchor::anchor_object(
                d,
                o,
                args,
                &mut AnchorContext {
                    geom: &self.geom,
                    page: &self.page,
                    extent: &mut self.extent,
                    body_dx: None,
                },
            )?
        };
        Ok((result["x"].clone(), result["y"].clone()))
    }
}
#[derive(Clone, Debug)]
pub struct ObjectArgs {
    pub object: Value,
    pub x: Value,
    pub y: Value,
    pub base_v: f64,
    pub area_w: f64,
    pub depth: usize,
}
pub trait DrawObjects {
    fn draw_object(
        &mut self,
        doc: &mut ParsedXml,
        args: ObjectArgs,
        ctx: &mut RenderContext,
    ) -> Result<(), String>;
}
fn truth(v: &Value) -> bool {
    super::numbering::truth(v)
}
fn nullish(v: &Value) -> bool {
    matches!(v, Value::Null | Value::Undefined)
}
fn greater(a: &Value, b: &Value) -> bool {
    // cellParaTop and its following + produce numbers or strings. JavaScript
    // compares two strings lexically, but converts a mixed pair to numbers.
    if let (Value::String(a), Value::String(b)) = (a, b) {
        a.encode_utf16().cmp(b.encode_utf16()).is_gt()
    } else {
        number(a) > number(b)
    }
}
fn get<'a>(styles: &'a Value, key: &str, id: &Value) -> &'a Value {
    if nullish(id) {
        return &Value::Undefined;
    }
    styles[key]
        .as_array()
        .and_then(|a| a.iter().rev().find(|p| p[0] == *id))
        .map_or(&Value::Undefined, |p| &p[1])
}
fn st(styles: &Value, id: &Value) -> Value {
    let s = get(styles, "charPr", id);
    if nullish(s) {
        head_styles::default_char()
    } else {
        s.clone()
    }
}
pub(crate) fn round(n: f64) -> f64 {
    if !n.is_finite() || n == 0.0 || n.abs() >= 4503599627370496.0 {
        n
    } else if (-0.5..0.0).contains(&n) {
        -0.0
    } else {
        let f = n.floor();
        if n - f < 0.5 { f } else { f + 1.0 }
    }
}
pub(crate) fn pt(n: f64) -> String {
    string(&(round(n) / 100.0).into())
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn ch(c: &Value) -> &str {
    text(&c["ch"])
}
fn ink(s: &str) -> bool {
    !s.trim_matches(crate::input::is_space).is_empty()
}
fn index(n: f64, len: usize) -> Result<usize, String> {
    if n >= 0.0 && n.fract() == 0.0 && n < (len as f64) {
        Ok(n as usize)
    } else {
        Err("문자 위치 오류".into())
    }
}
fn node(o: &Value) -> Result<usize, String> {
    o["el"]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| "개체 노드 없음".into())
}
pub(crate) fn region_open(id: &str, kind: &str, page: f64) -> LayoutText {
    format!(
        "<g data-kordoc-id=\"{id}\" data-kordoc-type=\"{kind}\" data-kordoc-page=\"{}\">",
        string(&page.into())
    )
    .into()
}
fn text_element(attrs: Vec<(String, LayoutText)>, body: LayoutText) -> LayoutText {
    let mut out = LayoutText::from("<text ");
    for (i, (key, value)) in attrs.into_iter().enumerate() {
        if i > 0 {
            out.append(&TextRef::Utf8(" "));
        }
        out.append(&TextRef::Utf8(&format!("{key}=\"")));
        out.append(&value.as_text_ref());
        out.append(&TextRef::Utf8("\""));
    }
    out.append(&TextRef::Utf8(">"));
    out.append(&body.as_text_ref());
    out.append(&TextRef::Utf8("</text>"));
    out
}
fn attr(k: &str, v: String) -> (String, LayoutText) {
    (k.into(), v.into())
}
fn escaped(v: &Value) -> LayoutText {
    LayoutText::from(text(v)).escape_xml()
}
fn inc_text(ctx: &mut RenderContext) {
    ctx.stats["texts"] = (number(&ctx.stats["texts"]) + 1.0).into();
}
fn glyph_y(y: &Value, vertical: &str, h: f64) -> Value {
    add(
        y,
        &Value::Number(if vertical == "CENTER" {
            round(h * 0.35)
        } else if vertical == "BOTTOM" {
            -round(h * 0.15)
        } else {
            0.0
        }),
    )
}
fn offsets(origin: &Value, amounts: &[f64]) -> Value {
    amounts
        .iter()
        .fold(origin.clone(), |sum, n| add(&sum, &(*n).into()))
}
fn units_trimmed_empty(units: &[u16]) -> bool {
    TextRef::Utf16(Cow::Borrowed(units))
        .code_points()
        .all(|cp| char::from_u32(cp).is_some_and(crate::input::is_space))
}
fn lower_units(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut valid = String::new();
    for cp in TextRef::Utf16(Cow::Borrowed(units)).code_points() {
        if let Some(c) = char::from_u32(cp) {
            valid.push(c);
        } else {
            out.extend(valid.to_lowercase().encode_utf16());
            valid.clear();
            out.push(cp as u16);
        }
    }
    out.extend(valid.to_lowercase().encode_utf16());
    out
}
fn matched_ranges(piece: &[u16], terms: &[LayoutText]) -> Result<Vec<(usize, usize)>, String> {
    let lower = lower_units(piece);
    let mut found = Vec::new();
    for term in terms {
        if term.0.is_empty() {
            return Err("빈 강조어는 정규화 후 제거해야 합니다.".into());
        }
        let mut from = 0;
        while from <= lower.len() {
            let Some(at) = lower[from..]
                .windows(term.0.len())
                .position(|w| w == term.0)
            else {
                break;
            };
            let start = from + at;
            found.push((start, start + term.0.len()));
            from = start + term.0.len();
        }
    }
    found.sort_by_key(|p| p.0);
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (s, e) in found {
        if let Some(tail) = out.last_mut().filter(|t| s <= t.1) {
            tail.1 = tail.1.max(e);
        } else {
            out.push((s, e));
        }
    }
    Ok(out)
}
struct SegmentDraw<'a> {
    style: &'a Value,
    scale: f64,
    space_add: f64,
    char_add: Option<f64>,
    fixed_to: f64,
    i: f64,
    skip_lead: f64,
    piece: &'a [u16],
    y: &'a Value,
    vertical: &'a str,
    oy: &'a Value,
    seg: &'a Value,
}
impl SegmentDraw<'_> {
    fn render(
        &self,
        units: &[u16],
        cx: &Value,
        hit: bool,
        ctx: &mut RenderContext,
    ) -> Result<f64, String> {
        let mut w = 0.0;
        let mut last = 0;
        let mut spaces = 0;
        let mut nonspaces = 0;
        let mut count = 0;
        for (k, cp) in TextRef::Utf16(Cow::Borrowed(units))
            .code_points()
            .enumerate()
        {
            if !line_plan::joins_jamo(last, cp) {
                w += glyph::glyph_width_cp(Some(cp), self.style, true)?.0;
            }
            last = cp;
            count += 1;
            if cp == 32 {
                if !((k as f64) < self.skip_lead && units == self.piece) {
                    spaces += 1;
                }
            } else {
                nonspaces += 1;
            }
        }
        let sw = w * self.scale
            + if self.space_add != 0.0 && !self.space_add.is_nan() && self.i >= self.fixed_to {
                self.space_add * (spaces as f64)
            } else {
                0.0
            }
            + if self.char_add.is_some_and(|n| n != 0.0 && !n.is_nan()) && self.i >= self.fixed_to {
                self.char_add.unwrap() * (nonspaces as f64)
            } else {
                0.0
            };
        let h = number(&self.style["height"]);
        let gy = number(&glyph_y(self.y, self.vertical, h));
        if truth(&self.style["shade"]) {
            let mut s = LayoutText::from(format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"",
                pt(number(cx)),
                pt(gy - round(h * 0.85)),
                pt(sw),
                pt(h)
            ));
            s.append(&escaped(&self.style["shade"]).as_text_ref());
            s.append(&TextRef::Utf8("\"/>"));
            ctx.emit(s)?;
        }
        if hit {
            ctx.emit(format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffd54f\" fill-opacity=\"0.45\"/>",pt(number(cx)),pt(number(&add(self.oy,&self.seg["vertpos"]))),pt(sw),pt(number(&self.seg["textheight"]))).into())?;
        }
        if !units_trimmed_empty(units) {
            let fh = if truth(&self.style["script"]) {
                round(h * 16.0 / 25.0)
            } else {
                h
            };
            let raise = if self.style["script"] == "sup" {
                round(h * 0.44)
            } else {
                0.0
            };
            let mut attrs = vec![
                attr("x", pt(number(cx))),
                attr("y", pt(gy - raise)),
                attr("font-size", pt(fh)),
            ];
            if truth(&self.style["fontFamily"]) {
                attrs.push(("font-family".into(), escaped(&self.style["fontFamily"])));
            }
            if count > 1 && sw > 50.0 {
                attrs.push(attr("textLength", pt(sw)));
                attrs.push(attr(
                    "lengthAdjust",
                    if self.scale < 1.0 {
                        "spacingAndGlyphs"
                    } else {
                        "spacing"
                    }
                    .into(),
                ));
            }
            if truth(&self.style["bold"]) {
                attrs.push(attr("font-weight", "bold".into()));
            }
            if truth(&self.style["italic"]) {
                attrs.push(attr("font-style", "italic".into()));
            }
            if truth(&self.style["underline"]) {
                attrs.push(attr("text-decoration", "underline".into()));
            }
            if truth(&self.style["color"]) {
                attrs.push(("fill".into(), escaped(&self.style["color"])));
            }
            ctx.emit(text_element(
                attrs,
                pua::map_pua_text_utf16(units)?.escape_xml(),
            ))?;
            inc_text(ctx);
        }
        Ok(sw)
    }
}
pub fn cell_para_top(d: &ParsedXml, p: usize, styles: &Value) -> Value {
    let Some(parent) = d.nodes[p].parent else {
        return 0.into();
    };
    let siblings = &d.nodes[parent].children;
    let Some(at) = siblings.iter().position(|&n| n == p) else {
        return 0.into();
    };
    let Some(&previous) = siblings[..at]
        .iter()
        .rev()
        .find(|&&n| d.nodes[n].local == "p")
    else {
        return 0.into();
    };
    let seg = child(d, previous, "linesegarray").and_then(|a| {
        d.nodes[a]
            .children
            .iter()
            .rev()
            .copied()
            .find(|&n| d.nodes[n].local == "lineseg")
    });
    let Some(seg) = seg else {
        return Value::Undefined;
    };
    let id = Value::from(d.nodes[previous].attr("paraPrIDRef").unwrap_or(""));
    let geom = get(styles, "paraGeom", &id);
    let after = if nullish(&geom["spaceAfter"]) {
        Value::Number(0.0)
    } else {
        geom["spaceAfter"].clone()
    };
    super::numbering::add(
        &(num(d, Some(seg), "vertpos", 0.0)
            + num(d, Some(seg), "vertsize", 0.0)
            + num(d, Some(seg), "spacing", 0.0))
        .into(),
        &after,
    )
}
fn object_height(d: &ParsedXml, o: &Value, ctx: &mut RenderContext) -> Result<f64, String> {
    Ok(if o["tag"] == "tbl" {
        maximum(
            number(&o["height"]),
            pm::measure_table_height(d, node(o)?, &mut ctx.extent)?,
        )
    } else {
        number(&o["height"])
    })
}
#[allow(clippy::too_many_arguments)]
fn call_object<R: DrawObjects>(
    r: &mut R,
    d: &mut ParsedXml,
    o: &Value,
    x: impl Into<Value>,
    y: impl Into<Value>,
    base_v: f64,
    area_w: f64,
    depth: usize,
    ctx: &mut RenderContext,
) -> Result<(), String> {
    r.draw_object(
        d,
        ObjectArgs {
            object: o.clone(),
            x: x.into(),
            y: y.into(),
            base_v,
            area_w,
            depth,
        },
        ctx,
    )
}
fn page_for(ctx: &mut RenderContext, pages: Option<&[Value]>, index: usize) {
    if let Some(page) = pages
        .and_then(|p| p.get(index))
        .filter(|v| !v.is_undefined())
    {
        ctx.page = page.clone();
    }
}
fn args(ox: f64, oy: f64, base: f64, area: f64) -> AnchorArgs {
    AnchorArgs {
        ox: ox.into(),
        oy: oy.into(),
        base_v: base.into(),
        area_w: area.into(),
        ..Default::default()
    }
}
#[allow(clippy::too_many_arguments)]
pub fn draw_para<R: DrawObjects>(
    d: &mut ParsedXml,
    p: usize,
    ox: f64,
    oy: f64,
    area_w: f64,
    ctx: &mut RenderContext,
    depth: usize,
    seg_pages: Option<&[Value]>,
    renderer: &mut R,
) -> Result<(), String> {
    draw_para_native(
        d,
        p,
        ox.into(),
        oy.into(),
        area_w,
        ctx,
        depth,
        seg_pages,
        renderer,
    )
}
/// Native coordinate entry point used by captions and nested table content.
/// Preserve JavaScript addition until the original arithmetic/serialization
/// boundary, including a coordinate produced by dynamic paragraph spacing.
#[allow(clippy::too_many_arguments)]
pub fn draw_para_native<R: DrawObjects>(
    d: &mut ParsedXml,
    p: usize,
    ox: Value,
    oy: Value,
    area_w: f64,
    ctx: &mut RenderContext,
    depth: usize,
    seg_pages: Option<&[Value]>,
    renderer: &mut R,
) -> Result<(), String> {
    if depth > 16 {
        ctx.warn_once("depth", "중첩 깊이 16 초과 — 이하 생략");
        return Ok(());
    }
    let m = pm::build_para(d, p, &mut ctx.extent)?;
    let chars = m["chars"].as_array().ok_or("문단 문자 배열 없음")?;
    let objs = m["objs"].as_array().ok_or("문단 개체 배열 없음")?;
    let segs = m["segs"].as_array().ok_or("문단 줄 배열 없음")?;
    let number_text = para_number::next_para_number(
        &mut ctx.nums,
        Some(get(&ctx.styles, "paraGeom", &m["paraPrId"])),
    )?;
    if segs.is_empty() {
        if chars.iter().any(|c| !ch(c).is_empty()) {
            ctx.warn_once(
                "no-lineseg",
                "조판 캐시 없는 문단 텍스트 생략 — reflow 옵션으로 합성 가능",
            );
        }
        for o in objs {
            let (x, y) = if truth(&o["inline"]) {
                (ox.clone(), oy.clone())
            } else {
                let mut a = args(0.0, 0.0, 0.0, area_w);
                a.ox = ox.clone();
                a.oy = oy.clone();
                ctx.anchor(d, o, &a)?
            };
            call_object(renderer, d, o, x, y, 0.0, area_w, depth, ctx)?;
        }
        return Ok(());
    }
    let plans = line_plan::plan_lines(&m, &ctx.styles, &number_text)?;
    let base_v = number(&segs[0]["vertpos"]);
    let has_text = chars.iter().any(|c| ink(ch(c)));
    let mut region_id = None::<String>;
    let mut open_on = None::<usize>;
    for (li, plan) in plans.iter().enumerate() {
        page_for(ctx, seg_pages, li);
        let seg = &plan.seg;
        if has_text {
            let b = ctx.bbox_value(
                &add(&ox, &seg["horzpos"]),
                &add(&oy, &seg["vertpos"]),
                &seg["horzsize"],
                &seg["textheight"],
            )?;
            if let Some(id) = &region_id {
                ctx.regions.add_fragment(id, b);
            } else {
                let parent = ctx
                    .parent_stack
                    .last()
                    .map_or(Value::Undefined, |s| s.clone().into());
                region_id = Some(ctx.regions.add("paragraph", b, &json!({"parentId":parent})));
            }
            let current = ctx.page.as_u64().ok_or("출력 쪽 번호 오류")? as usize;
            if open_on != Some(current) {
                if let Some(page) = open_on {
                    ctx.emit_on(page, LayoutText::from("</g>"))?;
                }
                ctx.emit(region_open(
                    region_id.as_deref().unwrap(),
                    "paragraph",
                    ctx.page_no(),
                ))?;
                open_on = Some(current);
            }
        }
        let mut i = plan.start;
        let mut cursor = offsets(&ox, &[number(&seg["horzpos"]), plan.xoff]);
        let y = offsets(&oy, &[number(&seg["vertpos"]), number(&seg["baseline"])]);
        let pva = text(&get(&ctx.styles, "paraGeom", &m["paraPrId"])["vertAlign"]).to_owned();
        if let Some(bullet) = &plan.bullet {
            let bs = st(&ctx.styles, &bullet["prId"]);
            let mut attrs = vec![
                attr("x", pt(number(&cursor) - number(&bullet["w"]))),
                attr("y", pt(number(&glyph_y(&y, &pva, number(&bs["height"]))))),
                attr("font-size", pt(number(&bs["height"]))),
            ];
            if truth(&bs["fontFamily"]) {
                attrs.push(("font-family".into(), escaped(&bs["fontFamily"])));
            }
            if truth(&bs["color"]) {
                attrs.push(("fill".into(), escaped(&bs["color"])));
            }
            ctx.emit(text_element(attrs, escaped(&bullet["ch"])))?;
            inc_text(ctx);
        }
        while i < plan.end && i < (chars.len() as f64) {
            let at = index(i, chars.len())?;
            let c = &chars[at];
            if i >= plan.text_end && ch(c) == " " {
                i += 1.0;
                continue;
            }
            if ch(c).is_empty() {
                if let Some(control) = control_text(c)?.filter(|t| !t.is_empty()) {
                    let s = st(&ctx.styles, &c["prId"]);
                    let sup = truth(&c["ctrlSup"]);
                    let h = number(&s["height"]);
                    let fh = if sup { round(h * 16.0 / 25.0) } else { h };
                    let cw = metrics::measure_text_width_ref(
                        control.clone(),
                        if sup { round(h * 3.0 / 4.0) } else { fh },
                        number(&s["ratio"]),
                        Some(
                            &json!({"spacingPct":s["spacing"],"faceClass":metrics::face_class_of(s["face"].as_str())}),
                        ),
                    );
                    let mut attrs = vec![
                        attr("x", pt(number(&cursor))),
                        attr(
                            "y",
                            pt(number(&glyph_y(&y, &pva, h))
                                - if sup { round(h * 0.35) } else { 0.0 }),
                        ),
                        attr("font-size", pt(fh)),
                    ];
                    if truth(&s["fontFamily"]) {
                        attrs.push(("font-family".into(), escaped(&s["fontFamily"])));
                    }
                    if truth(&s["color"]) {
                        attrs.push(("fill".into(), escaped(&s["color"])));
                    }
                    ctx.emit(text_element(attrs, control.to_layout_text().escape_xml()))?;
                    inc_text(ctx);
                    cursor = add(&cursor, &(cw * plan.scale).into());
                }
                if truth(&c["tab"]) {
                    let tab = plan
                        .tabs
                        .iter()
                        .find(|(k, _)| *k == i)
                        .map_or(0.0, |(_, v)| *v);
                    cursor = add(&cursor, &tab.into());
                }
                for o in objs {
                    if truth(&o["inline"]) && o["index"] == Value::Number(i) {
                        cursor = add(
                            &cursor,
                            &(number(&o["omL"]) + number(&o["width"]) + number(&o["omR"])).into(),
                        );
                    }
                }
                i += 1.0;
                continue;
            }
            let scale = if i < plan.fixed_to { 1.0 } else { plan.scale };
            let pr = &c["prId"];
            let mut j = i;
            let mut piece = Vec::new();
            while j < plan.end && j < (chars.len() as f64) {
                let c = &chars[index(j, chars.len())?];
                if c["prId"] != *pr || ch(c).is_empty() || j >= plan.text_end && ch(c) == " " {
                    break;
                }
                piece.extend(ch(c).encode_utf16());
                j += 1.0;
            }
            if let Some(cut) = piece.windows(2).position(|p| p == [32, 32]) {
                if cut > 0 {
                    piece.truncate(cut);
                    j = i + cut as f64;
                } else {
                    let n = piece.iter().take_while(|&&u| u == 32).count();
                    piece.truncate(n);
                    j = i + n as f64;
                }
            }
            let style = st(&ctx.styles, pr);
            let render = SegmentDraw {
                style: &style,
                scale,
                space_add: plan.space_add,
                char_add: plan.char_add,
                fixed_to: plan.fixed_to,
                i,
                skip_lead: maximum(0.0, plan.sp_from - i),
                piece: &piece,
                y: &y,
                vertical: &pva,
                oy: &oy,
                seg,
            };
            let ranges = if !ctx.highlights.is_empty() && !units_trimmed_empty(&piece) {
                matched_ranges(&piece, &ctx.highlights)?
            } else {
                Vec::new()
            };
            if ranges.is_empty() {
                cursor = add(&cursor, &render.render(&piece, &cursor, false, ctx)?.into());
            } else {
                let mut last = 0;
                for (s, e) in ranges {
                    let s = s.min(piece.len());
                    let e = e.min(piece.len());
                    cursor = add(
                        &cursor,
                        &render
                            .render(&piece[last.min(piece.len())..s], &cursor, false, ctx)?
                            .into(),
                    );
                    cursor = add(
                        &cursor,
                        &render.render(&piece[s..e], &cursor, true, ctx)?.into(),
                    );
                    last = e;
                }
                cursor = add(
                    &cursor,
                    &render.render(&piece[last..], &cursor, false, ctx)?.into(),
                );
            }
            i = j;
        }
    }
    if let Some(page) = open_on {
        ctx.emit_on(page, LayoutText::from("</g>"))?;
    }
    if let Some(id) = &region_id {
        ctx.parent_stack.push(id.clone());
    }
    let mut stacked = None::<HashMap<usize, f64>>;
    let mut stacked_top = None::<f64>;
    if depth == 0 {
        ctx.esc_origin = ctx.para_tops.get(&p).map_or(
            Value::Undefined,
            |top| json!({"x":ox,"y":add(&oy,top),"w":area_w}),
        );
    }
    for o in objs {
        let el = node(o)?;
        if truth(&o["inline"]) {
            let mut pi = 0;
            for (k, pl) in plans.iter().enumerate() {
                if pl.start <= number(&o["index"])
                    && (number(&o["index"]) < pl.end || k == plans.len() - 1)
                {
                    pi = k;
                }
            }
            let plan = &plans[pi];
            page_for(ctx, seg_pages, pi);
            let seg = &plan.seg;
            let x = offsets(
                &ox,
                &[
                    number(&seg["horzpos"]),
                    plan.xoff,
                    line_plan::advance_to(&m, &ctx.styles, plan, number(&o["index"]))?,
                    number(&o["omL"]),
                ],
            );
            let shadowed = o["tag"] == "pic"
                && child(d, child(d, el, "effects").unwrap_or(el), "shadow").is_some()
                && number(&seg["textheight"]) > number(&o["height"])
                && (number(&seg["baseline"]) - round(number(&seg["textheight"]) * 0.85)).abs()
                    <= 2.0;
            let baseline = d.nodes[el]
                .attr("baseLine")
                .map_or(0.0, |v| number(&v.into()));
            let asc = if o["tag"] == "equation" && baseline > 0.0 {
                round(number(&o["height"]) * baseline / 100.0)
            } else if shadowed {
                round(number(&seg["textheight"]) * 0.85)
            } else if o["tag"] == "pic" {
                round(number(&o["height"]) * 0.85)
            } else {
                object_height(d, o, ctx)?
            };
            let om = child(d, el, "outMargin");
            let top = num(d, om, "top", 0.0);
            let bottom = num(d, om, "bottom", 0.0);
            let caption = pm::caption_extra(d, el);
            let y = offsets(
                &oy,
                &[
                    number(&seg["vertpos"]),
                    maximum(
                        top,
                        if o["tag"] == "tbl" && caption == 0.0 {
                            number(&seg["baseline"]) - round((asc + top + bottom) * 0.85) + top
                        } else {
                            number(&seg["baseline"]) - asc - bottom - caption
                        },
                    ),
                ],
            );
            call_object(renderer, d, o, x, y, base_v, area_w, depth, ctx)?;
            continue;
        }
        page_for(ctx, seg_pages, 0);
        let pos = child(d, el, "pos");
        if depth > 0
            && truth(&ctx.esc_origin)
            && pm::attr(d, pos, "flowWithText") == Some("0")
            && pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") == "PARA"
        {
            let mut a = args(0.0, 0.0, 0.0, number(&ctx.esc_origin["w"]));
            a.ox = ctx.esc_origin["x"].clone();
            a.oy = ctx.esc_origin["y"].clone();
            a.stay_line = 0.into();
            let (x, y) = ctx.anchor(d, o, &a)?;
            call_object(renderer, d, o, x, y, base_v, area_w, depth, ctx)?;
            continue;
        }
        let (mut ax, mut aw) = (ox.clone(), area_w);
        if depth == 0 && truth(&ctx.cols) {
            let col = ((number(&plans[0].seg["horzpos"]) + 1.0)
                / (number(&ctx.cols["w"]) + number(&ctx.cols["gap"])))
            .floor();
            ax = add(
                &ox,
                &(col * (number(&ctx.cols["w"]) + number(&ctx.cols["gap"]))).into(),
            );
            aw = number(&ctx.cols["w"]);
        }
        if pm::attr(d, pos, "horzRelTo") == Some("PARA") && !nullish(&m["paraPrId"]) {
            let g = get(&ctx.styles, "paraGeom", &m["paraPrId"]);
            if truth(g) {
                ax = add(&ax, &g["marginLeft"]);
                aw -= number(&g["marginLeft"]) + number(&g["marginRight"]);
            }
        }
        let mut line_v = Value::Undefined;
        let mut line_h = 0.0;
        let mut anchor_k = None;
        for (k, pl) in plans.iter().enumerate() {
            if pl.start <= number(&o["index"]) {
                line_v = pl.seg["vertpos"].clone();
                line_h = number(&pl.seg["textheight"]);
                anchor_k = Some(k);
            }
        }
        let mut obj_base = Value::Number(base_v);
        let p_top = if depth == 0 {
            ctx.para_tops.get(&p).cloned().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let wrap = d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM");
        let top0 = if !nullish(&p_top) {
            p_top.clone()
        } else if depth > 0 && wrap != "TOP_AND_BOTTOM" {
            cell_para_top(d, p, &ctx.styles)
        } else {
            Value::Undefined
        };
        let g = get(&ctx.styles, "paraGeom", &m["paraPrId"]);
        let before = if nullish(&g["spaceBefore"]) {
            Value::Number(0.0)
        } else {
            g["spaceBefore"].clone()
        };
        if !top0.is_undefined() && wrap != "TOP_AND_BOTTOM" {
            let start = super::numbering::add(&top0, &before);
            if base_v > number(&super::numbering::add(&start, &100.0.into())) {
                obj_base = start.clone();
                if line_v == Value::Number(base_v) {
                    line_v = start.clone();
                }
            } else if depth > 0
                && greater(&start, &top0)
                && (base_v - number(&start)).abs() <= 100.0
            {
                obj_base = top0.clone();
                if line_v == Value::Number(base_v) {
                    line_v = top0.clone();
                }
            }
        }
        let tb_floats = objs
            .iter()
            .filter(|q| {
                !truth(&q["inline"])
                    && node(q).is_ok_and(|el| {
                        d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM") == "TOP_AND_BOTTOM"
                            && pm::attr(d, child(d, el, "pos"), "vertRelTo").unwrap_or("PARA")
                                == "PARA"
                    })
            })
            .count();
        let mut stay = tb_floats < 2
            && !objs.iter().any(|q| truth(&q["inline"]))
            && !chars
                .iter()
                .any(|c| !ch(c).is_empty() && !truth(&c["nb"]) && ink(ch(c)));
        let top_for_push = if !nullish(&p_top) {
            p_top.clone()
        } else if depth > 0 {
            cell_para_top(d, p, &ctx.styles)
        } else {
            Value::Undefined
        };
        let fallback_line = Value::Number(base_v);
        if stay
            && wrap == "TOP_AND_BOTTOM"
            && object_anchor::line_pushed_by(
                d,
                o,
                if nullish(&line_v) {
                    &fallback_line
                } else {
                    &line_v
                },
                &top_for_push,
            )?
        {
            stay = false;
        }
        let mut carried = None;
        if let (Some(pages), Some(k)) = (seg_pages, anchor_k)
            && k > 0
            && pages.get(k) != pages.first()
            && pages.get(k - 1) == pages.first()
            && wrap == "TOP_AND_BOTTOM"
            && pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") == "PARA"
        {
            let s = &plans[k - 1].seg;
            carried = Some(
                number(&s["vertpos"])
                    + number(&s["textheight"])
                    + if nullish(&s["spacing"]) {
                        0.0
                    } else {
                        number(&s["spacing"])
                    },
            );
        }
        let first = line_v.is_undefined() || line_v == Value::Number(base_v);
        let tail = chars
            .iter()
            .skip(number(&o["index"]).max(0.0) as usize)
            .any(|c| ink(ch(c)));
        let anchor_at = if tail
            && anchor_k.is_some_and(|k| {
                k != 0
                    && (number(&plans[0].seg["horzsize"]) - number(&plans[k].seg["horzsize"])).abs()
                        <= 100.0
                    && number(&plans[k].seg["horzpos"]) + number(&plans[k].seg["horzsize"])
                        < aw - 1000.0
            }) {
            "top"
        } else if tail {
            "mid"
        } else {
            "end"
        };
        let mut a = args(0.0, 0.0, base_v, aw);
        a.ox = ax;
        a.oy = oy.clone();
        a.base_v = obj_base;
        a.anchor_line_v = line_v;
        a.stay_line = if stay { before.clone() } else { false.into() };
        a.para_start_v =
            if (first || wrap == "SQUARE") && !p_top.is_undefined() && number(&p_top) < base_v {
                p_top.clone()
            } else {
                Value::Undefined
            };
        a.line_h = line_h.into();
        a.line_sb = if first { before.clone() } else { 0.0.into() };
        a.anchor_at = anchor_at.into();
        let (x, mut y) = ctx.anchor(d, o, &a)?;
        if let Some(carried) = carried {
            let raw = num(d, pos, "vertOffset", 0.0);
            y = offsets(
                &oy,
                &[
                    carried,
                    num(d, child(d, el, "outMargin"), "top", 0.0),
                    maximum(
                        0.0,
                        if raw >= 2147483648.0 {
                            raw - 4294967296.0
                        } else {
                            raw
                        },
                    ),
                ],
            );
        }
        let mut stack_top = if depth == 0 {
            if nullish(&p_top) {
                0.into()
            } else {
                p_top.clone()
            }
        } else {
            cell_para_top(d, p, &ctx.styles)
        };
        if tb_floats >= 2 && wrap == "TOP_AND_BOTTOM" && !stack_top.is_undefined() {
            if stacked.is_none() {
                let mut map = HashMap::new();
                let mut placed = Vec::<(f64, f64, f64)>::new();
                let mut stackable = objs
                    .iter()
                    .filter(|q| {
                        !truth(&q["inline"])
                            && node(q).is_ok_and(|el| {
                                d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM")
                                    == "TOP_AND_BOTTOM"
                                    && pm::attr(d, child(d, el, "pos"), "vertRelTo")
                                        .unwrap_or("PARA")
                                        == "PARA"
                                    && pm::attr(d, child(d, el, "pos"), "allowOverlap") != Some("1")
                            })
                    })
                    .collect::<Vec<_>>();
                if stackable.len() >= 2 {
                    stackable.sort_by(|a, b| {
                        (number(&a["index"]) - number(&b["index"]))
                            .partial_cmp(&0.0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    for q in stackable {
                        let qel = node(q)?;
                        let qp = child(d, qel, "pos");
                        let om = child(d, qel, "outMargin");
                        let raw = num(d, qp, "vertOffset", 0.0);
                        let x0 = num(d, qp, "horzOffset", 0.0);
                        let x1 = x0
                            + num(d, om, "left", 0.0)
                            + number(&q["width"])
                            + num(d, om, "right", 0.0);
                        let below = placed
                            .iter()
                            .filter(|(a, b, _)| *a < x1 && x0 < *b)
                            .fold(0.0, |v, (_, _, b)| maximum(v, *b));
                        let top = maximum(below, if depth > 0 { maximum(0.0, raw) } else { raw });
                        map.insert(qel, top + num(d, om, "top", 0.0));
                        placed.push((
                            x0,
                            x1,
                            top + num(d, om, "top", 0.0)
                                + object_height(d, q, ctx)?
                                + num(d, om, "bottom", 0.0),
                        ));
                    }
                }
                if depth > 0 && !placed.is_empty() {
                    stacked_top = Some(maximum(
                        number(&stack_top),
                        base_v
                            - placed
                                .iter()
                                .fold(f64::NEG_INFINITY, |v, (_, _, b)| maximum(v, *b)),
                    ));
                }
                stacked = Some(map);
            }
            if depth > 0
                && let Some(top) = stacked_top
            {
                stack_top = top.into();
            }
            if let Some(top) = stacked.as_ref().and_then(|m| m.get(&el)) {
                y = super::numbering::add(&super::numbering::add(&oy, &stack_top), &(*top).into());
            }
        }
        if let Some(moved) = ctx.obj_place.get(&el).cloned().filter(truth) {
            let saved = ctx.page.clone();
            ctx.page = moved["page"].clone();
            call_object(
                renderer,
                d,
                o,
                x,
                moved["y"].clone(),
                base_v,
                area_w,
                depth,
                ctx,
            )?;
            ctx.page = saved;
        } else {
            call_object(renderer, d, o, x, y, base_v, area_w, depth, ctx)?;
        }
    }
    if region_id.is_some() {
        ctx.parent_stack.pop();
    }
    Ok(())
}
#[cfg(test)]
fn select(d: &ParsedXml, path: &Value) -> Result<usize, String> {
    let mut at = d.root;
    if let Some(path) = path.as_array() {
        for i in path {
            at = *d.nodes[at]
                .children
                .get(i.as_u64().ok_or("노드 경로 번호 오류")? as usize)
                .ok_or("노드 경로 범위 오류")?;
        }
    }
    Ok(at)
}
#[cfg(test)]
pub(crate) fn report(value: &Value) -> serde_json::Value {
    match value {
        Value::Undefined => serde_json::json!({"$undefined":true}),
        Value::Number(n) if !n.is_finite() || *n == 0.0 && n.is_sign_negative() => {
            serde_json::json!({"$number":if n.is_nan(){"NaN"}else if *n==f64::INFINITY{"Infinity"}else if *n==f64::NEG_INFINITY{"-Infinity"}else{"-0"}})
        }
        Value::Array(a) => a.iter().map(report).collect(),
        Value::Object(o) => {
            serde_json::Value::Object(o.iter().map(|(k, v)| (k.clone(), report(v))).collect())
        }
        _ => value.to_json(),
    }
}
#[cfg(test)]
fn snapshot(ctx: &RenderContext) -> Value {
    json!({"page":ctx.page,"parentStack":ctx.parent_stack,"escOrigin":ctx.esc_origin,"stats":ctx.stats})
}
#[cfg(test)]
struct RecordingObjects {
    calls: Vec<Value>,
    updates: Vec<Value>,
    throw_at: Option<usize>,
    emit: bool,
}
#[cfg(test)]
impl DrawObjects for RecordingObjects {
    fn draw_object(
        &mut self,
        _: &mut ParsedXml,
        a: ObjectArgs,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        let at = self.calls.len();
        self.calls.push(json!({"object":a.object,"x":a.x,"y":a.y,"baseV":a.base_v,"areaW":a.area_w,"depth":a.depth,"state":snapshot(ctx)}));
        if self.throw_at == Some(at) {
            return Err("합성 drawObject 실패".into());
        }
        if self.emit {
            ctx.emit(format!("<!-- object {at} -->").into())?;
        }
        if let Some(update) = self.updates.get(at) {
            if let Some(v) = update.get("page") {
                ctx.page = v.clone();
            }
            if let Some(v) = update.get("escOrigin") {
                ctx.esc_origin = v.clone();
            }
            if let Some(v) = update.get("texts") {
                ctx.stats["texts"] = v.clone();
            }
        }
        Ok(())
    }
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut i = Value::from_json(input);
    if let Some(xml) = input["headerXml"].as_str() {
        i["styles"] = head_styles::parse_render_styles(xml, None)?;
    }
    if let Some(patches) = input["nativeNumbers"].as_array() {
        for patch in patches {
            let n = match patch["value"].as_str() {
                Some("NaN") => f64::NAN,
                Some("Infinity") => f64::INFINITY,
                Some("-Infinity") => f64::NEG_INFINITY,
                Some("-0") => -0.0,
                _ => return Err("특수 숫자 오류".into()),
            };
            let mut at = &mut i;
            for part in patch["path"].as_array().ok_or("특수 숫자 경로 오류")? {
                at = if let Some(k) = part.as_str() {
                    at.get_mut(k)
                } else {
                    at.get_mut(part.as_u64().ok_or("특수 숫자 키 오류")? as usize)
                }
                .ok_or("특수 숫자 값 없음")?;
            }
            *at = n.into();
        }
    }
    let mut d = super::style_dom::parse_style_xml(input["xml"].as_str().ok_or("xml 필요")?)?;
    if op == "cellParaTop" {
        return Ok(report(&cell_para_top(
            &d,
            select(&d, &i["path"])?,
            &i["styles"],
        )));
    }
    if !matches!(op, "drawPara" | "sequence") {
        return Err("알 수 없는 문단 그리기 연산".into());
    }
    let initial = &i["context"];
    let count = initial["pageCount"].as_u64().unwrap_or(3) as usize;
    if count > 500_000 {
        return Err("출력 쪽 수가 DOM 노드 한도를 넘었습니다.".into());
    }
    let mut ctx = RenderContext::new(i["styles"].clone(), count);
    if let Some(v) = initial.get("page") {
        ctx.page = v.clone();
    }
    if let Some(v) = initial.get("pageBase") {
        ctx.page_base = v.clone();
    }
    if let Some(v) = initial.get("pageH") {
        ctx.page_h = number(v);
    }
    ctx.geom = initial.get("geom").cloned().unwrap_or_else(
        || json!({"PW":59500,"PH":84200,"ML":8500,"MT":6200,"BODY_W":42500,"BODY_H":72000}),
    );
    ctx.cols = initial["cols"].clone();
    ctx.esc_origin = initial["escOrigin"].clone();
    if let Some(stack) = initial["parentStack"].as_array() {
        ctx.parent_stack = stack.iter().map(string).collect();
    }
    if let Some(rotations) = initial["regionRotations"].as_array() {
        ctx.region_rotations = rotations.clone();
    }
    ctx.nums = CounterState::from_value(&initial["nums"])?;
    if let Some(words) = initial["highlights"].as_array() {
        for word in words {
            let s = string(word);
            let s = s.trim_matches(crate::input::is_space).to_lowercase();
            if !s.is_empty() {
                ctx.highlights.push(s.into());
            }
        }
    }
    if let Some(words) = initial["highlightUnits"].as_array() {
        for word in words {
            ctx.highlights.push(LayoutText(
                word.as_array()
                    .ok_or("강조 UTF16 배열 오류")?
                    .iter()
                    .map(|n| {
                        n.as_u64()
                            .and_then(|n| u16::try_from(n).ok())
                            .ok_or("강조 UTF16 단위 오류")
                    })
                    .collect::<Result<_, _>>()?,
            ));
        }
    }
    for (name, map) in [
        ("paraTops", &mut ctx.para_tops),
        ("objPlace", &mut ctx.obj_place),
    ] {
        if let Some(entries) = initial[name].as_array() {
            for entry in entries {
                map.insert(select(&d, &entry[0])?, entry[1].clone());
            }
        }
    }
    ctx.extent.memoize = true;
    if let Some(entries) = ctx.styles["paraGeom"].as_array() {
        for pair in entries {
            if let Some(id) = pair[0].as_str() {
                ctx.extent.para_geom.insert(id.into(), pair[1].clone());
            }
        }
    }
    pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx.extent);
    let body_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
    if let Some(values) = initial["bodyDx"].as_array() {
        let values = values.clone();
        let calls = body_calls.clone();
        ctx.body_dx = Some(Box::new(move |page| {
            calls.borrow_mut().push(page.clone());
            Ok(page
                .as_u64()
                .and_then(|p| values.get(p as usize))
                .cloned()
                .unwrap_or(Value::Undefined))
        }));
    }
    let mut objects = RecordingObjects {
        calls: Vec::new(),
        updates: i["updates"].as_array().cloned().unwrap_or_default(),
        throw_at: i["throwAt"].as_u64().map(|n| n as usize),
        emit: truth(&i["emitObjects"]),
    };
    let draws = if op == "sequence" {
        i["draws"].as_array().ok_or("draws 배열 필요")?.clone()
    } else {
        vec![i["draw"].clone()]
    };
    let mut errors = Vec::new();
    for draw in draws {
        let selected = select(&d, &draw["path"])?;
        let result = draw_para(
            &mut d,
            selected,
            draw.get("ox").map_or(0.0, number),
            draw.get("oy").map_or(0.0, number),
            draw.get("areaW").map_or(10000.0, number),
            &mut ctx,
            draw["depth"].as_u64().unwrap_or(0) as usize,
            draw["segPages"].as_array().map(Vec::as_slice),
            &mut objects,
        );
        errors.push(result.err());
    }
    let pages = ctx
        .pages
        .iter()
        .map(|chunks| chunks.iter().map(|s| json!(s.0)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let validation = ctx
        .pages
        .iter()
        .map(|chunks| {
            let mut svg = LayoutText::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
            for (k, chunk) in chunks.iter().enumerate() {
                if k > 0 {
                    svg.append(&TextRef::Utf8("\n"));
                }
                svg.append(&chunk.as_text_ref());
            }
            svg.append(&TextRef::Utf8("</svg>"));
            let result = svg
                .to_string_strict()
                .map_err(|_| "XML에 쓸 수 없는 문자입니다.".to_owned())
                .and_then(|s| super::svg_safety::validate_rendered_svg(&s));
            json!({"accepted":result.is_ok(),"error":result.err()})
        })
        .collect::<Vec<_>>();
    let mut table_memo = ctx
        .extent
        .table_memo
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect::<Vec<_>>();
    table_memo.sort_by_key(|v| v.0);
    let mut cell_memo = ctx
        .extent
        .cell_memo
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect::<Vec<_>>();
    cell_memo.sort_by_key(|v| v.0);
    Ok(report(
        &json!({"pages":pages,"validation":validation,"regions":ctx.regions.regions,"nums":ctx.nums.to_value(),"warnings":ctx.warnings,"warned":ctx.warned_order,"state":snapshot(&ctx),"calls":objects.calls,"bodyCalls":body_calls.borrow().clone(),"errors":errors,"tableMemo":table_memo.iter().map(|(a,b)|json!([a,b])).collect::<Vec<_>>(),"cellMemo":cell_memo.iter().map(|(a,b)|json!([a,b])).collect::<Vec<_>>()}),
    ))
}
