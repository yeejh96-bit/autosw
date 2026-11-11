//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/shape-geometry.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
use super::head_styles::doc_color;
use super::ieee754;
use super::layout::{maximum, minimum, to_int32};
use super::style_dom::ParsedXml;
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::value::{Map, NativeValue as Value, native_json as json, number, string};

pub const SHAPE_TAGS: [&str; 7] = ["rect", "ellipse", "line", "polygon", "curv", "curve", "arc"];
pub type Affine = [f64; 6];

fn truthy(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => *v != 0.0 && !v.is_nan(),
        Value::String(v) => !v.is_empty(),
        _ => true,
    }
}
fn primitive(v: &Value) -> Value {
    match v {
        Value::Array(_) | Value::Object(_) => Value::String(string(v)),
        _ => v.clone(),
    }
}
fn add(a: &Value, b: &Value) -> Value {
    let a = primitive(a);
    let b = primitive(b);
    if a.as_str().is_some() || b.as_str().is_some() {
        Value::String(format!("{}{}", string(&a), string(&b)))
    } else {
        Value::Number(number(&a) + number(&b))
    }
}
fn n(v: f64) -> Value {
    Value::Number(v)
}
fn ns(v: f64) -> String {
    string(&n(v))
}
fn round(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 {
        return v;
    }
    let lo = v.floor();
    let out = if v - lo < 0.5 { lo } else { lo + 1.0 };
    if out == 0.0 && v < 0.0 { -0.0 } else { out }
}
fn pt(v: &Value) -> String {
    ns(round(number(v)) / 100.0)
}
fn esc(s: &str) -> String {
    s.chars()
        .filter(|&c| !matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
// Number.toFixed(2) rounds the exact binary rational, not an already-rounded
// value*100. Its tie goes upward in magnitude; Rust formatting uses ties-even.
pub(crate) fn fixed2(v: f64) -> String {
    if !v.is_finite() || v.abs() >= 1e21 {
        return ns(v);
    }
    let negative = v < 0.0;
    let v = v.abs();
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let significand = if exp == 0 {
        bits & 0xfffffffffffff
    } else {
        (bits & 0xfffffffffffff) | (1 << 52)
    };
    let exponent = if exp == 0 { -1074 } else { exp - 1023 - 52 };
    let product = u128::from(significand) * 100;
    let rounded = if exponent >= 0 {
        product << exponent
    } else if -exponent >= 128 {
        0
    } else {
        let shift = -exponent as u32;
        let whole = product >> shift;
        let remainder = product & ((1_u128 << shift) - 1);
        whole + u128::from(remainder >= (1_u128 << (shift - 1)))
    };
    format!(
        "{}{}.{:02}",
        if negative { "-" } else { "" },
        rounded / 100,
        rounded % 100
    )
}
fn child(tree: &ParsedXml, el: Option<usize>, local: &str) -> Option<usize> {
    tree.nodes[el?]
        .children
        .iter()
        .copied()
        .find(|&id| tree.nodes[id].local == local)
}
fn attr<'a>(tree: &'a ParsedXml, el: Option<usize>, name: &str) -> Option<&'a str> {
    tree.nodes.get(el?).and_then(|node| node.attr(name))
}
fn num(tree: &ParsedXml, el: Option<usize>, name: &str, fallback: f64) -> f64 {
    let Some(text) = attr(tree, el, name).filter(|s| !s.is_empty()) else {
        return fallback;
    };
    to_int32(number(&Value::String(text.into())), fallback)
}

