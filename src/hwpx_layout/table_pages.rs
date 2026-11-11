//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/table-pages.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
use super::layout::{maximum as max, minimum as min};
use super::para_model::{self as pm, Context, attr, child, num};
use super::style_dom::ParsedXml;
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::table_flow::sub_float_bottoms;
use super::value::{Map, NativeValue as Value, native_json as json, number};

/// The former unbounded loops stop with an error; a partial plan is never returned.
pub const MAX_PLAN_STEPS: usize = 100_000;
const LIMIT_ERROR: &str = "표 쪽 나눔 처리 한도(100000회)를 넘었습니다.";
#[derive(Default)]
struct Budget(usize);
impl Budget {
    fn tick(&mut self) -> Result<(), String> {
        self.0 += 1;
        if self.0 > MAX_PLAN_STEPS {
            Err(LIMIT_ERROR.into())
        } else {
            Ok(())
        }
    }
    fn push(&mut self, out: &mut Vec<RowPiece>, piece: RowPiece) -> Result<(), String> {
        self.tick()?;
        out.push(piece);
        Ok(())
    }
}
fn n(c: &Value, key: &str) -> f64 {
    number(&c[key])
}
fn id(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|v| usize::try_from(v).ok())
}
fn el(c: &Value) -> Result<usize, String> {
    id(&c["el"]).ok_or_else(|| "셀 노드가 없습니다.".into())
}
fn at(values: &[f64], i: f64) -> f64 {
    if i >= 0.0 && i.fract() == 0.0 && i < (values.len() as f64) {
        values[i as usize]
    } else {
        f64::NAN
    }
}
#[cfg(test)]
fn truth(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(v) => *v != 0.0 && !v.is_nan(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn strict(c: &Value, key: &str, v: f64) -> bool {
    c[key].as_f64() == Some(v)
}
fn max_values(values: impl Iterator<Item = f64>, initial: f64) -> f64 {
    values.fold(initial, max)
}
fn minimum(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::INFINITY, min)
}
fn offset(raw: f64) -> f64 {
    max(
        0.0,
        if raw >= 2147483648.0 {
            raw - 4294967296.0
        } else {
            raw
        },
    )
}

#[derive(Clone, Debug)]
pub struct CellLineBox {
    pub seg: usize,
    pub top: f64,
    pub bottom: f64,
    pub part: usize,
    pub origin: f64,
}
impl CellLineBox {
    pub fn to_value(&self) -> Value {
        json!({"seg":self.seg,"top":self.top,"bottom":self.bottom,"part":self.part,"origin":self.origin})
    }
}
#[derive(Clone, Debug)]
pub struct RowPiece {
    pub page: f64,
    pub y: f64,
    pub h: f64,
    pub range: Option<Map<usize, Value>>,
    pub place: Option<Map<usize, Value>>,
    pub fill: Option<bool>,
}
impl RowPiece {
    fn new(page: f64, y: f64, h: f64) -> Self {
        Self {
            page,
            y,
            h,
            range: None,
            place: None,
            fill: None,
        }
    }
    fn range(mut self, range: Map<usize, Value>) -> Self {
        self.range = Some(range);
        self
    }
    fn place(mut self, place: Map<usize, Value>) -> Self {
        self.place = Some(place);
        self
    }
    fn fill(mut self) -> Self {
        self.fill = Some(true);
        self
    }
    pub fn to_value(&self) -> Value {
        let mut out = json!({"page":self.page,"y":self.y,"h":self.h});
        if let Some(map) = &self.range {
            out["range"] = map.iter().map(|(id, v)| json!([*id, v])).collect();
        }
        if let Some(map) = &self.place {
            out["place"] = map.iter().map(|(id, v)| json!([*id, v])).collect();
        }
        if let Some(fill) = self.fill {
            out["fill"] = Value::Bool(fill);
        }
        out
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FloatSpan {
    pub el: Option<usize>,
    pub pages: f64,
    pub end_y: f64,
}
pub type FloatSpanCallback<'a> =
    dyn FnMut(usize, f64, f64, &mut Context) -> Result<Option<FloatSpan>, String> + 'a;
#[derive(Clone, Copy, Debug)]
pub struct PlanOptions {
    pub ty: f64,
    pub mt: f64,
    pub body_h: f64,
    pub slack: f64,
    pub nested: bool,
    pub cont_mt: f64,
}
#[derive(Clone, Copy)]
struct Float {
    el: usize,
    vo: f64,
    h: f64,
    om_t: f64,
}
fn stacked_floats(d: &ParsedXml, c: &Value) -> Vec<Float> {
    let mut out = Vec::new();
    let Some(sub) = id(&c["sub"]) else {
        return out;
    };
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        for &run in &d.nodes[p].children {
            if d.nodes[run].local != "run" {
                continue;
            }
            for &o in &d.nodes[run].children {
                if d.nodes[o].local != "pic" {
                    continue;
                }
                let pos = child(d, o, "pos");
                if pos.is_none()
                    || attr(d, pos, "treatAsChar") == Some("1")
                    || attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA"
                    || attr(d, pos, "allowOverlap") != Some("0")
                {
                    continue;
                }
                if !matches!(
                    d.nodes[o].attr("textWrap").unwrap_or("TOP_AND_BOTTOM"),
                    "SQUARE" | "TOP_AND_BOTTOM"
                ) {
                    continue;
                }
                let om = child(d, o, "outMargin");
                out.push(Float {
                    el: o,
                    vo: offset(num(d, pos, "vertOffset", 0.0)),
                    h: num(d, child(d, o, "sz"), "height", 0.0)
                        + num(d, om, "top", 0.0)
                        + num(d, om, "bottom", 0.0),
                    om_t: num(d, om, "top", 0.0),
                });
            }
        }
    }
    out
}
pub fn cell_line_boxes(
    d: &ParsedXml,
    c: &Value,
    body_h: Option<f64>,
    ctx: &mut Context,
) -> Result<Vec<CellLineBox>, String> {
    let mut out = Vec::new();
    let mut budget = Budget::default();
    let mut shift = 0.0;
    let mut prev_v = f64::NEG_INFINITY;
    let mut prev_h = 0.0;
    let mut prev_saved_end = 0.0;
    let mut prev_end = 0.0;
    let mut part = 0;
    let sub = id(&c["sub"]);
    let cols = pm::cell_columns(d, sub).is_some();
    let mut float_bottom = f64::NEG_INFINITY;
    let mut nested = false;
    let mut ancestor = d.nodes[el(c)?].parent;
    while let Some(a) = ancestor {
        if d.nodes[a].local == "tc" {
            nested = true;
            break;
        }
        ancestor = d.nodes[a].parent;
    }
    let Some(sub) = sub else {
        return Ok(out);
    };
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        let mut first = true;
        let mut p_top = 0.0;
        if let Some(lsa) = child(d, p, "linesegarray") {
            for &sg in &d.nodes[lsa].children {
                if d.nodes[sg].local != "lineseg" {
                    continue;
                }
                budget.tick()?;
                let v = num(d, Some(sg), "vertpos", 0.0);
                let hz = num(d, Some(sg), "horzpos", 0.0);
                if ((v < prev_v || v == prev_v && hz <= prev_h && prev_saved_end > v) && !cols)
                    || (!nested
                        && body_h.is_some_and(|bh| float_bottom > bh)
                        && first
                        && !cols
                        && v >= prev_v
                        && v < float_bottom - 1000.0)
                {
                    shift = prev_end - v;
                    part += 1;
                }
                if first {
                    p_top = if prev_saved_end > 0.0 && prev_saved_end <= v {
                        prev_saved_end
                    } else {
                        v
                    };
                }
                first = false;
                prev_v = v;
                prev_h = hz;
                prev_saved_end =
                    v + num(d, Some(sg), "vertsize", 0.0) + num(d, Some(sg), "spacing", 0.0);
                let top = v + shift;
                let bottom = top + num(d, Some(sg), "vertsize", 0.0);
                prev_end = bottom + num(d, Some(sg), "spacing", 0.0);
                out.push(CellLineBox {
                    seg: sg,
                    top,
                    bottom,
                    part,
                    origin: shift,
                });
            }
        }
        float_bottom = f64::NEG_INFINITY;
        for &run in &d.nodes[p].children {
            if d.nodes[run].local != "run" {
                continue;
            }
            for &o in &d.nodes[run].children {
                if d.nodes[o].local != "tbl"
                    || d.nodes[o].attr("textWrap").unwrap_or("TOP_AND_BOTTOM") != "TOP_AND_BOTTOM"
                {
                    continue;
                }
                let pos = child(d, o, "pos");
                if pos.is_none()
                    || attr(d, pos, "treatAsChar") == Some("1")
                    || attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA"
                {
                    continue;
                }
                let om = child(d, o, "outMargin");
                let vo = num(d, pos, "vertOffset", 0.0);
                let sz = num(d, child(d, o, "sz"), "height", 0.0);
                let full = pm::without_extent_memo(ctx, |ctx| pm::measure_table_height(d, o, ctx))?;
                let top = p_top + if vo < 2147483648.0 { vo } else { 0.0 } + num(d, om, "top", 0.0);
                if full > sz + 1000.0 {
                    let fits = if let Some(bh) = body_h {
                        top + pm::without_extent_memo(ctx, |ctx| {
                            pm::measure_table_row_heights(d, o, ctx)
                        })?
                        .first()
                        .copied()
                        .unwrap_or(0.0)
                            <= bh
                    } else {
                        true
                    };
                    if fits {
                        float_bottom = max(float_bottom, top + full + num(d, om, "bottom", 0.0));
                    }
                }
            }
        }
    }
    Ok(out)
}
pub fn span_part_rows(
    d: &ParsedXml,
    cells: &[Value],
    row_y: &[f64],
    body_h: Option<f64>,
    ctx: &mut Context,
) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    let mut budget = Budget::default();
    for c in cells {
        if n(c, "rs") < 2.0 {
            continue;
        }
        let bs = cell_line_boxes(d, c, body_h, ctx)?;
        let parts = bs.last().map_or(0, |b| b.part);
        let mut s = n(c, "ra");
        for k in 0..parts {
            budget.tick()?;
            let in_k: Vec<_> = bs.iter().filter(|b| b.part == k).collect();
            if in_k.is_empty() {
                break;
            }
            let ext = max_values(in_k.iter().map(|b| b.bottom), f64::NEG_INFINITY) - in_k[0].origin
                + n(c, "marginT")
                + n(c, "marginB");
            let mut r = s + 1.0;
            while r < n(c, "ra") + n(c, "rs") && at(row_y, r) - at(row_y, s) < ext - 100.0 {
                budget.tick()?;
                r += 1.0;
            }
            if r >= n(c, "ra") + n(c, "rs") {
                break;
            }
            if !out
                .iter()
                .any(|&x: &f64| x == r || x.is_nan() && r.is_nan())
            {
                out.push(r);
            }
            s = r;
        }
    }
    Ok(out)
}
fn co_anchored_follower(d: &ParsedXml, tbl: usize) -> bool {
    let is_tb = |o| {
        let p = child(d, o, "pos");
        p.is_some()
            && attr(d, p, "treatAsChar") != Some("1")
            && attr(d, p, "vertRelTo").unwrap_or("PARA") == "PARA"
            && d.nodes[o].attr("textWrap").unwrap_or("TOP_AND_BOTTOM") == "TOP_AND_BOTTOM"
    };
    let Some(p) = d.nodes[tbl].parent.and_then(|p| d.nodes[p].parent) else {
        return false;
    };
    if d.nodes[p].local != "p" || !is_tb(tbl) {
        return false;
    }
    let mut seen = false;
    for &run in &d.nodes[p].children {
        if d.nodes[run].local != "run" {
            continue;
        }
        for &o in &d.nodes[run].children {
            if o == tbl {
                return seen;
            }
            if matches!(d.nodes[o].local.as_str(), "tbl" | "pic") && is_tb(o) {
                seen = true;
            }
        }
    }
    false
}
fn ranges(cells: &[&Value], from: f64, to: f64) -> Result<Map<usize, Value>, String> {
    cells
        .iter()
        .map(|c| Ok((el(c)?, json!([from, to]))))
        .collect()
}
fn all_cells_ranges(cells: &[&Value], from: &Map<usize, f64>) -> Result<Map<usize, Value>, String> {
    cells
        .iter()
        .map(|c| {
            let id = el(c)?;
            Ok((
                id,
                json!([from.get(&id).copied().unwrap_or(0.0), f64::INFINITY]),
            ))
        })
        .collect()
}
fn boxes_for(
    d: &ParsedXml,
    cells: &[&Value],
    body_h: f64,
    ctx: &mut Context,
) -> Result<Map<usize, Vec<CellLineBox>>, String> {
    cells
        .iter()
        .map(|c| Ok((el(c)?, cell_line_boxes(d, c, Some(body_h), ctx)?)))
        .collect()
}
fn boxes<'a>(all: &'a Map<usize, Vec<CellLineBox>>, c: &Value) -> &'a [CellLineBox] {
    id(&c["el"])
        .and_then(|i| all.get(&i))
        .map_or(&[], Vec::as_slice)
}
fn call_span(
    callback: &mut Option<&mut FloatSpanCallback<'_>>,
    b: &CellLineBox,
    y: f64,
    cont: f64,
    ctx: &mut Context,
) -> Result<Option<FloatSpan>, String> {
    if let Some(cb) = callback.as_deref_mut() {
        cb(b.seg, y, cont, ctx)
    } else {
        Ok(None)
    }
}

