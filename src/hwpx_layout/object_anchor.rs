//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/object-anchor.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
use super::layout::maximum;
use super::para_model::{Context, attr, child, measure_table_height, num};
use super::style_dom::ParsedXml;
use super::value::{Map, NativeValue as Value, native_json as json, number, string};
#[cfg(test)]
use super::{para_model::build_para, style_dom::parse_style_xml};

fn n(value: f64) -> Value {
    Value::Number(value)
}
fn truth(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(v) => *v != 0.0 && !v.is_nan(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn primitive(value: &Value) -> Value {
    match value {
        Value::Array(_) | Value::Object(_) => Value::String(string(value)),
        _ => value.clone(),
    }
}
fn add(a: &Value, b: &Value) -> Value {
    let a = primitive(a);
    let b = primitive(b);
    if a.is_string() || b.is_string() {
        Value::String(format!("{}{}", string(&a), string(&b)))
    } else {
        n(number(&a) + number(&b))
    }
}
fn plus(a: &Value, b: f64) -> Value {
    add(a, &n(b))
}
fn nullish(value: &Value) -> bool {
    matches!(value, Value::Undefined | Value::Null)
}
fn greater(a: &Value, b: &Value) -> bool {
    let a = primitive(a);
    let b = primitive(b);
    if let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
        a.encode_utf16().cmp(b.encode_utf16()).is_gt()
    } else {
        number(&a) > number(&b)
    }
}
fn node_id(d: &ParsedXml, o: &Value) -> Result<usize, String> {
    o["el"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < d.nodes.len())
        .ok_or_else(|| "객체 XML 노드가 없습니다.".into())
}
fn signed_offset(raw: f64) -> f64 {
    if raw >= 2147483648.0 {
        raw - 4294967296.0
    } else {
        raw
    }
}
fn height(d: &ParsedXml, o: &Value, el: usize, extent: &mut Context) -> Result<Value, String> {
    if o["tag"] == "tbl" {
        Ok(n(maximum(
            number(&o["height"]),
            measure_table_height(d, el, extent)?,
        )))
    } else {
        Ok(o["height"].clone())
    }
}

pub fn line_pushed_by(
    d: &ParsedXml,
    o: &Value,
    line_v: &Value,
    para_top: &Value,
) -> Result<bool, String> {
    if para_top.is_undefined() {
        return Ok(false);
    }
    let el = node_id(d, o)?;
    let om = child(d, el, "outMargin");
    let pos = child(d, el, "pos");
    let raw = num(d, pos, "vertOffset", 0.0);
    let vo = if attr(d, pos, "vertRelTo").unwrap_or("PARA") == "PARA" {
        maximum(0.0, signed_offset(raw))
    } else {
        0.0
    };
    let end = n(vo + num(d, om, "top", 0.0));
    let end = plus(&add(&end, &o["height"]), num(d, om, "bottom", 0.0));
    Ok((number(line_v) - number(&end) - number(para_top)).abs() <= 10.0)
}

#[derive(Clone, Debug)]
pub struct AnchorArgs {
    pub ox: Value,
    pub oy: Value,
    pub base_v: Value,
    pub area_w: Value,
    pub anchor_line_v: Value,
    pub stay_line: Value,
    pub para_start_v: Value,
    pub line_h: Value,
    pub line_sb: Value,
    pub anchor_at: Value,
}
impl Default for AnchorArgs {
    fn default() -> Self {
        Self {
            ox: Value::Undefined,
            oy: Value::Undefined,
            base_v: Value::Undefined,
            area_w: Value::Undefined,
            anchor_line_v: Value::Undefined,
            stay_line: Value::Bool(false),
            para_start_v: Value::Undefined,
            line_h: Value::Undefined,
            line_sb: n(0.0),
            anchor_at: Value::String("end".into()),
        }
    }
}
impl AnchorArgs {
    pub fn from_value(v: &Value) -> Self {
        let defaults = Self::default();
        Self {
            ox: v["ox"].clone(),
            oy: v["oy"].clone(),
            base_v: v["baseV"].clone(),
            area_w: v["areaW"].clone(),
            anchor_line_v: v["anchorLineV"].clone(),
            stay_line: if v["stayLine"].is_undefined() {
                defaults.stay_line
            } else {
                v["stayLine"].clone()
            },
            para_start_v: v["paraStartV"].clone(),
            line_h: v["lineH"].clone(),
            line_sb: if v["lineSB"].is_undefined() {
                defaults.line_sb
            } else {
                v["lineSB"].clone()
            },
            anchor_at: if v["anchorAt"].is_undefined() {
                defaults.anchor_at
            } else {
                v["anchorAt"].clone()
            },
        }
    }
}
pub type BodyDx<'a> = dyn FnMut(&Value) -> Result<Value, String> + 'a;
pub struct AnchorContext<'a> {
    pub geom: &'a Value,
    pub page: &'a Value,
    pub extent: &'a mut Context,
    pub body_dx: Option<&'a mut BodyDx<'a>>,
}

