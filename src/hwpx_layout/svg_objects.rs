//! kordoc svg-content.mjs object/group/shape renderer, MIT, Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE. Equation conversion retains its separate notices.
use super::{
    equation,
    layout::maximum,
    layout_text::{LayoutText, TextRef},
    numbering::{add, truth},
    para_model::{self as pm, child, num},
    shape_geometry::{self as sg, ShapeContext, ShapeObject},
    style_dom::{ParsedXml, XmlPart},
    svg_para::{
        DrawObjects, ObjectArgs, RenderContext, draw_para, draw_para_native, pt, region_open, round,
    },
    value::{Value, native_json as json, number, string},
};

pub const SHAPE_TAGS: &[&str] = &["rect", "ellipse", "line", "polygon", "curv", "curve", "arc"];

/// Only terminal table/image painting is injected; all recursive object layout
/// uses this module and the real paragraph renderer.
pub trait DrawLeaves: DrawObjects {
    fn draw_table(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        ctx: &mut RenderContext,
        depth: usize,
    ) -> Result<(), String>;
    fn draw_pic(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        ctx: &mut RenderContext,
    ) -> Result<(), String>;
    #[allow(clippy::too_many_arguments)]
    fn draw_pic_frame(
        &mut self,
        doc: &mut ParsedXml,
        node: usize,
        x: Value,
        y: Value,
        w: Value,
        h: Value,
        extra: &str,
        ctx: &mut RenderContext,
    ) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub struct PaintJob {
    pub page: Value,
    pub layer: i32,
    pub z: f64,
    pub body: bool,
    pub parents: Vec<String>,
    pub esc_origin: Value,
    pub args: ObjectArgs,
}
impl PaintJob {
    /// Caller disables paint_jobs and selects the output layer/bodyDx exactly as
    /// svg-render does. On failure, retain the original callback's partial state.
    pub fn replay<R: DrawLeaves>(
        &self,
        doc: &mut ParsedXml,
        ctx: &mut RenderContext,
        renderer: &mut R,
    ) -> Result<(), String> {
        if ctx.paint_jobs.is_some() {
            return Err("지연 그리기 실행 전에 수집을 종료해야 합니다.".into());
        }
        ctx.page = self.page.clone();
        let saved = std::mem::replace(&mut ctx.parent_stack, self.parents.clone());
        ctx.esc_origin = self.esc_origin.clone();
        draw_object(doc, self.args.clone(), ctx, renderer)?;
        ctx.parent_stack = saved;
        Ok(())
    }
}
/// Stable ordering, including JavaScript comparator NaN-as-equal behavior.
pub fn sort_paint_jobs(jobs: &mut [PaintJob]) {
    jobs.sort_by(|a, b| {
        a.layer.cmp(&b.layer).then_with(|| {
            (a.z - b.z)
                .partial_cmp(&0.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
}
fn node(o: &Value, doc: &ParsedXml) -> Result<usize, String> {
    o["el"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&v| v < doc.nodes.len())
        .ok_or_else(|| "개체 노드 없음".into())
}
fn object(node: usize, tag: &str, w: f64, h: f64) -> Value {
    json!({"el":node,"tag":tag,"index":0,"inline":true,"width":w,"height":h,"omL":0,"omR":0})
}
fn contents(doc: &ParsedXml, node: usize) -> String {
    let mut result = String::new();
    let mut stack = doc.nodes[node].parts.iter().rev().collect::<Vec<_>>();
    while let Some(part) = stack.pop() {
        match part {
            XmlPart::Text(s) => result.push_str(s),
            XmlPart::Node(id) => stack.extend(doc.nodes[*id].parts.iter().rev()),
        }
    }
    result
}
fn inc(ctx: &mut RenderContext, key: &str) {
    ctx.stats[key] = (number(&ctx.stats[key]) + 1.0).into();
}

pub fn draw_object<R: DrawLeaves>(
    doc: &mut ParsedXml,
    args: ObjectArgs,
    ctx: &mut RenderContext,
    renderer: &mut R,
) -> Result<(), String> {
    let el = node(&args.object, doc)?;
    let o = &args.object;
    if !truth(&o["inline"]) && args.depth == 0 && ctx.paint_jobs.is_some() {
        let layer = match doc.nodes[el].attr("textWrap") {
            Some("BEHIND_TEXT") => -1,
            Some("IN_FRONT_OF_TEXT") => 1,
            _ => 0,
        };
        let job = PaintJob {
            page: ctx.page.clone(),
            layer,
            z: num(doc, Some(el), "zOrder", 0.0),
            body: ctx.body_dx.is_some(),
            parents: ctx.parent_stack.clone(),
            esc_origin: ctx.esc_origin.clone(),
            args,
        };
        ctx.paint_jobs.as_mut().expect("checked").push(job);
        return Ok(());
    }
    let tag = string(&o["tag"]);
    let mut x = args.x.clone();
    let mut y = args.y.clone();
    if matches!(tag.as_str(), "tbl" | "pic")
        && let Some(cap) = pm::caption_of(doc, el)
    {
        let obj_h = if tag == "tbl" {
            maximum(
                number(&o["height"]),
                pm::measure_table_height(doc, el, &mut ctx.extent)?,
            )
            .into()
        } else {
            o["height"].clone()
        };
        let mut cx = x.clone();
        let mut cy = y.clone();
        match cap["side"].as_str() {
            Some("TOP") => y = add(&add(&y, &cap["h"]), &cap["gap"]),
            Some("LEFT") => x = add(&add(&x, &cap["width"]), &cap["gap"]),
            Some("RIGHT") => cx = add(&add(&x, &o["width"]), &cap["gap"]),
            _ => cy = add(&add(&y, &obj_h), &cap["gap"]),
        }
        let sub = cap["sub"].as_u64().ok_or("캡션 sub 없음")? as usize;
        for cp in doc.nodes[sub].children.clone() {
            if doc.nodes[cp].local == "p" {
                draw_para_native(
                    doc,
                    cp,
                    cx.clone(),
                    cy.clone(),
                    number(&cap["width"]),
                    ctx,
                    args.depth + 1,
                    None,
                    renderer,
                )?;
            }
        }
    }
    match tag.as_str() {
        "tbl" => renderer.draw_table(doc, el, x, y, ctx, args.depth + 1)?,
        "pic" => renderer.draw_pic(doc, el, x, y, ctx)?,
        "container" => {
            if draw_group_by_matrix(
                doc,
                &args.object,
                x.clone(),
                y.clone(),
                args.base_v,
                args.area_w,
                ctx,
                args.depth,
                renderer,
            )? {
                return Ok(());
            }
            for ch in doc.nodes[el].children.clone() {
                let tag = doc.nodes[ch].local.clone();
                if !pm::OBJ_TAGS.contains(&tag.as_str()) {
                    continue;
                }
                let sz = child(doc, ch, "sz");
                let off = child(doc, ch, "offset");
                let sub = object(
                    ch,
                    &tag,
                    num(doc, sz, "width", 0.0),
                    num(doc, sz, "height", 0.0),
                );
                draw_object(
                    doc,
                    ObjectArgs {
                        object: sub,
                        x: add(&x, &num(doc, off, "x", 0.0).into()),
                        y: add(&y, &num(doc, off, "y", 0.0).into()),
                        depth: args.depth + 1,
                        ..args.clone()
                    },
                    ctx,
                    renderer,
                )?;
            }
        }
        "equation" => {
            let script = child(doc, el, "script")
                .map(|id| contents(doc, id))
                .unwrap_or_default();
            let text = if script.is_empty() {
                LayoutText::default()
            } else {
                let latex = equation::hwp_equation_to_latex_utf16(
                    &script.encode_utf16().collect::<Vec<_>>(),
                )?;
                LayoutText::from(equation::latex_to_plain_utf16(&latex.units)?)
            };
            if !text.0.is_empty() && number(&o["width"]) > 0.0 {
                let bl = number(
                    &doc.nodes[el]
                        .attr("baseLine")
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                );
                let size = num(doc, Some(el), "baseUnit", 1000.0);
                let by = add(
                    &y,
                    &round(
                        number(&o["height"])
                            * if bl.is_finite() && bl > 0.0 {
                                bl / 100.0
                            } else {
                                0.85
                            },
                    )
                    .into(),
                );
                let mut attrs = format!(
                    "x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"'Times New Roman','HCR Batang','Noto Serif KR',serif\"",
                    pt(number(&x)),
                    pt(number(&by)),
                    pt(size)
                );
                if text.as_text_ref().code_points().count() > 1 {
                    attrs.push_str(&format!(
                        " textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"",
                        pt(number(&o["width"]))
                    ));
                }
                if let Some(color) = doc.nodes[el]
                    .attr("textColor")
                    .filter(|s| !s.is_empty() && *s != "#000000")
                {
                    attrs.push_str(&format!(
                        " fill=\"{}\"",
                        LayoutText::from(color).escape_xml().to_string_strict()?
                    ));
                }
                let mut out = LayoutText::from(format!("<text {attrs}>"));
                out.append(&text.escape_xml().as_text_ref());
                out.append(&TextRef::Utf8("</text>"));
                ctx.emit(out)?;
                inc(ctx, "texts");
            }
        }
        tag if SHAPE_TAGS.contains(&tag) => {
            draw_shape(doc, o, x, y, ctx, args.depth, None, renderer)?
        }
        tag if !pm::FORM_TAGS.contains(&tag) => ctx.warn_once(
            &format!("shape:{tag}"),
            &format!("개체({tag}) 렌더 미지원 — 생략"),
        ),
        _ => {}
    }
    Ok(())
}

// V8's two-argument Math.hypot path scales before square/sqrt, with no
// platform hypot approximation (and Infinity taking priority over NaN).
fn hypot(a: f64, b: f64) -> f64 {
    let a = a.abs();
    let b = b.abs();
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let hi = a.max(b);
    let lo = a.min(b);
    if hi == 0.0 {
        0.0
    } else {
        let q = lo / hi;
        (1.0 + q * q).sqrt() * hi
    }
}
#[allow(clippy::too_many_arguments)]
pub fn draw_group_by_matrix<R: DrawLeaves>(
    doc: &mut ParsedXml,
    o: &Value,
    x: Value,
    y: Value,
    base_v: f64,
    area_w: f64,
    ctx: &mut RenderContext,
    depth: usize,
    renderer: &mut R,
) -> Result<bool, String> {
    let el = node(o, doc)?;
    let Some(top) = sg::rendering_matrix(doc, el) else {
        return Ok(false);
    };
    let mut leaves = Vec::new();
    let mut stack = doc.nodes[el]
        .children
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    while let Some(ch) = stack.pop() {
        let tag = &doc.nodes[ch].local;
        if !pm::OBJ_TAGS.contains(&tag.as_str()) {
            continue;
        }
        if tag == "container" {
            stack.extend(doc.nodes[ch].children.iter().rev().copied());
        } else if sg::rendering_matrix(doc, ch).is_some() {
            leaves.push(ch);
        } else {
            return Ok(false);
        }
    }
    let org = child(doc, el, "orgSz");
    let bounds = sg::rotated_bounds(
        &json!({"x":0,"y":0,"w":num(doc,org,"width",0.0),"h":num(doc,org,"height",0.0)}),
        &[json!(top)],
    );
    let base = [
        1.0,
        0.0,
        0.0,
        1.0,
        number(&x) - number(&bounds["x"]),
        number(&y) - number(&bounds["y"]),
    ];
    for leaf in leaves {
        let m = sg::mul_affine(
            &base,
            &sg::rendering_matrix(doc, leaf).ok_or("그룹 행렬 없음")?,
        );
        let tag = doc.nodes[leaf].local.clone();
        let org = child(doc, leaf, "orgSz");
        let cur = child(doc, leaf, "curSz");
        let choose = |name| {
            let v = num(doc, org, name, 0.0);
            if v == 0.0 || v.is_nan() {
                num(doc, cur, name, 0.0)
            } else {
                v
            }
        };
        let w = choose("width");
        let h = choose("height");
        ctx.region_rotations.push(json!(m));
        let matrix = m[..4]
            .iter()
            .map(|v| string(&(round(v * 1e6) / 1e6).into()))
            .collect::<Vec<_>>()
            .join(" ");
        ctx.emit(
            format!(
                "<g transform=\"matrix({matrix} {} {})\">",
                pt(m[4]),
                pt(m[5])
            )
            .into(),
        )?;
        let sub = object(leaf, &tag, w, h);
        if tag == "pic" {
            renderer.draw_pic_frame(doc, leaf, 0.into(), 0.into(), w.into(), h.into(), "", ctx)?;
        } else if SHAPE_TAGS.contains(&tag.as_str()) {
            let choose = |v: f64| if v == 0.0 || v.is_nan() { 1.0 } else { v };
            draw_shape(
                doc,
                &sub,
                0.into(),
                0.into(),
                ctx,
                depth + 1,
                Some([choose(hypot(m[0], m[1])), choose(hypot(m[2], m[3]))]),
                renderer,
            )?;
        } else {
            draw_object(
                doc,
                ObjectArgs {
                    object: sub,
                    x: 0.into(),
                    y: 0.into(),
                    base_v,
                    area_w,
                    depth: depth + 1,
                },
                ctx,
                renderer,
            )?;
        }
        ctx.emit("</g>".into())?;
        ctx.region_rotations.pop();
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_shape<R: DrawLeaves>(
    doc: &mut ParsedXml,
    o: &Value,
    x: Value,
    y: Value,
    ctx: &mut RenderContext,
    depth: usize,
    group_scale: Option<[f64; 2]>,
    renderer: &mut R,
) -> Result<(), String> {
    let el = node(o, doc)?;
    let mut shape = ShapeContext {
        defs: std::mem::take(&mut ctx.defs),
        images: std::mem::take(&mut ctx.images),
        warnings: Vec::new(),
    };
    let result = sg::shape_geometry(
        doc,
        el,
        &ShapeObject {
            tag: o["tag"].clone(),
            width: o["width"].clone(),
            height: o["height"].clone(),
        },
        x,
        y,
        &mut shape,
        group_scale.is_some(),
    );
    ctx.defs = shape.defs;
    ctx.images = shape.images;
    for warning in shape.warnings {
        ctx.warn_once(&string(&warning["key"]), &string(&warning["message"]));
    }
    let g = result?;
    if truth(&g["rotation"]) {
        ctx.region_rotations.push(g["rotation"].clone());
    }
    inc(ctx, "shapes");
    let bbox = ctx.bbox_value(&g["x"], &g["y"], &g["w"], &g["h"])?;
    let parent = ctx
        .parent_stack
        .last()
        .map(|s| Value::from(s.clone()))
        .unwrap_or(Value::Undefined);
    let id = ctx.regions.add("shape", bbox, &json!({"parentId":parent}));
    ctx.emit(region_open(&id, "shape", ctx.page_no()))?;
    ctx.parent_stack.push(id);
    if truth(&g["transform"]) {
        ctx.emit(format!("<g transform=\"{}\">", string(&g["transform"])).into())?;
    }
    ctx.emit(string(&g["svg"]).into())?;
    if let Some(dt) = child(doc, el, "drawText")
        && let Some(sub) = child(doc, dt, "subList")
    {
        let [sx, sy] = group_scale.unwrap_or([1.0, 1.0]);
        let unscale = sx != 1.0 || sy != 1.0;
        if unscale {
            ctx.emit(
                format!(
                    "<g transform=\"scale({} {})\">",
                    string(&(1.0 / sx).into()),
                    string(&(1.0 / sy).into())
                )
                .into(),
            )?;
            ctx.region_rotations
                .push(json!([1.0 / sx, 0, 0, 1.0 / sy, 0, 0]));
        }
        let tm = child(doc, dt, "textMargin");
        let ml = num(doc, tm, "left", 0.0);
        let mr = num(doc, tm, "right", 0.0);
        let mt = num(doc, tm, "top", 0.0);
        let mb = num(doc, tm, "bottom", 0.0);
        let va = doc.nodes[sub].attr("vertAlign").unwrap_or("TOP").to_owned();
        let mut floats = false;
        for &q in &doc.nodes[sub].children {
            if doc.nodes[q].local == "p" {
                // The original buildPara uses module-level deletion/note maps;
                // their Rust equivalent is the shared context, not a fresh memo.
                let m = pm::build_para(doc, q, &mut ctx.extent)?;
                if m["objs"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|o| !truth(&o["inline"])))
                {
                    floats = true;
                    break;
                }
            }
        }
        let mut yoff = 0.0;
        if !floats && matches!(va.as_str(), "CENTER" | "BOTTOM") {
            let cell = json!({"el":dt,"ca":0,"ra":0,"cs":1,"rs":1,"w":0,"h":0,"bfId":null,"sub":sub,"marginL":0,"marginR":0,"marginT":0,"marginB":0});
            let extent = pm::cell_content_extent(doc, &cell, &mut ctx.extent)?;
            let free = number(&g["h"]) * sy - mt - mb - extent;
            yoff = maximum(0.0, if va == "CENTER" { free / 2.0 } else { free });
        }
        for p in doc.nodes[sub].children.clone() {
            if doc.nodes[p].local == "p" {
                draw_para(
                    doc,
                    p,
                    number(&g["x"]) * sx + ml,
                    number(&g["y"]) * sy + mt + yoff,
                    number(&g["w"]) * sx - ml - mr,
                    ctx,
                    depth + 1,
                    None,
                    renderer,
                )?;
            }
        }
        if unscale {
            ctx.region_rotations.pop();
            ctx.emit("</g>".into())?;
        }
    }
    if truth(&g["rotation"]) {
        ctx.region_rotations.pop();
    }
    if truth(&g["transform"]) {
        ctx.emit("</g>".into())?;
    }
    ctx.parent_stack.pop();
    ctx.emit("</g>".into())?;
    Ok(())
}

#[cfg(test)]
fn select(doc: &ParsedXml, path: &Value) -> Result<usize, String> {
    let mut id = doc.root;
    if let Some(parts) = path.as_array() {
        for part in parts {
            id = *doc.nodes[id]
                .children
                .get(part.as_u64().ok_or("XML 자식 위치 오류")? as usize)
                .ok_or("XML 자식 위치 범위 오류")?;
        }
    }
    Ok(id)
}
#[cfg(test)]
fn snapshot(ctx: &RenderContext) -> Value {
    json!({"page":ctx.page,"parentStack":ctx.parent_stack,"escOrigin":ctx.esc_origin,"stats":ctx.stats,"regionRotations":ctx.region_rotations})
}
#[cfg(test)]
struct RecordingLeaves {
    calls: Vec<Value>,
    throw_at: Option<usize>,
    updates: Vec<Value>,
    emit: bool,
}
#[cfg(test)]
impl RecordingLeaves {
    fn record(
        &mut self,
        kind: &str,
        node: usize,
        x: Value,
        y: Value,
        extra: Value,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        let at = self.calls.len();
        self.calls
            .push(json!({"kind":kind,"node":node,"x":x,"y":y,"extra":extra,"state":snapshot(ctx)}));
        if self.throw_at == Some(at) {
            return Err("합성 leaf 실패".into());
        }
        if self.emit {
            ctx.emit(format!("<!-- leaf {at} -->").into())?;
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
            if let Some(v) = update.get("def") {
                ctx.defs.push(v.clone());
            }
            if let Some(v) = update.get("image") {
                ctx.images.insert(string(&v[0]), v[1].clone());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
impl DrawObjects for RecordingLeaves {
    fn draw_object(
        &mut self,
        d: &mut ParsedXml,
        a: ObjectArgs,
        c: &mut RenderContext,
    ) -> Result<(), String> {
        draw_object(d, a, c, self)
    }
}
#[cfg(test)]
impl DrawLeaves for RecordingLeaves {
    fn draw_table(
        &mut self,
        _: &mut ParsedXml,
        n: usize,
        x: Value,
        y: Value,
        c: &mut RenderContext,
        depth: usize,
    ) -> Result<(), String> {
        self.record("table", n, x, y, json!({"depth":depth}), c)
    }
    fn draw_pic(
        &mut self,
        _: &mut ParsedXml,
        n: usize,
        x: Value,
        y: Value,
        c: &mut RenderContext,
    ) -> Result<(), String> {
        self.record("pic", n, x, y, json!({}), c)
    }
    fn draw_pic_frame(
        &mut self,
        _: &mut ParsedXml,
        n: usize,
        x: Value,
        y: Value,
        w: Value,
        h: Value,
        extra: &str,
        c: &mut RenderContext,
    ) -> Result<(), String> {
        self.record("picFrame", n, x, y, json!({"w":w,"h":h,"extra":extra}), c)
    }
}
#[cfg(test)]
fn args(doc: &ParsedXml, input: &Value) -> Result<ObjectArgs, String> {
    let el = select(doc, &input["path"])?;
    let mut o = object(el, &doc.nodes[el].local, 1000.0, 1000.0);
    if let Some(values) = input["object"].as_object() {
        for (k, v) in values.iter() {
            o[k] = v.clone();
        }
    }
    o["el"] = el.into();
    Ok(ObjectArgs {
        object: o,
        x: input.get("x").cloned().unwrap_or(0.into()),
        y: input.get("y").cloned().unwrap_or(0.into()),
        base_v: input.get("baseV").map_or(0.0, number),
        area_w: input.get("areaW").map_or(10000.0, number),
        depth: input["depth"].as_u64().unwrap_or(0) as usize,
    })
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut i = Value::from_json(input);
    if let Some(header) = input["headerXml"].as_str() {
        i["styles"] = super::head_styles::parse_render_styles(header, None)?;
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
                    at.as_array_mut()
                        .and_then(|a| a.get_mut(part.as_u64()? as usize))
                }
                .ok_or("특수 숫자 경로 없음")?;
            }
            *at = n.into();
        }
    }
    let mut doc = super::style_dom::parse_style_xml(i["xml"].as_str().ok_or("xml 필요")?)?;
    let init = &i["context"];
    let count = init["pageCount"].as_u64().unwrap_or(3) as usize;
    if count > 500_000 {
        return Err("출력 쪽 수가 DOM 노드 한도를 넘었습니다.".into());
    }
    let mut ctx = RenderContext::new(i.get("styles").cloned().unwrap_or_else(|| json!({})), count);
    ctx.page = init.get("page").cloned().unwrap_or(0.into());
    ctx.page_base = init.get("pageBase").cloned().unwrap_or(0.into());
    ctx.page_h = init.get("pageH").map_or(100000.0, number);
    ctx.geom = init.get("geom").cloned().unwrap_or_else(
        || json!({"PW":59500,"PH":84200,"ML":8500,"MT":6200,"BODY_W":42500,"BODY_H":72000}),
    );
    ctx.esc_origin = init["escOrigin"].clone();
    ctx.parent_stack = init["parentStack"]
        .as_array()
        .map(|a| a.iter().map(string).collect())
        .unwrap_or_default();
    ctx.region_rotations = init["regionRotations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ctx.defs = init["defs"].as_array().cloned().unwrap_or_default();
    if let Some(images) = init["images"].as_array() {
        for pair in images {
            ctx.images.insert(string(&pair[0]), pair[1].clone());
        }
    }
    ctx.extent.memoize = true;
    pm::prepare_deleted_ranges(&doc, doc.root, 0, &mut ctx.extent);
    if let Some(entries) = ctx.styles["paraGeom"].as_array() {
        for pair in entries {
            ctx.extent
                .para_geom
                .insert(string(&pair[0]), pair[1].clone());
        }
    }
    ctx.nums = super::para_number::CounterState::from_value(&init["nums"])?;
    if let Some(highlights) = init["highlights"].as_array() {
        for word in highlights {
            let text = string(word)
                .trim_matches(crate::input::is_space)
                .to_lowercase();
            if !text.is_empty() {
                ctx.highlights.push(text.into());
            }
        }
    }
    ctx.cols = init["cols"].clone();
    for (name, map) in [
        ("paraTops", &mut ctx.para_tops),
        ("objPlace", &mut ctx.obj_place),
    ] {
        if let Some(entries) = init[name].as_array() {
            for pair in entries {
                map.insert(select(&doc, &pair[0])?, pair[1].clone());
            }
        }
    }
    let body_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
    if let Some(values) = init["bodyDx"].as_array() {
        let values = values.clone();
        let calls = body_calls.clone();
        ctx.body_dx = Some(Box::new(move |page| {
            calls.borrow_mut().push(page.clone());
            Ok(page
                .as_u64()
                .and_then(|n| values.get(n as usize))
                .cloned()
                .unwrap_or(Value::Undefined))
        }));
    }
    if truth(&init["paintJobs"]) {
        ctx.paint_jobs = Some(Vec::new());
    }
    let mut renderer = RecordingLeaves {
        calls: Vec::new(),
        throw_at: i["throwAt"].as_u64().map(|n| n as usize),
        updates: i["updates"].as_array().cloned().unwrap_or_default(),
        emit: truth(&i["emitLeaves"]),
    };
    let draws = if operation == "sequence" {
        i["draws"].as_array().ok_or("draws 배열 필요")?.clone()
    } else {
        vec![i["draw"].clone()]
    };
    let mut results = Vec::new();
    for draw in draws {
        let a = args(&doc, &draw)?;
        let op = draw["operation"].as_str().unwrap_or(operation);
        let result = match op {
            "object" | "sequence" => {
                draw_object(&mut doc, a, &mut ctx, &mut renderer).map(|()| Value::Null)
            }
            "group" => draw_group_by_matrix(
                &mut doc,
                &a.object,
                a.x,
                a.y,
                a.base_v,
                a.area_w,
                &mut ctx,
                a.depth,
                &mut renderer,
            )
            .map(Value::Bool),
            "shape" => {
                let scale = draw["groupScale"]
                    .as_array()
                    .map(|v| [number(&v[0]), number(&v[1])]);
                draw_shape(
                    &mut doc,
                    &a.object,
                    a.x,
                    a.y,
                    &mut ctx,
                    a.depth,
                    scale,
                    &mut renderer,
                )
                .map(|()| Value::Null)
            }
            "para" => draw_para_native(
                &mut doc,
                a.object["el"].as_u64().unwrap() as usize,
                a.x,
                a.y,
                a.area_w,
                &mut ctx,
                a.depth,
                draw["segPages"].as_array().map(Vec::as_slice),
                &mut renderer,
            )
            .map(|()| Value::Null),
            _ => return Err("알 수 없는 개체 그리기 연산".into()),
        };
        results.push(match result {
            Ok(value) => json!({"value":value,"error":null}),
            Err(e) => json!({"value":null,"error":e}),
        });
    }
    let jobs = ctx
        .paint_jobs
        .as_ref()
        .map(|jobs| {
            jobs.iter()
                .map(|j| json!({"page":j.page,"layer":j.layer,"z":j.z,"body":j.body}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut replay = Vec::new();
    if truth(&i["replayJobs"]) {
        let mut jobs = ctx.paint_jobs.take().unwrap_or_default();
        sort_paint_jobs(&mut jobs);
        if let Some(v) = i["beforeReplay"].get("page") {
            ctx.page = v.clone();
        }
        if let Some(v) = i["beforeReplay"].get("escOrigin") {
            ctx.esc_origin = v.clone();
        }
        if let Some(v) = i["beforeReplay"]["parentStack"].as_array() {
            ctx.parent_stack = v.iter().map(string).collect();
        }
        for job in jobs {
            let err = job.replay(&mut doc, &mut ctx, &mut renderer).err();
            replay.push(
                json!({"page":job.page,"layer":job.layer,"z":job.z,"body":job.body,"error":err}),
            );
        }
    }
    let pages = ctx
        .pages
        .iter()
        .map(|page| page.iter().map(|text| json!(text.0)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let validation = ctx
        .pages
        .iter()
        .map(|page| {
            let mut svg = LayoutText::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
            for (k, fragment) in page.iter().enumerate() {
                if k > 0 {
                    svg.append(&TextRef::Utf8("\n"));
                }
                svg.append(&fragment.as_text_ref());
            }
            svg.append(&TextRef::Utf8("</svg>"));
            let result = svg
                .to_string_strict()
                .map_err(|_| "XML에 쓸 수 없는 문자입니다.".to_owned())
                .and_then(|s| super::svg_safety::validate_rendered_svg(&s));
            json!({"accepted":result.is_ok(),"error":result.err()})
        })
        .collect::<Vec<_>>();
    let images = ctx
        .images
        .iter()
        .map(|(k, v)| json!([k, v]))
        .collect::<Vec<_>>();
    let table_memo = (0..doc.nodes.len())
        .filter_map(|n| ctx.extent.table_memo.get(&n).map(|v| json!([n, v])))
        .collect::<Vec<_>>();
    let cell_memo = (0..doc.nodes.len())
        .filter_map(|n| ctx.extent.cell_memo.get(&n).map(|v| json!([n, v])))
        .collect::<Vec<_>>();
    Ok(super::svg_para::report(
        &json!({"pages":pages,"validation":validation,"regions":ctx.regions.regions,"defs":ctx.defs,"images":images,"nums":ctx.nums.to_value(),"warnings":ctx.warnings,"warned":ctx.warned_order,"state":snapshot(&ctx),"calls":renderer.calls,"bodyCalls":body_calls.borrow().clone(),"results":results,"jobs":jobs,"replay":replay,"tableMemo":table_memo,"cellMemo":cell_memo}),
    ))
}