/// Computes complete row fragments before returning. Numeric intermediates and
/// node-keyed ordered maps stay native until the reporting bridge.
pub fn plan_table_rows(
    d: &ParsedXml,
    tbl: usize,
    cells: &[Value],
    row_h: &[f64],
    opts: PlanOptions,
    ctx: &mut Context,
    mut float_span: Option<&mut FloatSpanCallback<'_>>,
) -> Result<Vec<Vec<RowPiece>>, String> {
    let PlanOptions {
        ty,
        mt,
        body_h,
        mut slack,
        nested,
        cont_mt,
    } = opts;
    if nested {
        slack = 0.0;
    }
    let mut budget = Budget::default();
    let n_rows = row_h.len();
    if n_rows > MAX_PLAN_STEPS {
        return Err(LIMIT_ERROR.into());
    }
    let mut row_y = vec![0.0];
    for &h in row_h {
        row_y.push(row_y.last().unwrap() + h);
    }
    let plain = |page: f64, top: f64| {
        row_h
            .iter()
            .enumerate()
            .map(|(r, &h)| vec![RowPiece::new(page, top + row_y[r], h)])
            .collect::<Vec<_>>()
    };
    let page_bottom = mt + body_h;
    let total = row_y[n_rows];
    let cell_split = d.nodes[tbl].attr("pageBreak") == Some("CELL");
    let stack_over = cell_split
        && row_h.iter().enumerate().any(|(r, &h)| {
            let rc: Vec<_> = cells
                .iter()
                .filter(|c| strict(c, "ra", r as f64) && strict(c, "rs", 1.0))
                .collect();
            if rc.len() != 1 {
                return false;
            }
            let st = stacked_floats(d, rc[0]);
            st.len() >= 2 && st.iter().fold(0.0, |a, f| a + f.h) > h
        });
    if slack > 0.0 && ty + total > page_bottom && ty + total <= page_bottom + slack {
        for c in cells {
            if cell_line_boxes(d, c, Some(body_h), ctx)?
                .iter()
                .any(|b| b.part > 0)
            {
                slack = 0.0;
                break;
            }
        }
    }
    if slack > 0.0 && ty + total > page_bottom && ty + total <= page_bottom + slack {
        let declared = num(d, child(d, tbl, "sz"), "height", 0.0);
        if row_y
            .iter()
            .enumerate()
            .any(|(k, &v)| k > 0 && k < n_rows && (v - declared).abs() <= 2.0)
        {
            slack = 0.0;
        }
    }
    let mut saved_parts = false;
    if cell_split {
        for c in cells {
            if n(c, "h") > total
                && cell_line_boxes(d, c, Some(body_h), ctx)?
                    .iter()
                    .any(|b| b.part > 0)
            {
                saved_parts = true;
                break;
            }
        }
    }
    if body_h <= 0.0 || (ty + total <= page_bottom + slack && !stack_over && !saved_parts) {
        return Ok(plain(0.0, ty));
    }
    if attr(d, child(d, tbl, "pos"), "treatAsChar") == Some("1") {
        let declared = num(d, child(d, tbl, "sz"), "height", 0.0);
        let declared = if declared == 0.0 || declared.is_nan() {
            total
        } else {
            declared
        };
        return Ok(
            if ty - mt < 2000.0 || ty + declared <= page_bottom + slack {
                plain(0.0, ty)
            } else {
                plain(1.0, mt)
            },
        );
    }
    let mut hdr_rows = 0;
    if d.nodes[tbl].attr("repeatHeader") == Some("1") {
        while hdr_rows < n_rows {
            let rc: Vec<_> = cells
                .iter()
                .filter(|c| strict(c, "ra", hdr_rows as f64))
                .collect();
            if rc.is_empty() {
                break;
            }
            let mut all = true;
            for c in rc {
                if d.nodes[el(c)?].attr("header") != Some("1") {
                    all = false;
                    break;
                }
            }
            if !all {
                break;
            }
            hdr_rows += 1;
        }
    }
    let om = child(d, tbl, "outMargin");
    let cont_top = cont_mt + num(d, om, "top", 0.0) + row_y[hdr_rows];
    let out_b = num(d, om, "bottom", 0.0);
    let mut out: Vec<Vec<RowPiece>> = Vec::new();
    let flows = attr(d, child(d, tbl, "pos"), "vertRelTo").unwrap_or("PARA") == "PARA";
    let mut page = 0.0;
    let mut y = ty;
    let mut first = !flows || ty - mt < 2000.0;
    if !first && d.nodes[tbl].attr("pageBreak").unwrap_or("NONE") == "NONE" {
        let out_t = num(d, om, "top", 0.0);
        if out_t + total + out_b <= body_h {
            return Ok(plain(1.0, cont_mt + out_t));
        }
    }
    if !first && co_anchored_follower(d, tbl) {
        let out_t = num(d, om, "top", 0.0);
        if out_t + total + out_b <= body_h {
            return Ok(plain(1.0, cont_mt + out_t));
        }
    }
    let span_starts = if cell_split {
        span_part_rows(d, cells, &row_y, Some(body_h), ctx)?
    } else {
        Vec::new()
    };
    let declared_h = num(d, child(d, tbl, "sz"), "height", 0.0);
    let cut_row = if declared_h > 0.0 && declared_h < total - 1000.0 {
        row_y
            .iter()
            .enumerate()
            .find(|&(k, v)| {
                k > hdr_rows
                    && k < n_rows
                    && (*v - declared_h).abs() <= 20.0
                    && ty + v <= page_bottom
            })
            .map(|(k, _)| k)
    } else {
        None
    };
    for r in 0..n_rows {
        budget.tick()?;
        let mut h = row_h[r];
        if Some(r) == cut_row && page == 0.0 && !first {
            page += 1.0;
            y = cont_top;
            first = true;
        }
        let mut pieces = Vec::new();
        if !first
            && r >= hdr_rows
            && span_starts.contains(&(r as f64))
            && !cells.iter().any(|c| {
                let ra = n(c, "ra");
                let first_page = if ra >= 0.0 && ra.fract() == 0.0 {
                    out.get(ra as usize)
                        .and_then(|row| row.first())
                        .map_or(page, |p| p.page)
                } else {
                    page
                };
                n(c, "rs") > 1.0
                    && ra < (r as f64)
                    && ra + n(c, "rs") > (r as f64)
                    && first_page < page
            })
        {
            page += 1.0;
            y = cont_top;
            first = true;
        }
        let row_cells: Vec<_> = cells
            .iter()
            .filter(|c| strict(c, "ra", r as f64) && strict(c, "rs", 1.0))
            .collect();
        let splittable = cell_split && r >= hdr_rows && !row_cells.is_empty();
        let stack = if splittable && row_cells.len() == 1 {
            stacked_floats(d, row_cells[0])
        } else {
            Vec::new()
        };
        if stack.len() >= 2 {
            let c = row_cells[0];
            let mut top = y;
            let mut bottom = y + n(c, "marginT");
            let mut pcs = Vec::new();
            let mut place = Map::new();
            for (k, f) in stack.iter().enumerate() {
                budget.tick()?;
                let mut at = if k == 0 {
                    top + n(c, "marginT") + f.vo
                } else {
                    bottom
                };
                if k > 0 && at + f.h > page_bottom - n(c, "marginB") {
                    let mut range = Map::new();
                    range.insert(
                        el(c)?,
                        json!([
                            if pcs.is_empty() { 0.0 } else { f64::INFINITY },
                            f64::INFINITY
                        ]),
                    );
                    budget.push(
                        &mut pcs,
                        RowPiece::new(page, top, bottom - top + n(c, "marginB"))
                            .range(range)
                            .place(place),
                    )?;
                    page += 1.0;
                    top = cont_top;
                    place = Map::new();
                    at = top + n(c, "marginT");
                    bottom = at;
                }
                place.insert(f.el, json!(at + f.om_t));
                bottom = max(bottom, at + f.h);
            }
            if !pcs.is_empty() {
                let mut range = Map::new();
                range.insert(el(c)?, json!([f64::INFINITY, f64::INFINITY]));
                budget.push(
                    &mut pcs,
                    RowPiece::new(page, top, bottom - top + n(c, "marginB"))
                        .range(range)
                        .place(place),
                )?;
                out.push(pcs);
                y = bottom + n(c, "marginB");
                first = false;
                continue;
            }
        }
        let all_boxes = if splittable {
            Some(boxes_for(d, &row_cells, body_h, ctx)?)
        } else {
            None
        };
        let h_parts = all_boxes.as_ref().map_or(0, |all| {
            all.values()
                .flat_map(|bs| bs.iter().map(|b| b.part))
                .max()
                .unwrap_or(0)
        });
        if let Some(all) = all_boxes.as_ref().filter(|_| h_parts > 0) {
            if !first
                && row_cells.iter().any(|c| {
                    boxes(all, c).iter().find(|b| b.part == 0).is_some_and(|b| {
                        n(c, "marginT") + b.bottom - b.origin > page_bottom - y - n(c, "marginB")
                    })
                })
            {
                page += 1.0;
                y = cont_top;
            }
            let declared = num(d, child(d, tbl, "sz"), "height", 0.0);
            let mut led = Vec::new();
            let mut first_ph = 0.0;
            for k in 0..=h_parts {
                budget.tick()?;
                if k > 0 && float_span.is_some() {
                    let mut lead = 0.0;
                    let mut place = None;
                    for c in &row_cells {
                        let Some(b) = boxes(all, c).iter().find(|b| b.part == k) else {
                            continue;
                        };
                        if let Some(f) = call_span(
                            &mut float_span,
                            b,
                            y + n(c, "marginT"),
                            cont_top + n(c, "marginT"),
                            ctx,
                        )? && f.el.is_some()
                            && f.pages > 0.0
                            && f.pages > lead
                            && (cont_top + n(c, "marginT") + b.top - b.origin - f.end_y).abs()
                                < 3000.0
                        {
                            lead = f.pages;
                            let mut map = Map::new();
                            map.insert(f.el.unwrap(), json!(y + n(c, "marginT")));
                            place = Some(map);
                            if !led.contains(&b.seg) {
                                led.push(b.seg);
                            }
                        }
                    }
                    if lead > 0.0 {
                        let none = ranges(&row_cells, f64::INFINITY, f64::INFINITY)?;
                        let mut e = 0.0;
                        while e < lead {
                            budget.tick()?;
                            let mut piece =
                                RowPiece::new(page + e, cont_top, page_bottom - cont_top)
                                    .range(none.clone());
                            if e == 0.0 {
                                piece.place = place.clone();
                            }
                            budget.push(&mut pieces, piece)?;
                            e += 1.0;
                        }
                        page += lead;
                    }
                }
                let mut range = Map::new();
                let mut ext = 0.0;
                let mut to_next = 0.0;
                for c in &row_cells {
                    let bs = boxes(all, c);
                    let in_k: Vec<_> = bs.iter().filter(|b| b.part == k).collect();
                    let next = bs.iter().find(|b| b.part > k);
                    let from = in_k
                        .first()
                        .map(|b| b.origin)
                        .or_else(|| next.map(|b| b.origin))
                        .unwrap_or(f64::INFINITY);
                    range.insert(el(c)?, json!([from, next.map_or(f64::INFINITY, |b| b.top)]));
                    if let Some(last) = in_k.last() {
                        ext = max(ext, last.bottom - from + n(c, "marginT") + n(c, "marginB"));
                    }
                    if k == h_parts
                        && !in_k.is_empty()
                        && let Some(sub) = id(&c["sub"])
                    {
                        for f in pm::without_extent_memo(ctx, |ctx| sub_float_bottoms(d, sub, ctx))?
                        {
                            if let Some(b) = bs.iter().find(|b| Some(b.seg) == id(&f["seg"]))
                                && b.part == k
                            {
                                ext = max(
                                    ext,
                                    b.top + n(&f, "bottom") - n(&f, "v0") - from
                                        + n(c, "marginT")
                                        + n(c, "marginB"),
                                );
                            }
                        }
                    }
                    if !in_k.is_empty()
                        && let Some(next) = next
                    {
                        to_next = max(to_next, next.top - from + n(c, "marginT") + n(c, "marginB"));
                    }
                }
                if k > 0 {
                    for c in cells {
                        if n(c, "rs") < 2.0 || n(c, "ra") + n(c, "rs") - 1.0 != r as f64 {
                            continue;
                        }
                        let bs = cell_line_boxes(d, c, Some(body_h), ctx)?;
                        let in_k: Vec<_> = bs.iter().filter(|b| b.part == k).collect();
                        if let Some(last) = in_k.last() {
                            ext = max(
                                ext,
                                last.bottom - in_k[0].origin + n(c, "marginT") + n(c, "marginB"),
                            );
                        }
                    }
                }
                let last = k == h_parts;
                let cell_h = max_values(
                    row_cells.iter().map(|c| {
                        num(
                            d,
                            id(&c["el"]).and_then(|i| child(d, i, "cellSz")),
                            "height",
                            0.0,
                        )
                    }),
                    0.0,
                );
                let first_h = min(
                    if declared > 0.0 && ty + declared - y >= ext {
                        min(page_bottom - y, ty + declared - y)
                    } else {
                        page_bottom - y
                    },
                    max(ext, cell_h),
                );
                let share = if last && k == 1 {
                    max_values(
                        row_cells
                            .iter()
                            .filter(|c| {
                                d.nodes[tbl].attr("noAdjust") == Some("1")
                                    || matches!(
                                        attr(d, id(&c["sub"]), "vertAlign"),
                                        Some("CENTER" | "BOTTOM")
                                    )
                            })
                            .map(|c| n(c, "h") - first_ph)
                            .filter(|v| *v <= page_bottom - cont_top),
                        0.0,
                    )
                } else {
                    0.0
                };
                let ph = if last {
                    max(ext, share)
                } else if k == 0 {
                    first_h
                } else if to_next > 0.0 {
                    min(page_bottom - cont_top, to_next)
                } else {
                    page_bottom - cont_top
                };
                if k == 0 {
                    first_ph = ph;
                }
                budget.push(&mut pieces, RowPiece::new(page, y, ph).range(range))?;
                if !last {
                    let mut span = 0.0;
                    let mut span_end = 0.0;
                    if float_span.is_some() {
                        for c in &row_cells {
                            let bs: Vec<_> = boxes(all, c).iter().filter(|b| b.part == k).collect();
                            let from = bs.first().map_or(0.0, |b| b.origin);
                            for b in bs {
                                if led.contains(&b.seg) {
                                    continue;
                                }
                                if let Some(f) = call_span(
                                    &mut float_span,
                                    b,
                                    y + n(c, "marginT") + b.top - from,
                                    cont_top + n(c, "marginT"),
                                    ctx,
                                )? && f.pages > span
                                {
                                    span = f.pages;
                                    span_end = f.end_y;
                                }
                            }
                        }
                    }
                    if span > 0.0 {
                        let next_top = minimum(row_cells.iter().map(|c| {
                            boxes(all, c)
                                .iter()
                                .find(|b| b.part == k + 1)
                                .map_or(f64::INFINITY, |b| {
                                    cont_top + n(c, "marginT") + b.top - b.origin
                                })
                        }));
                        let skip = if next_top < span_end - 100.0 {
                            span + 1.0
                        } else {
                            span
                        };
                        let mut e = 1.0;
                        while e < skip {
                            budget.tick()?;
                            budget.push(
                                &mut pieces,
                                RowPiece::new(page + e, cont_top, page_bottom - cont_top)
                                    .range(ranges(&row_cells, f64::INFINITY, f64::INFINITY)?),
                            )?;
                            e += 1.0;
                        }
                        page += skip;
                    } else {
                        page += 1.0;
                    }
                    y = cont_top;
                } else {
                    h = ph;
                }
            }
            out.push(pieces);
            y += h;
            first = false;
            continue;
        }
        if let Some(all) = all_boxes.as_ref().filter(|_| float_span.is_some()) {
            let mut span = 0.0;
            let mut span_end = 0.0;
            let mut margin_b = 0.0;
            for c in &row_cells {
                for b in boxes(all, c) {
                    if let Some(f) = call_span(
                        &mut float_span,
                        b,
                        y + n(c, "marginT") + b.top,
                        cont_top + n(c, "marginT"),
                        ctx,
                    )? && f.pages > span
                    {
                        span = f.pages;
                        span_end = f.end_y;
                        margin_b = n(c, "marginB");
                    }
                }
            }
            if span > 0.0 {
                let none = ranges(&row_cells, f64::INFINITY, f64::INFINITY)?;
                budget.push(
                    &mut pieces,
                    RowPiece::new(page, y, page_bottom - y)
                        .range(ranges(&row_cells, 0.0, f64::INFINITY)?)
                        .fill(),
                )?;
                let mut e = 1.0;
                while e < span {
                    budget.tick()?;
                    budget.push(
                        &mut pieces,
                        RowPiece::new(page + e, cont_top, page_bottom - cont_top)
                            .range(none.clone())
                            .fill(),
                    )?;
                    e += 1.0;
                }
                page += span;
                let end_h = max(0.0, span_end - cont_top) + margin_b;
                budget.push(
                    &mut pieces,
                    RowPiece::new(page, cont_top, end_h).range(none),
                )?;
                out.push(pieces);
                y = cont_top + end_h;
                first = false;
                continue;
            }
        }
        if splittable && y + h + out_b > page_bottom {
            let mut from = Map::new();
            let mut lines = Map::new();
            let mut sp0 = Map::new();
            for c in &row_cells {
                lines.insert(
                    el(c)?,
                    cell_line_boxes(d, c, Some(body_h), ctx)?
                        .iter()
                        .map(|b| [b.top, b.bottom])
                        .collect::<Vec<_>>(),
                );
            }
            for c in &row_cells {
                let bs = cell_line_boxes(d, c, Some(body_h), ctx)?;
                sp0.insert(
                    el(c)?,
                    bs.first()
                        .map_or(0.0, |b| num(d, Some(b.seg), "spacing", 0.0)),
                );
            }
            while y + h + out_b > page_bottom && body_h > row_y[hdr_rows] {
                budget.tick()?;
                let avail = page_bottom - y - out_b;
                let mut rem = 0.0;
                let mut placed = false;
                let mut all_fit = true;
                let mut blocked = false;
                let mut cuts = Vec::new();
                for c in &row_cells {
                    let id = el(c)?;
                    let off = from.get(&id).copied().unwrap_or(0.0);
                    let ls: Vec<_> = lines
                        .get(&id)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter(|l| l[0] >= off)
                        .collect();
                    if ls.is_empty() {
                        cuts.push((id, off, off));
                        continue;
                    }
                    let end = max_values(ls.iter().map(|l| l[1]), f64::NEG_INFINITY);
                    let cut = ls.iter().enumerate().position(|(i, l)| {
                        n(c, "marginT")
                            + (l[1]
                                + if off == 0.0 && i == 0 && ls.len() > 1 {
                                    sp0.get(&id).copied().unwrap_or(0.0)
                                } else {
                                    0.0
                                }
                                - off)
                            > avail - n(c, "marginB")
                    });
                    let Some(k) = cut else {
                        cuts.push((id, off, end + 1.0));
                        continue;
                    };
                    all_fit = false;
                    if k > 0 {
                        placed = true;
                    } else if off == 0.0 {
                        blocked = true;
                    }
                    cuts.push((id, off, ls[k][0]));
                    rem = max(rem, end - ls[k][0] + n(c, "marginT") + n(c, "marginB"));
                }
                if all_fit {
                    let span_cells: Vec<_> = cells
                        .iter()
                        .filter(|c| strict(c, "ra", r as f64) && n(c, "rs") > 1.0)
                        .collect();
                    if !first && h <= page_bottom - cont_top {
                        let mut extents = Vec::new();
                        for c in &row_cells {
                            extents.push(
                                pm::without_extent_memo(ctx, |ctx| {
                                    pm::cell_content_extent(d, c, ctx)
                                })? + n(c, "marginT")
                                    + n(c, "marginB"),
                            );
                        }
                        for c in span_cells {
                            let bs = cell_line_boxes(d, c, Some(body_h), ctx)?;
                            let ext = if bs.iter().any(|b| b.part > 0) {
                                max_values(
                                    bs.iter().filter(|b| b.part == 0).map(|b| b.bottom),
                                    f64::NEG_INFINITY,
                                )
                            } else {
                                pm::without_extent_memo(ctx, |ctx| {
                                    pm::cell_content_extent(d, c, ctx)
                                })?
                            };
                            extents.push(ext + n(c, "marginT") + n(c, "marginB"));
                        }
                        if extents.iter().any(|&e| e > avail) {
                            page += 1.0;
                            y = cont_top;
                            break;
                        }
                    }
                    let mut carry = !nested && h - avail > 1000.0 && h <= page_bottom - cont_top;
                    if carry {
                        for c in &row_cells {
                            let e = pm::without_extent_memo(ctx, |ctx| {
                                pm::cell_content_extent(d, c, ctx)
                            })? + n(c, "marginT")
                                + n(c, "marginB");
                            let va = attr(d, id(&c["sub"]), "vertAlign").unwrap_or("TOP");
                            let e = if va == "CENTER" {
                                (h + e) / 2.0
                            } else if va == "BOTTOM" {
                                h
                            } else {
                                e
                            };
                            let fits = e <= avail;
                            if !fits {
                                carry = false;
                                break;
                            }
                        }
                    }
                    if (nested || carry) && avail > 0.0 {
                        budget.push(
                            &mut pieces,
                            RowPiece::new(page, y, avail)
                                .range(all_cells_ranges(&row_cells, &from)?),
                        )?;
                        for c in &row_cells {
                            from.insert(el(c)?, f64::INFINITY);
                        }
                        page += 1.0;
                        y = cont_top;
                        first = true;
                        h -= avail;
                        continue;
                    }
                    h = avail;
                    break;
                }
                if blocked && !first {
                    page += 1.0;
                    y = cont_top;
                    first = true;
                    continue;
                }
                if !placed {
                    if first {
                        break;
                    }
                    page += 1.0;
                    y = cont_top;
                    first = true;
                    continue;
                }
                let range = cuts.iter().map(|&(id, a, b)| (id, json!([a, b]))).collect();
                budget.push(&mut pieces, RowPiece::new(page, y, avail).range(range))?;
                for (id, _, b) in cuts {
                    from.insert(id, b);
                }
                page += 1.0;
                y = cont_top;
                first = true;
                h = rem;
            }
            budget.push(
                &mut pieces,
                RowPiece::new(page, y, h).range(all_cells_ranges(&row_cells, &from)?),
            )?;
            out.push(pieces);
            y += h;
            first = false;
            continue;
        }
        if !first && y + h + if cell_split { 0.0 } else { out_b } > page_bottom && r >= hdr_rows {
            page += 1.0;
            y = cont_top;
        } else if !first
            && d.nodes[tbl].attr("pageBreak") == Some("TABLE")
            && r >= hdr_rows
            && !cells
                .iter()
                .any(|c| n(c, "ra") < (r as f64) && n(c, "ra") + n(c, "rs") > (r as f64))
        {
            let mut end = r as f64 + 1.0;
            let mut grown = true;
            while grown {
                budget.tick()?;
                grown = false;
                for c in cells {
                    if n(c, "ra") >= r as f64 && n(c, "ra") < end && n(c, "ra") + n(c, "rs") > end {
                        end = n(c, "ra") + n(c, "rs");
                        grown = true;
                    }
                }
            }
            let group_h = at(&row_y, min(end, n_rows as f64)) - row_y[r];
            if end > r as f64 + 1.0
                && y + group_h + out_b > page_bottom
                && group_h <= page_bottom - cont_top
            {
                page += 1.0;
                y = cont_top;
            }
        }
        budget.push(&mut pieces, RowPiece::new(page, y, h))?;
        out.push(pieces);
        y += h;
        first = false;
    }
    Ok(out)
}