pub fn anchor_object(
    d: &ParsedXml,
    o: &Value,
    args: &AnchorArgs,
    ctx: &mut AnchorContext<'_>,
) -> Result<Value, String> {
    if nullish(ctx.geom) {
        return Err("쪽 배치 기준이 없습니다.".into());
    }
    let g = ctx.geom;
    let el = node_id(d, o)?;
    let pos = child(d, el, "pos");
    let om = child(d, el, "outMargin");
    let om_t = num(d, om, "top", 0.0);
    let om_b = num(d, om, "bottom", 0.0);
    let w = &o["width"];
    let h = height(d, o, el, ctx.extent)?;
    if pos.is_none() {
        return Ok(json!({"x":args.ox,"y":add(&args.oy,&args.base_v)}));
    }
    let vo_s = signed_offset(num(d, pos, "vertOffset", 0.0));
    let vrel = attr(d, pos, "vertRelTo").unwrap_or("PARA");
    let hrel = attr(d, pos, "horzRelTo").unwrap_or("PARA");
    let vo = if vrel == "PARA" {
        maximum(0.0, vo_s)
    } else {
        vo_s
    };
    let ho = num(d, pos, "horzOffset", 0.0);
    let va = attr(d, pos, "vertAlign").unwrap_or("TOP");
    let ha = attr(d, pos, "horzAlign").unwrap_or("LEFT");
    let wrap = d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM");
    let y = if vrel == "PAPER" {
        if va == "BOTTOM" {
            n(number(&g["PH"]) - number(&h) - vo - om_b)
        } else if va == "CENTER" {
            n((number(&g["PH"]) - number(&h)) / 2.0 + vo)
        } else {
            n(vo + om_t)
        }
    } else if vrel == "PAGE" {
        if va == "BOTTOM" {
            n(number(&add(&g["MT"], &g["BODY_H"])) - number(&h) - vo)
        } else if va == "CENTER" {
            plus(
                &plus(&g["MT"], (number(&g["BODY_H"]) - number(&h)) / 2.0),
                vo,
            )
        } else {
            plus(&g["MT"], vo)
        }
    } else if wrap == "TOP_AND_BOTTOM" {
        let line_v = if nullish(&args.anchor_line_v) {
            &args.base_v
        } else {
            &args.anchor_line_v
        };
        let pushed = number(line_v) - number(&plus(&add(&n(vo + om_t), &h), om_b));
        // Keep !(x <= 10), including NaN; replacing it with x > 10 differs.
        let pushed_from_start = !args.para_start_v.is_undefined()
            && (pushed - number(&args.para_start_v)).abs() <= 10.0;
        let below =
            !args.line_h.is_undefined() && vo + om_t >= number(&args.line_h) && !pushed_from_start;
        let anchor = if below {
            n(number(line_v) - number(&args.line_sb))
        } else if args.stay_line != Value::Bool(false) {
            n(number(line_v) - number(&args.stay_line))
        } else if pushed >= -100.0 {
            n(pushed)
        } else if !args.para_start_v.is_undefined() {
            args.para_start_v.clone()
        } else {
            line_v.clone()
        };
        plus(&plus(&add(&args.oy, &anchor), om_t), vo)
    } else {
        let origin = if vrel == "PARA" && wrap == "SQUARE" && !nullish(&args.para_start_v) {
            &args.para_start_v
        } else {
            &args.base_v
        };
        let later = vrel == "PARA"
            && wrap == "SQUARE"
            && args.anchor_at == "end"
            && !args.anchor_line_v.is_undefined()
            && greater(&args.anchor_line_v, &args.base_v);
        let mid_later = vrel == "PARA"
            && wrap == "SQUARE"
            && args.anchor_at == "top"
            && !args.anchor_line_v.is_undefined()
            && greater(&args.anchor_line_v, &args.base_v);
        if later {
            plus(&plus(&add(&args.oy, &args.anchor_line_v), vo), om_t)
        } else {
            let anchor = if wrap == "SQUARE" && !mid_later && !args.anchor_line_v.is_undefined() {
                number(&args.anchor_line_v)
            } else {
                f64::NEG_INFINITY
            };
            plus(
                &plus(&args.oy, maximum(number(&plus(origin, vo)), anchor)),
                om_t,
            )
        }
    };
    let om_l = num(d, om, "left", 0.0);
    let om_r = num(d, om, "right", 0.0);
    let x = if hrel == "PAGE" {
        if ha == "RIGHT" {
            n(number(&add(&g["ML"], &g["BODY_W"])) - number(w) - ho - om_r)
        } else if ha == "CENTER" {
            plus(
                &plus(&g["ML"], (number(&g["BODY_W"]) - number(w)) / 2.0),
                ho,
            )
        } else {
            plus(&plus(&g["ML"], ho), om_l)
        }
    } else if hrel == "PAPER" {
        let x = if ha == "RIGHT" {
            number(&g["PW"]) - number(w) - ho - om_r
        } else if ha == "CENTER" {
            (number(&g["PW"]) - number(w)) / 2.0 + ho
        } else {
            ho + om_l
        };
        let dx = if let Some(callback) = ctx.body_dx.as_deref_mut() {
            callback(ctx.page)?
        } else {
            Value::Undefined
        };
        n(x - if nullish(&dx) { 0.0 } else { number(&dx) })
    } else if ha == "RIGHT" {
        n(number(&add(&args.ox, &args.area_w)) - number(w) - ho - om_r)
    } else if ha == "CENTER" {
        plus(
            &plus(&args.ox, (number(&args.area_w) - number(w)) / 2.0),
            ho,
        )
    } else {
        plus(&plus(&args.ox, ho), om_l)
    };
    Ok(json!({"x":x,"y":y}))
}
struct Placed {
    x0: f64,
    x1: Value,
    bottom: Value,
}
pub fn square_stack_tops(
    d: &ParsedXml,
    objects: &[Value],
    extent: &mut Context,
) -> Result<Map<usize, Value>, String> {
    let mut out = Map::new();
    let mut square = Vec::new();
    for o in objects {
        let el = node_id(d, o)?;
        let pos = child(d, el, "pos");
        if !truth(&o["inline"])
            && d.nodes[el].attr("textWrap") == Some("SQUARE")
            && attr(d, pos, "allowOverlap") == Some("0")
            && attr(d, pos, "vertRelTo").unwrap_or("PARA") == "PARA"
        {
            square.push((el, o));
        }
    }
    square.sort_by(|(_, a), (_, b)| {
        (number(&a["index"]) - number(&b["index"]))
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if square.len() < 2 {
        return Ok(out);
    }
    let mut placed: Vec<Placed> = Vec::new();
    for (el, o) in square {
        let pos = child(d, el, "pos");
        let om = child(d, el, "outMargin");
        let raw = num(d, pos, "vertOffset", 0.0);
        let x0 = num(d, pos, "horzOffset", 0.0);
        let x1 = plus(
            &add(&n(x0 + num(d, om, "left", 0.0)), &o["width"]),
            num(d, om, "right", 0.0),
        );
        let own = maximum(0.0, signed_offset(raw));
        let below = placed
            .iter()
            .filter(|r| r.x0 < number(&x1) && x0 < number(&r.x1))
            .fold(0.0, |v, r| maximum(v, number(&r.bottom)));
        let top = maximum(below, own);
        if below > own {
            out.insert(el, n(top));
        }
        let bottom = plus(
            &add(&n(top + num(d, om, "top", 0.0)), &height(d, o, el, extent)?),
            num(d, om, "bottom", 0.0),
        );
        placed.push(Placed { x0, x1, bottom });
    }
    Ok(out)
}

#[cfg(test)]
fn select(d: &ParsedXml, v: &Value) -> Result<usize, String> {
    if !v["el"].is_undefined() {
        return node_id(d, v);
    }
    let mut el = d.root;
    if let Some(path) = v["path"].as_array() {
        for at in path {
            el = *d.nodes[el]
                .children
                .get(at.as_u64().ok_or("XML 자식 위치 오류")? as usize)
                .ok_or("XML 자식 위치 범위 오류")?;
        }
    }
    Ok(el)
}
#[cfg(test)]
fn object(d: &ParsedXml, input: &Value) -> Result<Value, String> {
    let mut out = if input["object"].is_object() {
        input["object"].clone()
    } else {
        json!({})
    };
    let el = if !out["el"].is_undefined() || out["path"].is_array() {
        select(d, &out)?
    } else {
        select(d, input)?
    };
    out["el"] = Value::from(el);
    Ok(out)
}
#[cfg(test)]
fn memo_report(d: &ParsedXml, ctx: &Context) -> (Value, Value) {
    let table = (0..d.nodes.len())
        .filter_map(|id| ctx.table_memo.get(&id).map(|v| json!([id, *v])))
        .collect::<Vec<_>>();
    let cell = (0..d.nodes.len())
        .filter_map(|id| ctx.cell_memo.get(&id).map(|v| json!([id, *v])))
        .collect::<Vec<_>>();
    (Value::from(table), Value::from(cell))
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    let d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let mut extent = Context {
        memoize: truth(&input["memo"]),
        ..Context::default()
    };
    if let Some(entries) = input["paraGeom"].as_array() {
        for entry in entries {
            if let Some(key) = entry[0].as_str() {
                extent.para_geom.insert(key.into(), entry[1].clone());
            }
        }
    }
    if truth(&input["prepare"]) {
        super::para_model::prepare_deleted_ranges(&d, d.root, 0, &mut extent);
    }
    match operation {
        "linePushedBy" => Ok(Value::Bool(line_pushed_by(
            &d,
            &object(&d, input)?,
            &input["lineV"],
            &input["paraTop"],
        )?)),
        "squareStackTops" => {
            let objects = if truth(&input["fromPara"]) {
                build_para(&d, select(&d, input)?, &mut extent)?["objs"]
                    .as_array()
                    .ok_or("문단 객체 배열이 없습니다.")?
                    .clone()
            } else {
                input["objects"]
                    .as_array()
                    .ok_or("objects 배열이 필요합니다.")?
                    .iter()
                    .map(|o| {
                        let mut out = o.clone();
                        let id = select(&d, o)?;
                        out["el"] = Value::from(id);
                        Ok(out)
                    })
                    .collect::<Result<Vec<_>, String>>()?
            };
            let tops = square_stack_tops(&d, &objects, &mut extent)?
                .into_iter()
                .map(|(id, v)| json!([id, v]))
                .collect::<Vec<_>>();
            let (table_memo, cell_memo) = memo_report(&d, &extent);
            Ok(json!({"tops":tops,"tableMemo":table_memo,"cellMemo":cell_memo}))
        }
        "anchorObject" | "anchorSequence" => {
            let mut calls = Vec::new();
            let callback_present = input.get("bodyDxResults").is_some();
            let values = input["bodyDxResults"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let throw_at = input["bodyDxThrowAt"].as_u64();
            let mut callback = |page: &Value| {
                let at = calls.len();
                calls.push(page.clone());
                if throw_at == Some(at as u64) {
                    Err("synthetic bodyDx failure".into())
                } else {
                    Ok(values.get(at).cloned().unwrap_or(Value::Undefined))
                }
            };
            let mut one = |request: &Value| {
                let o = object(&d, request)?;
                let page = request.get("page").unwrap_or(&input["page"]);
                let geom = request.get("geom").unwrap_or(&input["geom"]);
                let mut context = AnchorContext {
                    geom,
                    page,
                    extent: &mut extent,
                    body_dx: if callback_present {
                        Some(&mut callback)
                    } else {
                        None
                    },
                };
                anchor_object(&d, &o, &AnchorArgs::from_value(request), &mut context)
            };
            let output = if operation == "anchorObject" {
                one(input)?
            } else {
                let mut results = Vec::new();
                for request in input["calls"]
                    .as_array()
                    .ok_or("calls 배열이 필요합니다.")?
                {
                    results.push(match one(request) {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(error) => json!({"ok":false,"error":error}),
                    });
                }
                Value::from(results)
            };
            let (table_memo, cell_memo) = memo_report(&d, &extent);
            if operation == "anchorObject" {
                Ok(
                    json!({"anchor":output,"calls":calls,"tableMemo":table_memo,"cellMemo":cell_memo}),
                )
            } else {
                Ok(
                    json!({"results":output,"calls":calls,"tableMemo":table_memo,"cellMemo":cell_memo}),
                )
            }
        }
        _ => Err(format!("알 수 없는 object-anchor 연산: {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undefined_paragraph_top_returns_before_object_access() {
        let d = parse_style_xml("<root/>").unwrap();
        assert!(!line_pushed_by(&d, &Value::Undefined, &n(0.0), &Value::Undefined).unwrap());
    }
    #[test]
    fn missing_numbers_remain_nan_before_reporting() {
        let d = parse_style_xml("<rect><pos textWrap=\"SQUARE\"/></rect>").unwrap();
        let mut extent = Context::default();
        let geom = json!({});
        let page = n(1.0);
        let mut ctx = AnchorContext {
            geom: &geom,
            page: &page,
            extent: &mut extent,
            body_dx: None,
        };
        let out = anchor_object(
            &d,
            &json!({"el":0,"width":1,"height":2}),
            &AnchorArgs::default(),
            &mut ctx,
        )
        .unwrap();
        assert!(out["x"].as_f64().unwrap().is_nan());
        assert!(out["y"].as_f64().unwrap().is_nan());
    }
}
