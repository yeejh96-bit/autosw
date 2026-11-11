//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Floating-table predicates and measured extents; see data/kordoc-LICENSE.
//! Splitting/rebasing rows is implemented separately from these read-only helpers.
use super::{
    layout::{self, maximum},
    para_model::{self as pm, Context, child, num},
    style_dom::ParsedXml,
    value::{Value, native_json as json},
};

fn n(v: &Value) -> f64 {
    super::value::number(v)
}
fn truth(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map_or(&[], Vec::as_slice)
}
fn el(v: &Value) -> Result<usize, String> {
    v["el"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| "개체 노드가 없습니다.".into())
}
fn has_ink(model: &Value, trim: bool) -> bool {
    list(model, "chars").iter().any(|c| {
        c["ch"].as_str().is_some_and(|s| {
            !s.is_empty() && (!trim || !s.trim_matches(crate::input::is_space).is_empty())
        })
    })
}
fn offset(v: f64) -> f64 {
    maximum(
        0.0,
        if v >= 2147483648.0 {
            v - 4294967296.0
        } else {
            v
        },
    )
}
pub fn square_table_over(
    d: &ParsedXml,
    p: usize,
    start_v: f64,
    limit: f64,
    with_text: bool,
    area_w: f64,
    ctx: &mut Context,
) -> Result<bool, String> {
    let m = pm::build_para(d, p, ctx)?;
    if !with_text && has_ink(&m, false) {
        return Ok(false);
    }
    for o in list(&m, "objs") {
        let i = el(o)?;
        if truth(&o["inline"])
            || o["tag"] != "tbl"
            || d.nodes[i].attr("textWrap") != Some("SQUARE")
            || !matches!(d.nodes[i].attr("pageBreak"), Some("CELL" | "TABLE"))
        {
            continue;
        }
        let pos = child(d, i, "pos");
        let om = child(d, i, "outMargin");
        if pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA" {
            continue;
        }
        if with_text
            && maximum(0.0, num(d, pos, "horzOffset", 0.0))
                + num(d, om, "left", 0.0)
                + n(&o["width"])
                + num(d, om, "right", 0.0)
                < area_w - 1440.0
        {
            continue;
        }
        let actual = pm::without_extent_memo(ctx, |ctx| pm::measure_table_height(d, i, ctx))?;
        let height = if with_text {
            actual
        } else {
            maximum(n(&o["height"]), actual)
        };
        if start_v
            + offset(num(d, pos, "vertOffset", 0.0))
            + num(d, om, "top", 0.0)
            + height
            + num(d, om, "bottom", 0.0)
            > limit
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn bullet_empty_host(
    d: &ParsedXml,
    p: usize,
    geom: &Value,
    ctx: &mut Context,
) -> Result<bool, String> {
    if !truth(&geom["bullet"]) {
        return Ok(false);
    }
    let m = pm::build_para(d, p, ctx)?;
    if list(&m, "objs").is_empty() || has_ink(&m, true) {
        return Ok(false);
    }
    for o in list(&m, "objs") {
        if truth(&o["inline"])
            || o["tag"] != "tbl"
            || d.nodes[el(o)?].attr("textWrap") != Some("TOP_AND_BOTTOM")
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn has_tb_float(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<bool, String> {
    let m = pm::build_para(d, p, ctx)?;
    for o in list(&m, "objs") {
        let i = el(o)?;
        if !truth(&o["inline"])
            && d.nodes[i].attr("textWrap") == Some("TOP_AND_BOTTOM")
            && pm::attr(d, child(d, i, "pos"), "vertRelTo").unwrap_or("PARA") == "PARA"
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn m0_objs(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<Option<usize>, String> {
    let m = pm::build_para(d, p, ctx)?;
    for o in list(&m, "objs") {
        if !truth(&o["inline"]) && o["tag"] == "tbl" {
            return Ok(Some(el(o)?));
        }
    }
    Ok(None)
}
pub fn content_table_height(d: &ParsedXml, tbl: usize, ctx: &mut Context) -> Result<f64, String> {
    let cells = pm::collect_cells(d, tbl);
    if cells.is_empty() {
        return Ok(0.0);
    }
    let count = cells
        .iter()
        .map(|c| n(&c["ra"]) + n(&c["rs"]))
        .fold(f64::NEG_INFINITY, maximum);
    if !count.is_finite() || count < 0.0 || count.fract() != 0.0 {
        return Err("Invalid array length".into());
    }
    if count > 1_000_000.0 {
        return Err("표 행 계산 한도를 넘었습니다.".into());
    }
    let mut rows = Vec::new();
    for c in &cells {
        let single = n(&c["rs"]) == 1.0;
        let h = pm::without_extent_memo(ctx, |ctx| pm::cell_content_extent(d, c, ctx))?
            + n(&c["marginT"])
            + n(&c["marginB"]);
        rows.push(layout::CellRow {
            row_addr: n(&c["ra"]),
            row_span: n(&c["rs"]),
            height: if single { h } else { 0.0 },
            content_h: if single { None } else { Some(h) },
        })
    }
    Ok(
        layout::solve_row_heights(&rows, count as usize, f64::INFINITY, false)
            .into_iter()
            .fold(0.0, |a, b| a + b),
    )
}
pub fn band_table_over(
    d: &ParsedXml,
    p: usize,
    top: f64,
    limit: f64,
    ctx: &mut Context,
) -> Result<bool, String> {
    let m = pm::build_para(d, p, ctx)?;
    if !has_ink(&m, true) {
        return Ok(false);
    }
    for o in list(&m, "objs") {
        let i = el(o)?;
        if truth(&o["inline"])
            || o["tag"] != "tbl"
            || d.nodes[i].attr("textWrap") != Some("TOP_AND_BOTTOM")
            || !matches!(d.nodes[i].attr("pageBreak"), Some("CELL" | "TABLE"))
        {
            continue;
        }
        let pos = child(d, i, "pos");
        let om = child(d, i, "outMargin");
        if pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA" {
            continue;
        }
        let vo = num(d, pos, "vertOffset", 0.0);
        if vo <= 0.0 || vo >= 2147483648.0 {
            continue;
        }
        if top
            + vo
            + num(d, om, "top", 0.0)
            + content_table_height(d, i, ctx)?
            + num(d, om, "bottom", 0.0)
            > limit
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn spaces_host(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<bool, String> {
    let m = pm::build_para(d, p, ctx)?;
    Ok(list(&m, "chars").iter().any(|c| c["ch"] == " ")
        && list(&m, "chars")
            .iter()
            .all(|c| c["ch"] == "" || c["ch"] == " ")
        && !list(&m, "objs").iter().any(|o| truth(&o["inline"])))
}
pub fn sub_float_bottoms(
    d: &ParsedXml,
    sub: usize,
    ctx: &mut Context,
) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        let segs: Vec<_> = child(d, p, "linesegarray").map_or_else(Vec::new, |lsa| {
            d.nodes[lsa]
                .children
                .iter()
                .copied()
                .filter(|&s| d.nodes[s].local == "lineseg")
                .collect()
        });
        let Some(&seg) = segs.first() else { continue };
        let last = *segs.last().unwrap();
        let host_end = num(d, Some(last), "vertpos", 0.0)
            + num(d, Some(last), "vertsize", 0.0)
            + num(d, Some(last), "spacing", 0.0);
        for &run in &d.nodes[p].children {
            if d.nodes[run].local != "run" {
                continue;
            }
            for &i in &d.nodes[run].children {
                let tag = d.nodes[i].local.as_str();
                if !pm::OBJ_TAGS.contains(&tag)
                    || d.nodes[i].attr("textWrap") != Some("TOP_AND_BOTTOM")
                {
                    continue;
                }
                let pos = child(d, i, "pos");
                if pm::attr(d, pos, "treatAsChar") == Some("1")
                    || pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA"
                {
                    continue;
                }
                let om = child(d, i, "outMargin");
                let sz = child(d, i, "sz");
                let h = if tag == "tbl" {
                    maximum(
                        num(d, sz, "height", 0.0),
                        pm::without_extent_memo(ctx, |ctx| pm::measure_table_height(d, i, ctx))?,
                    )
                } else {
                    num(d, sz, "height", 0.0)
                } + pm::caption_extra(d, i);
                let v0 = num(d, Some(seg), "vertpos", 0.0);
                let carry = num(d, om, "top", 0.0) + h + num(d, om, "bottom", 0.0);
                let vo = offset(num(d, pos, "vertOffset", 0.0));
                let spaces = spaces_host(d, p, ctx)?;
                let top = if spaces { v0 - vo - carry } else { v0 };
                let mut item = json!({"v0":top,"bottom":if spaces{v0}else{v0+vo+carry},"carry":carry,"hostEnd":if spaces{top}else{host_end},"lineBottom":top+num(d,Some(seg),"vertsize",0.0)});
                if tag == "tbl" && matches!(d.nodes[i].attr("pageBreak"), Some("CELL" | "TABLE")) {
                    item["rows"] = pm::without_extent_memo(ctx, |ctx| {
                        pm::measure_table_row_heights(d, i, ctx)
                    })?
                    .into();
                    item["outT"] = num(d, om, "top", 0.0).into();
                    item["outB"] = num(d, om, "bottom", 0.0).into();
                    item["el"] = i.into();
                }
                item["seg"] = seg.into();
                out.push(item);
            }
        }
    }
    Ok(out)
}
pub fn page_bottom_reserve(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<f64, String> {
    let mut r = 0.0;
    for &run in &d.nodes[p].children {
        if d.nodes[run].local != "run" {
            continue;
        }
        for &i in &d.nodes[run].children {
            let tag = d.nodes[i].local.as_str();
            if !pm::OBJ_TAGS.contains(&tag) || d.nodes[i].attr("textWrap") != Some("TOP_AND_BOTTOM")
            {
                continue;
            }
            let pos = child(d, i, "pos");
            if pm::attr(d, pos, "treatAsChar") == Some("1")
                || pm::attr(d, pos, "vertRelTo") != Some("PAGE")
                || pm::attr(d, pos, "vertAlign") != Some("BOTTOM")
            {
                continue;
            }
            let om = child(d, i, "outMargin");
            let sz = child(d, i, "sz");
            let h = if tag == "tbl" {
                maximum(
                    num(d, sz, "height", 0.0),
                    pm::without_extent_memo(ctx, |ctx| pm::measure_table_height(d, i, ctx))?,
                )
            } else {
                num(d, sz, "height", 0.0)
            };
            r = maximum(
                r,
                num(d, pos, "vertOffset", 0.0)
                    + num(d, om, "top", 0.0)
                    + h
                    + num(d, om, "bottom", 0.0),
            );
        }
    }
    Ok(r)
}
pub fn column_width_in(d: &ParsedXml, p: usize, body_w: f64) -> Value {
    for &run in &d.nodes[p].children {
        if d.nodes[run].local != "run" {
            continue;
        }
        for &i in &d.nodes[run].children {
            if d.nodes[i].local != "ctrl" {
                continue;
            }
            let Some(col) = child(d, i, "colPr") else {
                continue;
            };
            let count = maximum(1.0, num(d, Some(col), "colCount", 1.0));
            if count == 1.0 {
                return body_w.into();
            }
            if d.nodes[col].attr("sameSz") != Some("0") {
                return ((body_w - num(d, Some(col), "sameGap", 0.0) * (count - 1.0)) / count)
                    .trunc()
                    .into();
            }
            return child(d, col, "colSz")
                .map_or(body_w, |first| num(d, Some(first), "width", body_w))
                .into();
        }
    }
    Value::Undefined
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let xml = input["xml"].as_str().ok_or("xml이 필요합니다.")?;
    let d = super::style_dom::parse_style_xml(xml)?;
    let mut target = d.root;
    if let Some(path) = input["path"].as_array() {
        for index in path {
            target = *d.nodes[target]
                .children
                .get(index.as_u64().ok_or("path 번호 오류")? as usize)
                .ok_or("path 범위 오류")?;
        }
    }
    let i = Value::from_json(input);
    let mut ctx = Context::default();
    if truth(&i["prepare"]) {
        pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx)
    }
    let result = match op {
        "squareTableOver" => square_table_over(
            &d,
            target,
            n(&i["startV"]),
            n(&i["lim"]),
            truth(&i["withText"]),
            i.get("areaW").map_or(0.0, n),
            &mut ctx,
        )?
        .into(),
        "bulletEmptyHost" => bullet_empty_host(&d, target, &i["geom"], &mut ctx)?.into(),
        "hasTbFloat" => has_tb_float(&d, target, &mut ctx)?.into(),
        "m0Objs" => m0_objs(&d, target, &mut ctx)?.map_or(Value::Undefined, Value::from),
        "bandTableOver" => {
            band_table_over(&d, target, n(&i["top"]), n(&i["lim"]), &mut ctx)?.into()
        }
        "spacesHost" => spaces_host(&d, target, &mut ctx)?.into(),
        "subFloatBottoms" => sub_float_bottoms(&d, target, &mut ctx)?.into(),
        "pageBottomReserve" => page_bottom_reserve(&d, target, &mut ctx)?.into(),
        "columnWidthIn" => column_width_in(&d, target, n(&i["bodyW"])),
        _ => return Err("알 수 없는 표 배치 보조 연산입니다.".into()),
    };
    Ok(json!({"defined":!matches!(result,Value::Undefined),"result":result}).to_json())
}