#[cfg(test)]
fn select(d: &ParsedXml, input: &Value) -> Result<usize, String> {
    if let Some(node) = id(&input["node"]) {
        return if node < d.nodes.len() {
            Ok(node)
        } else {
            Err("노드 위치 오류".into())
        };
    }
    let mut root = d.root;
    if let Some(path) = input["path"].as_array() {
        for i in path {
            root = *d.nodes[root]
                .children
                .get(i.as_u64().ok_or("노드 경로 오류")? as usize)
                .ok_or("노드 경로 범위 오류")?;
        }
    }
    Ok(root)
}
#[cfg(test)]
fn number_or(input: &Value, key: &str, fallback: f64) -> f64 {
    if input[key].is_undefined() {
        fallback
    } else {
        number(&input[key])
    }
}
#[cfg(test)]
fn row_heights(
    d: &ParsedXml,
    tbl: usize,
    input: &Value,
    ctx: &mut Context,
) -> Result<Vec<f64>, String> {
    if let Some(rows) = input["rowH"].as_array() {
        Ok(rows.iter().map(number).collect())
    } else {
        pm::without_extent_memo(ctx, |ctx| pm::measure_table_row_heights(d, tbl, ctx))
    }
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    let d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let tbl = select(&d, input)?;
    let mut ctx = Context::default();
    if truth(&input["prepare"]) {
        pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx);
    }
    let mut cells = pm::collect_cells(&d, tbl);
    if let Some(overrides) = input["cellOverrides"].as_array() {
        for entry in overrides {
            let i = entry["index"].as_u64().ok_or("셀 위치 오류")? as usize;
            let c = cells.get_mut(i).ok_or("셀 위치 범위 오류")?;
            for (k, v) in entry["values"].as_object().ok_or("셀 덮어쓰기 객체 오류")? {
                c[k] = v.clone();
            }
        }
    }
    match operation {
        "cellLineBoxes" => {
            let c = cells
                .get(input["cellIndex"].as_u64().unwrap_or(0) as usize)
                .ok_or("선택한 셀이 없습니다.")?;
            Ok(cell_line_boxes(
                &d,
                c,
                input.get("bodyH").filter(|v| !v.is_undefined()).map(number),
                &mut ctx,
            )?
            .iter()
            .map(CellLineBox::to_value)
            .collect())
        }
        "spanPartRows" => {
            let row_y = if let Some(rows) = input["rowY"].as_array() {
                rows.iter().map(number).collect()
            } else {
                let mut ys = vec![0.0];
                for h in row_heights(&d, tbl, input, &mut ctx)? {
                    ys.push(ys.last().unwrap() + h);
                }
                ys
            };
            Ok(Value::from(span_part_rows(
                &d,
                &cells,
                &row_y,
                input.get("bodyH").filter(|v| !v.is_undefined()).map(number),
                &mut ctx,
            )?))
        }
        "planTableRows" => {
            let row_h = row_heights(&d, tbl, input, &mut ctx)?;
            let opts = PlanOptions {
                ty: n(input, "ty"),
                mt: n(input, "MT"),
                body_h: n(input, "BODY_H"),
                slack: number_or(input, "slack", 0.0),
                nested: truth(&input["nested"]),
                cont_mt: number_or(input, "contMT", n(input, "MT")),
            };
            let mut calls = Vec::new();
            let supplied = input.get("floatSpans").is_some();
            let replies = input["floatSpans"].as_array().cloned().unwrap_or_default();
            let repeat = truth(&input["repeatLast"]);
            let throw_at = input["floatSpanErrorAt"].as_u64();
            let mut callback = |seg: usize, y: f64, cont: f64, _: &mut Context| {
                let at = calls.len();
                calls.push(json!({"seg":seg,"y":y,"contY":cont}));
                if throw_at == Some(at as u64) {
                    return Err("synthetic floatSpan failure".into());
                }
                let value = replies
                    .get(at)
                    .or_else(|| if repeat { replies.last() } else { None });
                let Some(value) = value.filter(|v| !v.is_null() && !v.is_undefined()) else {
                    return Ok(None);
                };
                let el = if value["elPath"].is_array() {
                    let wrapper = json!({"path":value["elPath"]});
                    Some(select(&d, &wrapper)?)
                } else {
                    id(&value["el"]).filter(|&id| id < d.nodes.len())
                };
                Ok(Some(FloatSpan {
                    el,
                    pages: n(value, "pages"),
                    end_y: n(value, "endY"),
                }))
            };
            let rows = plan_table_rows(
                &d,
                tbl,
                &cells,
                &row_h,
                opts,
                &mut ctx,
                if supplied { Some(&mut callback) } else { None },
            )?;
            let rows = rows
                .iter()
                .map(|row| row.iter().map(RowPiece::to_value).collect::<Value>())
                .collect::<Value>();
            Ok(json!({"rows":rows,"calls":calls}))
        }
        _ => Err(format!("알 수 없는 table-pages 연산: {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_array_addresses_remain_undefined() {
        assert!(at(&[0.0, 100.0], 0.5).is_nan());
        assert!(at(&[0.0], f64::INFINITY).is_nan());
        assert_eq!(at(&[-0.0], -0.0).to_bits(), (-0.0_f64).to_bits());
    }
    #[test]
    fn piece_maps_keep_native_infinities_and_insertion_order() {
        let mut range = Map::new();
        range.insert(12, json!([f64::INFINITY, f64::NAN]));
        range.insert(4, json!([-0.0, 3.5]));
        let piece = RowPiece::new(0.0, -0.0, 4.0).range(range);
        let value = piece.to_value();
        assert_eq!(value["range"][0][0], 12);
        assert!(value["range"][0][1][0].as_f64().unwrap().is_infinite());
        assert_eq!(
            value["range"][1][1][0].as_f64().unwrap().to_bits(),
            (-0.0_f64).to_bits()
        );
    }
    #[test]
    fn nonprogressing_callback_is_bounded() {
        let d=parse_style_xml("<tbl pageBreak=\"CELL\"><sz height=\"100\"/><tr><tc><cellAddr rowAddr=\"0\" colAddr=\"0\"/><cellSpan rowSpan=\"1\" colSpan=\"1\"/><cellSz width=\"100\" height=\"5000\"/><cellMargin top=\"0\" bottom=\"0\"/><subList><p><linesegarray><lineseg vertpos=\"0\" vertsize=\"100\"/></linesegarray></p></subList></tc></tr></tbl>").unwrap();
        let cells = pm::collect_cells(&d, 0);
        let mut callback = |_: usize, _: f64, _: f64, _: &mut Context| {
            Ok(Some(FloatSpan {
                el: Some(0),
                pages: f64::INFINITY,
                end_y: 0.0,
            }))
        };
        let error = plan_table_rows(
            &d,
            0,
            &cells,
            &[5000.0],
            PlanOptions {
                ty: 0.0,
                mt: 0.0,
                body_h: 1000.0,
                slack: 0.0,
                nested: false,
                cont_mt: 0.0,
            },
            &mut Context::default(),
            Some(&mut callback),
        )
        .unwrap_err();
        assert_eq!(error, LIMIT_ERROR);
    }
}