/// The unchanged box is returned when rotations is empty, including metadata.
pub fn rotated_bounds(box_: &Value, rotations: &[Value]) -> Value {
    if rotations.is_empty() {
        return box_.clone();
    }
    let xw = add(&box_["x"], &box_["w"]);
    let yh = add(&box_["y"], &box_["h"]);
    let mut points = vec![
        [box_["x"].clone(), box_["y"].clone()],
        [xw.clone(), box_["y"].clone()],
        [xw, yh.clone()],
        [box_["x"].clone(), yh],
    ];
    for r in rotations.iter().rev() {
        points = if r.is_array() {
            points
                .into_iter()
                .map(|[x, y]| {
                    [
                        add(
                            &n(number(&r[0]) * number(&x) + number(&r[2]) * number(&y)),
                            &r[4],
                        ),
                        add(
                            &n(number(&r[1]) * number(&x) + number(&r[3]) * number(&y)),
                            &r[5],
                        ),
                    ]
                })
                .collect()
        } else {
            let a = number(&r["angle"]) * std::f64::consts::PI / 180.0;
            let c = ieee754::cos(a);
            let s = ieee754::sin(a);
            points
                .into_iter()
                .map(|[x, y]| {
                    [
                        n(
                            number(&add(&r["cx"], &n(c * (number(&x) - number(&r["cx"])))))
                                - s * (number(&y) - number(&r["cy"])),
                        ),
                        add(
                            &add(&r["cy"], &n(s * (number(&x) - number(&r["cx"])))),
                            &n(c * (number(&y) - number(&r["cy"]))),
                        ),
                    ]
                })
                .collect()
        };
    }
    let x = points
        .iter()
        .fold(f64::INFINITY, |v, p| minimum(v, number(&p[0])));
    let y = points
        .iter()
        .fold(f64::INFINITY, |v, p| minimum(v, number(&p[1])));
    let xmax = points
        .iter()
        .fold(f64::NEG_INFINITY, |v, p| maximum(v, number(&p[0])));
    let ymax = points
        .iter()
        .fold(f64::NEG_INFINITY, |v, p| maximum(v, number(&p[1])));
    json!({"x":x,"y":y,"w":xmax-x,"h":ymax-y})
}
pub fn mul_affine(m: &Affine, n: &Affine) -> Affine {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}
#[cfg(test)]
fn mul_affine_native(m: &Value, n_: &Value) -> Value {
    let term = |a: usize, b: usize, c: usize, d: usize| {
        n(number(&m[a]) * number(&n_[b]) + number(&m[c]) * number(&n_[d]))
    };
    Value::Array(vec![
        term(0, 0, 2, 1),
        term(1, 0, 3, 1),
        term(0, 2, 2, 3),
        term(1, 2, 3, 3),
        add(&term(0, 4, 2, 5), &m[4]),
        add(&term(1, 4, 3, 5), &m[5]),
    ])
}
pub fn rendering_matrix(tree: &ParsedXml, el: usize) -> Option<Affine> {
    let info = child(tree, Some(el), "renderingInfo")?;
    let mut matrix: Option<Affine> = None;
    let mut pending = None;
    let apply = |matrix: &mut Option<Affine>, next: Affine| {
        *matrix = Some(matrix.as_ref().map_or(next, |m| mul_affine(m, &next)));
    };
    for &id in &tree.nodes[info].children {
        let tag = tree.nodes[id].local.as_str();
        if !matches!(tag, "transMatrix" | "scaMatrix" | "rotMatrix") {
            continue;
        }
        let next = [
            num(tree, Some(id), "e1", 1.0),
            num(tree, Some(id), "e4", 0.0),
            num(tree, Some(id), "e2", 0.0),
            num(tree, Some(id), "e5", 1.0),
            num(tree, Some(id), "e3", 0.0),
            num(tree, Some(id), "e6", 0.0),
        ];
        if tag == "scaMatrix" {
            if let Some(p) = pending {
                apply(&mut matrix, p);
            }
            pending = Some(next);
            continue;
        }
        if tag == "rotMatrix"
            && let Some(p) = pending.take()
        {
            apply(&mut matrix, mul_affine(&next, &p));
            continue;
        }
        apply(&mut matrix, next);
    }
    if let Some(p) = pending {
        apply(&mut matrix, p);
    }
    matrix
}
#[derive(Clone, Debug)]
pub struct GeometryBox {
    pub x: Value,
    pub y: Value,
    pub w: Value,
    pub h: Value,
    pub box_w: Option<Value>,
    pub box_h: Option<Value>,
}
pub fn object_geometry(tree: &ParsedXml, el: usize, b: &GeometryBox) -> Value {
    let angle = num(tree, child(tree, Some(el), "rotationInfo"), "angle", 0.0) % 360.0;
    if angle == 0.0 || angle.is_nan() {
        return json!({"x":b.x,"y":b.y,"w":b.w,"h":b.h,"bbox":{"x":b.x,"y":b.y,"w":b.w,"h":b.h},"transform":""});
    }
    let cx = add(&b.x, &n(number(b.box_w.as_ref().unwrap_or(&b.w)) / 2.0));
    let cy = add(&b.y, &n(number(b.box_h.as_ref().unwrap_or(&b.h)) / 2.0));
    let fx = number(&cx) - number(&b.w) / 2.0;
    let fy = number(&cy) - number(&b.h) / 2.0;
    let rotation = json!({"angle":angle,"cx":round(number(&cx)),"cy":round(number(&cy))});
    let bbox = rotated_bounds(
        &json!({"x":fx,"y":fy,"w":b.w,"h":b.h}),
        std::slice::from_ref(&rotation),
    );
    json!({"x":fx,"y":fy,"w":b.w,"h":b.h,"bbox":bbox,"rotation":rotation,"transform":format!("rotate({} {} {})",ns(angle),pt(&cx),pt(&cy))})
}
#[derive(Default, Debug)]
pub struct ShapeContext {
    pub defs: Vec<Value>,
    pub images: Map<String, Value>,
    pub warnings: Vec<Value>,
}
impl ShapeContext {
    fn warn(&mut self, key: String, message: String) {
        self.warnings.push(json!({"key":key,"message":message}));
    }
}
fn shape_fill(tree: &ParsedXml, el: usize, ctx: &mut ShapeContext) -> Result<String, String> {
    let brush = child(tree, Some(el), "fillBrush");
    let win = child(tree, brush, "winBrush");
    if let Some(face) = doc_color(attr(tree, win, "faceColor"), None) {
        return Ok(esc(face));
    }
    if let Some(grad) = child(tree, brush, "gradation") {
        let colors: Vec<_> = tree.nodes[grad]
            .children
            .iter()
            .filter(|&&id| tree.nodes[id].local == "color")
            .map(|&id| doc_color(attr(tree, Some(id), "value"), Some("#000000")).unwrap())
            .collect();
        if colors.is_empty() {
            return Ok("none".into());
        }
        let kind = attr(tree, Some(grad), "type").unwrap_or("LINEAR");
        if kind != "LINEAR" && kind != "RADIAL" {
            ctx.warn(
                format!("gradient:{kind}"),
                format!("그라데이션({kind}) 미지원 — 첫 색으로 채움"),
            );
            return Ok(esc(colors[0]));
        }
        let id = format!("paint{}", ctx.defs.len());
        let opacity = maximum(
            0.0,
            minimum(1.0, 1.0 - num(tree, Some(grad), "alpha", 0.0) / 255.0),
        );
        let stops = colors
            .iter()
            .enumerate()
            .map(|(i, color)| {
                format!(
                    "<stop offset=\"{}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
                    ns(if colors.len() == 1 {
                        0.0
                    } else {
                        i as f64 / (colors.len() - 1) as f64
                    }),
                    esc(color),
                    ns(opacity)
                )
            })
            .collect::<String>();
        let def = if kind == "RADIAL" {
            format!(
                "<radialGradient id=\"{id}\" cx=\"{}%\" cy=\"{}%\">{stops}</radialGradient>",
                ns(num(tree, Some(grad), "centerX", 50.0)),
                ns(num(tree, Some(grad), "centerY", 50.0))
            )
        } else {
            let a = num(tree, Some(grad), "angle", 0.0) * std::f64::consts::PI / 180.0;
            let dx = ieee754::cos(a) / 2.0;
            let dy = -ieee754::sin(a) / 2.0;
            format!(
                "<linearGradient id=\"{id}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{stops}</linearGradient>",
                ns(0.5 - dx),
                ns(0.5 - dy),
                ns(0.5 + dx),
                ns(0.5 + dy)
            )
        };
        ctx.defs.push(Value::String(def));
        return Ok(format!("url(#{id})"));
    }
    let image_brush = child(tree, brush, "imgBrush");
    let image = child(tree, image_brush, "img");
    if image_brush.is_some() && image.is_some() {
        let reference = attr(tree, image, "binaryItemIDRef").unwrap_or("");
        if !ctx.images.get(reference).is_some_and(truthy) {
            ctx.warn(
                format!("brush-image:{reference}"),
                format!("그림 채움 이미지({reference}) 없음 — 채움 생략"),
            );
            return Ok("none".into());
        }
        let mode = attr(tree, image_brush, "mode").unwrap_or("TOTAL");
        if mode != "TOTAL" {
            ctx.warn(
                format!("brush-mode:{mode}"),
                format!("그림 채움({mode}) 미지원 — 채움 생략"),
            );
            return Ok("none".into());
        }
        let loaded = ctx.images.get_mut(reference).unwrap();
        if truthy(&loaded["paintId"]) {
            return Ok(format!("url(#{})", string(&loaded["paintId"])));
        }
        let uri = loaded["dataUri"]
            .as_str()
            .ok_or("그림 dataUri 문자열이 필요합니다.")?;
        let id = format!("paint{}", ctx.defs.len());
        ctx.defs.push(Value::String(format!("<pattern id=\"{id}\" width=\"1\" height=\"1\" patternContentUnits=\"objectBoundingBox\"><image href=\"{}\" width=\"1\" height=\"1\" preserveAspectRatio=\"none\"/></pattern>",esc(uri))));
        loaded["paintId"] = Value::String(id.clone());
        return Ok(format!("url(#{id})"));
    }
    Ok("none".into())
}
#[derive(Clone, Debug)]
pub struct ShapeObject {
    pub tag: Value,
    pub width: Value,
    pub height: Value,
}
#[allow(clippy::too_many_arguments)]
pub fn shape_geometry(
    tree: &ParsedXml,
    el: usize,
    o: &ShapeObject,
    box_x: Value,
    box_y: Value,
    ctx: &mut ShapeContext,
    local: bool,
) -> Result<Value, String> {
    let org = child(tree, Some(el), "orgSz");
    let cur = child(tree, Some(el), "curSz");
    let ow = num(tree, org, "width", 0.0);
    let oh = num(tree, org, "height", 0.0);
    let choose = |original: f64, current: f64, fallback: &Value| {
        let first = if local { original } else { current };
        if first != 0.0 && !first.is_nan() {
            n(first)
        } else if original != 0.0 && !original.is_nan() {
            n(original)
        } else if current != 0.0 && !current.is_nan() {
            n(current)
        } else {
            fallback.clone()
        }
    };
    let w = choose(ow, num(tree, cur, "width", 0.0), &o.width);
    let h = choose(oh, num(tree, cur, "height", 0.0), &o.height);
    let mut geometry = if local {
        json!({"x":box_x,"y":box_y,"w":w,"h":h,"bbox":{"x":box_x,"y":box_y,"w":w,"h":h},"transform":""})
    } else {
        object_geometry(
            tree,
            el,
            &GeometryBox {
                x: box_x,
                y: box_y,
                w: w.clone(),
                h: h.clone(),
                box_w: Some(if truthy(&o.width) {
                    o.width.clone()
                } else {
                    w.clone()
                }),
                box_h: Some(if truthy(&o.height) {
                    o.height.clone()
                } else {
                    h.clone()
                }),
            },
        )
    };
    let x = &geometry["x"];
    let y = &geometry["y"];
    let sx = if ow > 0.0 { number(&w) / ow } else { 1.0 };
    let sy = if oh > 0.0 { number(&h) / oh } else { 1.0 };
    let line = child(tree, Some(el), "lineShape");
    let style = attr(tree, line, "style").unwrap_or("SOLID");
    let color = esc(doc_color(attr(tree, line, "color"), Some("#000000")).unwrap());
    let stroke_w = if style == "NONE" {
        0.0
    } else {
        maximum(0.2, num(tree, line, "width", 33.0) / 100.0 * 2.834645)
    };
    let dash = if style.contains("DASH") || style.contains("DOT") {
        format!(
            " stroke-dasharray=\"{}\"",
            if style.contains("DOT") {
                "1,1.5"
            } else {
                "3,1.5"
            }
        )
    } else {
        String::new()
    };
    let keep_width = if local {
        " vector-effect=\"non-scaling-stroke\""
    } else {
        ""
    };
    let stroke = if stroke_w != 0.0 && !stroke_w.is_nan() {
        format!(
            " stroke=\"{color}\" stroke-width=\"{}\"{dash}{keep_width}",
            fixed2(stroke_w)
        )
    } else {
        String::new()
    };
    let fill = format!(" fill=\"{}\"", shape_fill(tree, el, ctx)?);
    let mut svg = String::new();
    match o.tag.as_str() {
        Some("rect") => {
            svg = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{fill}{stroke}/>",
                pt(x),
                pt(y),
                pt(&w),
                pt(&h)
            )
        }
        Some("ellipse") => {
            svg = format!(
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{fill}{stroke}/>",
                pt(&add(x, &n(number(&w) / 2.0))),
                pt(&add(y, &n(number(&h) / 2.0))),
                pt(&n(number(&w) / 2.0)),
                pt(&n(number(&h) / 2.0))
            )
        }
        Some("line") => {
            let s = child(tree, Some(el), "startPt");
            let e = child(tree, Some(el), "endPt");
            svg = format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{}\"{dash}{keep_width}/>",
                pt(&add(x, &n(num(tree, s, "x", 0.0) * sx))),
                pt(&add(y, &n(num(tree, s, "y", 0.0) * sy))),
                pt(&add(x, &n(num(tree, e, "x", 0.0) * sx))),
                pt(&add(y, &n(num(tree, e, "y", 0.0) * sy))),
                fixed2(if stroke_w == 0.0 || stroke_w.is_nan() {
                    0.3
                } else {
                    stroke_w
                })
            );
        }
        Some("polygon" | "curv") => {
            let pts: Vec<_> = tree.nodes[el]
                .children
                .iter()
                .filter(|&&id| tree.nodes[id].local == "pt")
                .map(|&id| {
                    format!(
                        "{},{}",
                        pt(&add(x, &n(num(tree, Some(id), "x", 0.0) * sx))),
                        pt(&add(y, &n(num(tree, Some(id), "y", 0.0) * sy)))
                    )
                })
                .collect();
            if pts.len() >= 2 {
                svg = format!("<polygon points=\"{}\"{fill}{stroke}/>", pts.join(" "));
            }
        }
        Some("curve") => {
            let segs: Vec<_> = tree.nodes[el]
                .children
                .iter()
                .copied()
                .filter(|&id| tree.nodes[id].local == "seg")
                .collect();
            if !segs.is_empty() {
                let mut path = String::new();
                let mut last = None;
                for id in segs {
                    let x1 = num(tree, Some(id), "x1", 0.0);
                    let y1 = num(tree, Some(id), "y1", 0.0);
                    let x2 = num(tree, Some(id), "x2", 0.0);
                    let y2 = num(tree, Some(id), "y2", 0.0);
                    if last != Some((x1, y1)) {
                        path.push_str(&format!(
                            "M{} {}",
                            pt(&add(x, &n(x1 * sx))),
                            pt(&add(y, &n(y1 * sy)))
                        ));
                    }
                    path.push_str(&format!(
                        "L{} {}",
                        pt(&add(x, &n(x2 * sx))),
                        pt(&add(y, &n(y2 * sy)))
                    ));
                    last = Some((x2, y2));
                }
                svg = format!("<path d=\"{path}\"{fill}{stroke}/>");
            }
        }
        Some("arc") => {
            let stroke = if stroke.is_empty() {
                format!(" stroke=\"{color}\" stroke-width=\"0.3\"")
            } else {
                stroke
            };
            svg = format!(
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"none\"{stroke}/>",
                pt(&add(x, &n(number(&w) / 2.0))),
                pt(&add(y, &n(number(&h) / 2.0))),
                pt(&n(number(&w) / 2.0)),
                pt(&n(number(&h) / 2.0))
            );
        }
        _ => {}
    }
    geometry["svg"] = Value::String(svg);
    Ok(geometry)
}
#[cfg(test)]
fn select(tree: &ParsedXml, input: &Value) -> Result<usize, String> {
    let mut id = tree.root;
    if let Some(path) = input["path"].as_array() {
        for p in path {
            let index = p.as_u64().ok_or("XML 자식 위치 오류")? as usize;
            id = *tree.nodes[id]
                .children
                .get(index)
                .ok_or("XML 자식 위치 범위 오류")?;
        }
    }
    Ok(id)
}
#[cfg(test)]
fn context(input: &Value) -> Result<ShapeContext, String> {
    let mut ctx = ShapeContext::default();
    if let Some(defs) = input["defs"].as_array() {
        ctx.defs = defs.clone();
    }
    if let Some(images) = input["images"].as_array() {
        for pair in images {
            let key = pair[0].as_str().ok_or("이미지 키 문자열이 필요합니다.")?;
            ctx.images.insert(key.into(), pair[1].clone());
        }
    }
    Ok(ctx)
}
#[cfg(test)]
fn report_ctx(ctx: &ShapeContext) -> (Value, Value, Value) {
    (
        Value::Array(ctx.defs.clone()),
        Value::Array(ctx.images.iter().map(|(k, v)| json!([k, v])).collect()),
        Value::Array(ctx.warnings.clone()),
    )
}
#[cfg(test)]
fn shape_request(input: &Value, ctx: &mut ShapeContext) -> Result<Value, String> {
    let tree = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let el = select(&tree, input)?;
    shape_geometry(
        &tree,
        el,
        &ShapeObject {
            tag: input["tag"].clone(),
            width: input["width"].clone(),
            height: input["height"].clone(),
        },
        input["boxX"].clone(),
        input["boxY"].clone(),
        ctx,
        truthy(&input["local"]),
    )
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    match operation {
        "shapeTags" => Ok(Value::from(SHAPE_TAGS)),
        "rotatedBounds" => {
            let rotations = input["rotations"]
                .as_array()
                .ok_or("rotations 배열이 필요합니다.")?;
            if (!rotations.is_empty() && matches!(input["box"], Value::Null | Value::Undefined))
                || rotations
                    .iter()
                    .any(|r| matches!(r, Value::Null | Value::Undefined))
            {
                return Err("회전 자료의 필수 객체가 없습니다.".into());
            }
            Ok(rotated_bounds(&input["box"], rotations))
        }
        "mulAffine" => {
            if matches!(input["m"], Value::Null | Value::Undefined)
                || matches!(input["n"], Value::Null | Value::Undefined)
            {
                return Err("변환 행렬이 없습니다.".into());
            }
            Ok(mul_affine_native(&input["m"], &input["n"]))
        }
        "renderingMatrix" | "objectGeometry" => {
            let tree = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
            let el = select(&tree, input)?;
            if operation == "renderingMatrix" {
                Ok(rendering_matrix(&tree, el).map_or(Value::Null, Value::from))
            } else {
                Ok(object_geometry(
                    &tree,
                    el,
                    &GeometryBox {
                        x: input["x"].clone(),
                        y: input["y"].clone(),
                        w: input["w"].clone(),
                        h: input["h"].clone(),
                        box_w: input.get("boxW").filter(|v| !v.is_undefined()).cloned(),
                        box_h: input.get("boxH").filter(|v| !v.is_undefined()).cloned(),
                    },
                ))
            }
        }
        "shapeGeometry" => {
            let mut ctx = context(input)?;
            let geometry = shape_request(input, &mut ctx)?;
            let (defs, images, warnings) = report_ctx(&ctx);
            Ok(json!({"geometry":geometry,"defs":defs,"images":images,"warnings":warnings}))
        }
        "shapeSequence" => {
            let mut ctx = context(input)?;
            let mut geometries = Vec::new();
            for shape in input["shapes"]
                .as_array()
                .ok_or("shapes 배열이 필요합니다.")?
            {
                geometries.push(shape_request(shape, &mut ctx)?);
            }
            let (defs, images, warnings) = report_ctx(&ctx);
            Ok(json!({"geometries":geometries,"defs":defs,"images":images,"warnings":warnings}))
        }
        _ => Err(format!("알 수 없는 shape-geometry 연산: {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_decimal_uses_binary_value_then_upward_tie() {
        assert_eq!(fixed2(0.125), "0.13");
        assert_eq!(fixed2(2.675), "2.67");
        assert_eq!(fixed2(-0.0), "0.00");
        assert_eq!(fixed2(-0.001), "-0.00");
    }
    #[test]
    fn round_keeps_negative_zero_and_positive_half_direction() {
        assert_eq!(round(-0.5).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(round(-1.5), -1.0);
        assert_eq!(round(1.5), 2.0);
    }
    #[test]
    fn empty_rotation_returns_metadata_and_native_nonfinite() {
        let b = json!({"x":f64::NAN,"w":f64::INFINITY,"meta":"keep"});
        let out = rotated_bounds(&b, &[]);
        assert!(out["x"].as_f64().unwrap().is_nan());
        assert_eq!(out["w"].as_f64(), Some(f64::INFINITY));
        assert_eq!(out["meta"], "keep");
    }
}
