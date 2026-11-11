//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Block/section/endnote flow from render/reflow.mjs; data/kordoc-LICENSE.
#[cfg(test)]
use super::{head_styles, style_dom::parse_style_xml};
use super::{
    layout::{maximum as max, minimum as min},
    numbering::truth,
    para_model::{self as pm, Context, child, num},
    para_number::CounterState,
    reflow_children, reflow_helpers,
    reflow_para::{self, ReflowArgs, ReflowState},
    style_dom::ParsedXml,
    table_flow as tf,
    table_split::{self, Notes, SplitState},
    value::{Value, native_json as json, number, string},
};
use std::collections::HashSet;

#[derive(Default)]
pub struct BlockState {
    pub para: ReflowState,
    pub split: SplitState,
    depth: usize,
    work: usize,
}
pub struct FlowCounter {
    pub fields: Value,
    pub nums: CounterState,
}
impl FlowCounter {
    pub fn new(body_w: f64) -> Self {
        Self {
            fields: json!({"n":0,"bodyW":body_w}),
            nums: CounterState::default(),
        }
    }
    pub fn to_value(&self) -> Value {
        let mut v = self.fields.clone();
        v["nums"] = self.nums.to_value();
        v
    }
}
fn n(v: &Value, k: &str) -> f64 {
    number(&v[k])
}
fn same_width(width: f64, value: &Value) -> bool {
    matches!(value, Value::Number(other) if width == *other)
}
fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v[k].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn map_get<'a>(v: &'a Value, key: &Value) -> Option<&'a Value> {
    v.as_array()?
        .iter()
        .rev()
        .find(|e| &e[0] == key)
        .map(|e| &e[1])
}
fn nz(v: &Value, k: &str) -> f64 {
    v.get(k)
        .filter(|v| !matches!(v, Value::Null | Value::Undefined))
        .map_or(0.0, number)
}
fn attr<'a>(d: &'a ParsedXml, i: Option<usize>, key: &str, default: &'a str) -> &'a str {
    i.and_then(|i| d.nodes[i].attr(key)).unwrap_or(default)
}
fn id(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|i| usize::try_from(i).ok())
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
fn segments(d: &ParsedXml, p: usize) -> Vec<usize> {
    child(d, p, "linesegarray")
        .map(|a| {
            d.nodes[a]
                .children
                .iter()
                .copied()
                .filter(|&i| d.nodes[i].local == "lineseg")
                .collect()
        })
        .unwrap_or_default()
}
fn put(d: &mut ParsedXml, i: usize, k: &str, v: f64) {
    d.nodes[i].set_attr(k, string(&v.into()))
}
fn shift(d: &mut ParsedXml, p: usize, delta: f64) {
    for a in d.nodes[p].children.clone() {
        if d.nodes[a].local != "linesegarray" {
            continue;
        }
        for i in d.nodes[a].children.clone() {
            if d.nodes[i].local == "lineseg" {
                put(d, i, "vertpos", num(d, Some(i), "vertpos", 0.0) + delta)
            }
        }
    }
}
fn descendants(d: &ParsedXml, root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = d.nodes[root]
        .children
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend(d.nodes[i].children.iter().rev().copied())
    }
    out
}
fn carry_zones(zones: &mut Vec<Value>, body_h: f64, one_column: bool) {
    let moved = if one_column {
        zones
            .iter()
            .filter(|z| {
                !z["carry"].is_undefined() && (n(z, "bottom") > body_h || truth(&z["nextPage"]))
            })
            .map(|z| {
                let mut v = z.clone();
                v["top"] = 0.into();
                v["bottom"] = z["carry"].clone();
                v["carry"] = Value::Undefined;
                v["nextPage"] = Value::Undefined;
                v
            })
            .collect()
    } else {
        Vec::new()
    };
    *zones = moved
}
fn regrown(d: &ParsedXml, p: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for &r in &d.nodes[p].children {
        if d.nodes[r].local != "run" {
            continue;
        }
        for &o in &d.nodes[r].children {
            if d.nodes[o].local == "tbl"
                && attr(d, child(d, o, "pos"), "treatAsChar", "") == "1"
                && descendants(d, o)
                    .iter()
                    .any(|&i| d.nodes[i].local == "p" && child(d, i, "linesegarray").is_none())
            {
                out.push(o)
            }
        }
    }
    out
}
fn table_height(d: &ParsedXml, t: usize, ctx: &mut Context) -> Result<f64, String> {
    pm::without_extent_memo(ctx, |ctx| pm::measure_table_height(d, t, ctx))
}
fn fresh_height(d: &ParsedXml, t: usize, styles: &Value, ctx: &mut Context) -> Result<f64, String> {
    let old_cells = std::mem::take(&mut ctx.cell_memo);
    let old_tables = std::mem::take(&mut ctx.table_memo);
    let old_geom = std::mem::take(&mut ctx.para_geom);
    let old_memo = ctx.memoize;
    ctx.memoize = true;
    ctx.para_geom = list(styles, "paraGeom")
        .iter()
        .filter_map(|v| v[0].as_str().map(|k| (k.to_owned(), v[1].clone())))
        .collect();
    let result = pm::measure_table_height(d, t, ctx);
    ctx.cell_memo = old_cells;
    ctx.table_memo = old_tables;
    ctx.para_geom = old_geom;
    ctx.memoize = old_memo;
    result
}
fn page_top_band(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<[f64; 2], String> {
    let mut top = f64::INFINITY;
    let mut bot = 0.0;
    for &r in &d.nodes[p].children {
        if d.nodes[r].local != "run" {
            continue;
        }
        for &o in &d.nodes[r].children {
            if !pm::OBJ_TAGS.contains(&d.nodes[o].local.as_str())
                || attr(d, Some(o), "textWrap", "") != "TOP_AND_BOTTOM"
            {
                continue;
            }
            let pos = child(d, o, "pos");
            if attr(d, pos, "treatAsChar", "") == "1"
                || attr(d, pos, "vertRelTo", "") != "PAGE"
                || attr(d, pos, "vertAlign", "TOP") != "TOP"
            {
                continue;
            }
            let om = child(d, o, "outMargin");
            let sz = child(d, o, "sz");
            let h = if d.nodes[o].local == "tbl" {
                max(num(d, sz, "height", 0.0), table_height(d, o, ctx)?)
            } else {
                num(d, sz, "height", 0.0)
            };
            let vo = max(0.0, num(d, pos, "vertOffset", 0.0));
            top = min(top, vo);
            bot = max(
                bot,
                vo + num(d, om, "top", 0.0) + h + num(d, om, "bottom", 0.0),
            );
        }
    }
    Ok(if bot > 0.0 { [top, bot] } else { [0.0, 0.0] })
}
#[derive(Clone, Copy)]
struct FootSpacing {
    above: f64,
    below: f64,
    between: f64,
}
impl FootSpacing {
    fn area(self, n: f64, sum: f64) -> f64 {
        if n > 0.0 {
            self.above + self.below + sum + self.between * (n - 1.0)
        } else {
            0.0
        }
    }
}
fn foot_spacing(d: &ParsedXml, root: usize) -> FootSpacing {
    let pr = descendants(d, root)
        .into_iter()
        .find(|&i| d.nodes[i].name == "hp:footNotePr");
    let sp = pr.and_then(|p| child(d, p, "noteSpacing"));
    FootSpacing {
        above: num(d, sp, "aboveLine", 850.0),
        below: num(d, sp, "belowLine", 567.0),
        between: num(d, sp, "betweenNotes", 283.0),
    }
}
fn foot_bottom(d: &ParsedXml, note: usize) -> f64 {
    let Some(sub) = child(d, note, "subList") else {
        return 0.0;
    };
    let mut bottom = 0.0;
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local == "p" {
            for i in segments(d, p) {
                bottom = max(
                    bottom,
                    num(d, Some(i), "vertpos", 0.0) + num(d, Some(i), "vertsize", 0.0),
                )
            }
        }
    }
    bottom
}
fn foot_refs(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<Vec<[f64; 2]>, String> {
    let m = pm::build_para(d, p, ctx)?;
    let mut out = Vec::new();
    for (i, c) in list(&m, "chars").iter().enumerate() {
        if let Some(note) = id(&c["note"])
            && d.nodes[note].local == "footNote"
        {
            out.push([foot_bottom(d, note), i as f64])
        }
    }
    for o in list(&m, "objs") {
        if let Some(el) = id(&o["el"]) {
            for f in descendants(d, el) {
                if d.nodes[f].name == "hp:footNote" {
                    out.push([foot_bottom(d, f), n(o, "index")])
                }
            }
        }
    }
    Ok(out)
}
fn foot_heights(d: &ParsedXml, p: usize, own: bool) -> Vec<f64> {
    let mut out = Vec::new();
    for f in descendants(d, p) {
        if d.nodes[f].name != "hp:footNote" || child(d, f, "subList").is_none() {
            continue;
        }
        let mut split = false;
        if own {
            let mut a = d.nodes[f].parent;
            while let Some(i) = a {
                if i == p {
                    break;
                }
                if d.nodes[i].local == "tbl"
                    && ["CELL", "TABLE"].contains(&attr(d, Some(i), "pageBreak", ""))
                    && attr(d, child(d, i, "pos"), "treatAsChar", "") != "1"
                {
                    split = true
                }
                a = d.nodes[i].parent
            }
        }
        if !split {
            out.push(foot_bottom(d, f))
        }
    }
    out
}
fn copy_nums(nums: &CounterState) -> Result<CounterState, String> {
    let mut v = nums.to_value();
    for entry in v.as_array_mut().ok_or("번호 상태 오류")? {
        let len = n(&entry[1], "length");
        let keep = list(&entry[1], "entries")
            .iter()
            .filter(|e| {
                e[0].as_str().is_some_and(|s| {
                    s.parse::<u32>()
                        .ok()
                        .is_some_and(|i| i.to_string() == s && (i as f64) < len && i < u32::MAX)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        entry[1]["entries"] = json!(keep)
    }
    CounterState::from_value(&v)
}
fn square(
    d: &ParsedXml,
    p: usize,
    start: f64,
    lim: f64,
    with_text: bool,
    width: f64,
    ctx: &mut Context,
) -> Result<bool, String> {
    tf::square_table_over(d, p, start, lim, with_text, width, ctx)
}
#[allow(clippy::too_many_arguments)]
fn split(
    d: &mut ParsedXml,
    p: usize,
    start: f64,
    lim: f64,
    body: f64,
    styles: &Value,
    with_text: bool,
    notes: Option<(f64, f64, FootSpacing)>,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<Option<f64>, String> {
    let mut area = |count: Value, sum: Value| {
        Ok(Value::from(
            notes.unwrap().2.area(number(&count), number(&sum)),
        ))
    };
    let mut ns = notes.map(|(n, s, _)| Notes {
        n0: n.into(),
        s0: s.into(),
        area: &mut area as &mut table_split::AreaCallback<'_>,
    });
    table_split::split_float_table_remainder(
        d,
        p,
        start,
        lim,
        body,
        Some(styles),
        with_text,
        ns.as_mut(),
        ctx,
        &mut state.split,
    )
}
#[allow(clippy::too_many_arguments)]
fn paragraph(
    d: &mut ParsedXml,
    p: usize,
    styles: &Value,
    width: f64,
    start: f64,
    mode: &str,
    zones: &mut Vec<Value>,
    nums: &mut CounterState,
    body: f64,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<Option<Value>, String> {
    reflow_para::reflow_para(
        d,
        p,
        styles,
        &ReflowArgs {
            area_w: width,
            start_v: start,
            mode,
            body_flow: body > 0.0,
            page_h: body,
        },
        zones,
        nums,
        ctx,
        &mut state.para,
    )
}
struct Previous {
    p: usize,
    start: f64,
    top: f64,
    h: f64,
    space_after: f64,
    keep: bool,
}
fn budget(depth: usize, work: usize, depth_limit: usize, work_limit: usize) -> Result<(), String> {
    if depth > depth_limit || work > work_limit {
        Err("문단 흐름 처리 한도(64단계 또는 100000문단)를 넘었습니다.".into())
    } else {
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
pub fn reflow_block_flow(
    d: &mut ParsedXml,
    container: usize,
    styles: &Value,
    area_w: f64,
    mode: &str,
    counter: &mut FlowCounter,
    body_h: f64,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<(), String> {
    flow(
        d,
        container,
        styles,
        area_w,
        mode,
        &mut counter.fields,
        &mut counter.nums,
        body_h,
        ctx,
        state,
    )
}
#[allow(clippy::too_many_arguments)]
fn flow(
    d: &mut ParsedXml,
    container: usize,
    styles: &Value,
    area_w: f64,
    mode: &str,
    counter: &mut Value,
    nums: &mut CounterState,
    body_h: f64,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<(), String> {
    state.depth += 1;
    let result = (|| {
        budget(state.depth, state.work, 64, 100_000)?;
        flow_inner(
            d, container, styles, area_w, mode, counter, nums, body_h, ctx, state,
        )
    })();
    state.depth -= 1;
    result
}
#[allow(clippy::too_many_arguments)]
fn flow_inner(
    d: &mut ParsedXml,
    container: usize,
    styles: &Value,
    mut area_w: f64,
    mode: &str,
    counter: &mut Value,
    nums: &mut CounterState,
    body_h: f64,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<(), String> {
    let mut cursor = 0.0;
    let mut prev_space = 0.0;
    let mut float_floor = 0.0;
    let (mut page_notes, mut page_foot) = (0.0, 0.0);
    let (mut carry_notes, mut carry_foot) = (0.0, 0.0);
    let mut content_bottom = 0.0;
    let mut split_page = false;
    let foot = foot_spacing(d, container);
    let mut zones = Vec::new();
    let mut page_limit = body_h;
    let mut prev_placed: Option<Previous> = None;
    let body_flow = same_width(area_w, &counter["bodyW"]) && body_h > 0.0;
    let vis = if body_h > 0.0 {
        pm::find_first(d, container, "visibility", 0)
    } else {
        None
    };
    let hide_first = attr(d, vis, "hideFirstEmptyLine", "") == "1";
    let mut hid_prev = 0;
    let cell_w = area_w;
    let (mut cell_cols, mut cell_col) = (1.0, 0.0);
    let mut stale_rest = false;
    let mut prev_reflowed = false;
    let (mut cache_shift, mut last_stored) = (0.0, 0.0);
    for p in d.nodes[container].children.clone() {
        if d.nodes[p].local != "p" {
            continue;
        }
        state.work += 1;
        budget(state.depth, state.work, 64, 100_000)?;
        if body_flow {
            let w = tf::column_width_in(d, p, n(counter, "bodyW"));
            if !w.is_undefined() {
                area_w = number(&w)
            }
        } else if body_h == 0.0 {
            let w = tf::column_width_in(d, p, cell_w);
            if !w.is_undefined() {
                let w = number(&w);
                cell_cols = if w == cell_w {
                    1.0
                } else {
                    round((cell_w + 1.0) / (w + 1.0))
                };
                area_w = w
            }
            if cell_cols > 1.0
                && cell_col < cell_cols - 1.0
                && cursor > 0.0
                && attr(d, Some(p), "columnBreak", "") == "1"
            {
                cell_col += 1.0;
                cursor = 0.0;
                prev_space = 0.0
            }
        }
        let grown = regrown(d, p);
        {
            let mut callback = |d: &mut ParsedXml, sub: usize, w: Value, counter: &mut Value| {
                flow(
                    d,
                    sub,
                    styles,
                    number(&w),
                    mode,
                    counter,
                    nums,
                    0.0,
                    ctx,
                    state,
                )
            };
            reflow_children::reflow_tables_in(d, p, counter, &mut callback)?;
            let note_width = Value::from(area_w);
            reflow_children::reflow_sub_lists_in(
                d,
                p,
                counter,
                if body_flow { Some(&note_width) } else { None },
                &mut callback,
            )?;
        }
        let g = map_get(
            &styles["paraGeom"],
            &Value::from(attr(d, Some(p), "paraPrIDRef", "")),
        )
        .cloned()
        .unwrap_or(Value::Undefined);
        let cursor_before = cursor;
        let mut start = max(cursor + prev_space + nz(&g, "spaceBefore"), float_floor);
        float_floor = 0.0;
        if body_h > 0.0 {
            let [top, bot] = page_top_band(d, p, ctx)?;
            if bot > 0.0 && top <= start && start < bot {
                start = bot
            }
        }
        if !stale_rest {
            for t in grown {
                if fresh_height(d, t, styles, ctx)?
                    > num(d, child(d, t, "sz"), "height", 0.0) + 500.0
                {
                    stale_rest = true;
                    break;
                }
            }
        }
        if stale_rest && let Some(a) = child(d, p, "linesegarray") {
            d.remove_child(p, a)?
        }
        let nums_before = copy_nums(nums)?;
        let zones_before = zones.len();
        let mut res = paragraph(
            d, p, styles, area_w, start, mode, &mut zones, nums, body_h, ctx, state,
        )?;
        if res.is_some() && body_flow {
            let a = child(d, p, "linesegarray");
            let first = segments(d, p).first().copied();
            let v0 = first
                .map(|i| num(d, Some(i), "vertpos", 0.0))
                .unwrap_or(start);
            let band = v0 > start
                && zones[..zones_before]
                    .iter()
                    .any(|z| n(z, "left") <= -1e8 && n(z, "bottom") == v0 && n(z, "top") < v0);
            if band && tf::has_tb_float(d, p, ctx)? {
                if let Some(a) = a {
                    d.remove_child(p, a)?
                }
                zones.truncate(zones_before);
                res = paragraph(
                    d,
                    p,
                    styles,
                    area_w,
                    v0,
                    mode,
                    &mut zones,
                    &mut copy_nums(&nums_before)?,
                    body_h,
                    ctx,
                    state,
                )?;
                start = v0
            }
        }
        let Some(res) = res else {
            let a = child(d, p, "linesegarray");
            if a.is_some() && (body_flow || body_h == 0.0) {
                let segs = segments(d, p);
                let vs = segs
                    .iter()
                    .map(|&i| num(d, Some(i), "vertpos", 0.0))
                    .collect::<Vec<_>>();
                let v0 = vs.first().copied().unwrap_or(0.0);
                let mono = vs
                    .iter()
                    .enumerate()
                    .all(|(i, &v)| i == 0 || v >= vs[i - 1]);
                if prev_reflowed {
                    let delta = start - v0;
                    cache_shift = if mono && v0 > 0.0 && delta <= 2000.0 {
                        delta
                    } else {
                        0.0
                    }
                } else if cache_shift != 0.0 && !cache_shift.is_nan() && (!mono || v0 < last_stored)
                {
                    cache_shift = 0.0
                }
                if let Some(&last) = segs.last() {
                    last_stored = vs[vs.len() - 1] + num(d, Some(last), "vertsize", 0.0)
                }
                if cache_shift != 0.0 && !cache_shift.is_nan() {
                    shift(d, p, cache_shift)
                }
            }
            prev_reflowed = false;
            if let Some(a) = a {
                let bottom = reflow_helpers::cached_paragraph_bottom(d, a, body_flow)?;
                if bottom.is_finite() {
                    cursor = bottom;
                    prev_space = nz(&g, "spaceAfter");
                    if let Some(&last) = segments(d, p).last()
                        && body_flow
                        && num(d, Some(last), "vertpos", 0.0) + num(d, Some(last), "vertsize", 0.0)
                            > body_h
                    {
                        cursor = num(d, Some(last), "vertsize", 0.0)
                            + num(d, Some(last), "spacing", 0.0);
                        prev_space = 0.0
                    }
                }
            }
            continue;
        };
        let para_h = n(&res, "paraBottom") - start;
        prev_reflowed = true;
        let explicit = body_h > 0.0
            && start > 0.0
            && (attr(d, Some(p), "pageBreak", "") == "1"
                || attr(d, Some(p), "columnBreak", "") == "1"
                || truth(&g["pageBreakBefore"]));
        let lines = if body_h > 0.0 {
            segments(d, p)
        } else {
            Vec::new()
        };
        let reserve = if body_h > 0.0 {
            tf::page_bottom_reserve(d, p, ctx)?
        } else {
            0.0
        };
        let fns = if body_h > 0.0 {
            foot_heights(d, p, false)
        } else {
            Vec::new()
        };
        let fn_sum = fns.iter().fold(0.0, |a, b| a + b);
        let lim_foot = body_h - foot.area(page_notes + fns.len() as f64, page_foot + fn_sum);
        let base_limit = if reserve > 0.0 {
            min(page_limit, body_h - reserve)
        } else {
            page_limit
        };
        let lim = min(base_limit, lim_foot);
        let own = if !fns.is_empty() {
            foot_heights(d, p, true)
        } else {
            fns.clone()
        };
        let own_sum = own.iter().fold(0.0, |a, b| a + b);
        let lim_own = if own.len() < fns.len() {
            min(
                base_limit,
                body_h - foot.area(page_notes + own.len() as f64, page_foot + own_sum),
            )
        } else {
            lim
        };
        let refs = if body_h > 0.0 && !fns.is_empty() {
            foot_refs(d, p, ctx)?
        } else {
            Vec::new()
        };
        let lim_at = |d: &ParsedXml, i: usize| {
            if refs.is_empty() {
                return lim;
            }
            let end = lines
                .get(i + 1)
                .map(|&l| num(d, Some(l), "textpos", 0.0))
                .unwrap_or(f64::INFINITY);
            let cut = if i > 0 {
                num(d, Some(lines[i]), "textpos", 0.0)
            } else {
                end
            };
            let inner = refs.iter().filter(|r| r[1] < cut).collect::<Vec<_>>();
            min(
                base_limit,
                body_h
                    - foot.area(
                        page_notes + inner.len() as f64,
                        page_foot + inner.iter().fold(0.0, |a, r| a + r[0]),
                    ),
            )
        };
        let line_bottom = |d: &ParsedXml, i: usize| {
            num(d, Some(i), "vertpos", 0.0)
                + num(d, Some(i), "vertsize", 0.0)
                + if reserve > 0.0 {
                    num(d, Some(i), "spacing", 0.0)
                } else {
                    0.0
                }
                + 1.0
        };
        let mut first_over = !lines.is_empty()
            && start > 0.0
            && line_bottom(d, lines[0])
                > if lim_own != lim {
                    lim_own
                } else {
                    lim_at(d, 0)
                };
        let buh = body_h > 0.0 && tf::bullet_empty_host(d, p, &g, ctx)?;
        if first_over && buh {
            first_over = false
        }
        let mut hidden = false;
        if first_over && hide_first && lines.len() == 1 && hid_prev < 2 {
            let model = pm::build_para(d, p, ctx)?;
            if list(&model, "objs").is_empty()
                && !list(&model, "chars")
                    .iter()
                    .any(|c| !crate::input::trim(c["ch"].as_str().unwrap_or("")).is_empty())
            {
                first_over = false;
                hidden = true
            }
        }
        let mut off = 0.0;
        let mut cut: Option<usize> = None;
        if !explicit && !first_over && lines.len() > 1 {
            cut = lines.iter().enumerate().position(|(i, &seg)| {
                i > 0
                    && line_bottom(d, seg) > lim_at(d, i)
                    && num(d, Some(seg), "vertsize", 0.0) <= body_h
            });
            if let Some(k) = cut
                && k > 0
                && truth(&g["widowOrphan"])
                && lines.len() >= 3
                && k == lines.len() - 1
            {
                cut = Some(k - 1)
            }
        }
        let orphan = cut == Some(1) && truth(&g["widowOrphan"])
            || cut.is_some_and(|k| k > 0) && truth(&g["keepLines"]);
        if let Some(cut) = cut.filter(|&k| k > 0 && !orphan) {
            for (i, &seg) in lines.iter().enumerate().skip(cut) {
                let mut vp = num(d, Some(seg), "vertpos", 0.0) + off;
                let vs = num(d, Some(seg), "vertsize", 0.0);
                if i == cut || vp > 0.0 && vp + vs > body_h && vs <= body_h {
                    off -= vp;
                    vp = 0.0;
                    carry_zones(&mut zones, body_h, same_width(area_w, &counter["bodyW"]))
                }
                put(d, seg, "vertpos", vp)
            }
        }
        if explicit
            || first_over
            || orphan
            || body_h > 0.0
                && start > 0.0
                && lines.is_empty()
                && n(&res, "paraBottom") > body_h
                && para_h <= body_h
        {
            let has_own = zones.len() > zones_before;
            zones.truncate(zones_before);
            if !zones.is_empty() || has_own {
                carry_zones(&mut zones, body_h, same_width(area_w, &counter["bodyW"]));
                if let Some(a) = child(d, p, "linesegarray") {
                    d.remove_child(p, a)?
                }
                let zb = zones.len();
                let sb = nz(&g, "spaceBefore");
                let mut moved = paragraph(
                    d,
                    p,
                    styles,
                    area_w,
                    sb,
                    mode,
                    &mut zones,
                    &mut copy_nums(&nums_before)?,
                    body_h,
                    ctx,
                    state,
                )?
                .ok_or("옮길 문단의 줄 정보가 없습니다.")?;
                let first = segments(d, p).first().copied();
                let vm = first.map(|i| num(d, Some(i), "vertpos", 0.0)).unwrap_or(sb);
                if vm > sb
                    && zones[..zb]
                        .iter()
                        .any(|z| n(z, "left") <= -1e8 && n(z, "bottom") == vm && n(z, "top") < vm)
                    && tf::has_tb_float(d, p, ctx)?
                {
                    if let Some(a) = child(d, p, "linesegarray") {
                        d.remove_child(p, a)?
                    }
                    zones.truncate(zb);
                    moved = paragraph(
                        d,
                        p,
                        styles,
                        area_w,
                        vm,
                        mode,
                        &mut zones,
                        &mut copy_nums(&nums_before)?,
                        body_h,
                        ctx,
                        state,
                    )?
                    .ok_or("옮길 문단의 줄 정보가 없습니다.")?;
                }
                cursor = n(&moved, "paraBottom");
                if body_h > 0.0
                    && square(d, p, 0.0, body_h, false, f64::NAN, ctx)?
                    && let Some(rem) =
                        split(d, p, 0.0, body_h, body_h, styles, false, None, ctx, state)?
                {
                    cursor = 0.0;
                    float_floor = rem;
                    zones.clear();
                    split_page = true
                }
            } else {
                let top = nz(&g, "spaceBefore");
                shift(d, p, top - start);
                cursor = para_h + top;
                if body_h > 0.0
                    && (top + para_h > body_h || square(d, p, top, body_h, false, f64::NAN, ctx)?)
                    && let Some(rem) =
                        split(d, p, top, body_h, body_h, styles, false, None, ctx, state)?
                {
                    cursor = 0.0;
                    float_floor = rem;
                    zones.clear();
                    split_page = true
                }
                if let Some(prev) = &prev_placed
                    && prev.keep
                    && prev.start > prev.top
                {
                    shift(d, prev.p, prev.top - prev.start);
                    let base = prev.top + prev.h + prev.space_after;
                    shift(d, p, base);
                    cursor += base
                }
            }
        } else {
            cursor = if n(&res, "floatFloor") > 0.0 && off == 0.0 {
                n(&res, "textBottom")
            } else {
                n(&res, "paraBottom")
            } + off;
            if n(&res, "floatFloor") > 0.0 && off == 0.0 {
                float_floor = n(&res, "floatFloor")
            }
            let rem = if body_h > 0.0
                && off == 0.0
                && (n(&res, "paraBottom") > lim || square(d, p, start, lim, false, f64::NAN, ctx)?)
            {
                split(
                    d,
                    p,
                    start - nz(&g, "spaceBefore"),
                    lim_own,
                    body_h,
                    styles,
                    false,
                    Some((page_notes + own.len() as f64, page_foot + own_sum, foot)),
                    ctx,
                    state,
                )?
            } else {
                None
            };
            let table = if rem.is_some() && state.split.whole {
                tf::m0_objs(d, p, ctx)?
            } else {
                None
            };
            let vo = table
                .map(|t| num(d, child(d, t, "pos"), "vertOffset", 0.0))
                .unwrap_or(0.0);
            let om = table.and_then(|t| child(d, t, "outMargin"));
            let fits = if let Some(t) = table {
                vo > 0.0
                    && vo < 2147483648.0
                    && num(d, om, "top", 0.0) + table_height(d, t, ctx)? + num(d, om, "bottom", 0.0)
                        <= body_h
            } else {
                false
            };
            if fits {
                let top = start - nz(&g, "spaceBefore") + vo;
                zones.push(json!({"top":top,"bottom":max(body_h+1.0,top+1.0),"left":-1e9,"right":1e9,"flow":"BOTH_SIDES","carry":rem.map(Value::from).unwrap_or(Value::Undefined)}));
                float_floor = 0.0;
                cursor = n(&res, "textBottom")
            } else if rem.is_some() && state.split.moved {
                let rem = rem.unwrap();
                cursor = 0.0;
                float_floor = rem;
                zones.clear();
                split_page = true;
                let own = if buh { segments(d, p) } else { Vec::new() };
                if let Some(&first) = own.first() {
                    let delta = rem - num(d, Some(first), "vertpos", 0.0);
                    for sg in own {
                        put(d, sg, "vertpos", num(d, Some(sg), "vertpos", 0.0) + delta)
                    }
                    cursor = n(&res, "textBottom") + delta;
                    float_floor = 0.0
                }
            } else if body_h > 0.0 && off == 0.0 && square(d, p, start, lim, true, area_w, ctx)? {
                let rem = split(d, p, start, lim, body_h, styles, true, None, ctx, state)?;
                let own = segments(d, p);
                let sq = tf::m0_objs(d, p, ctx)?;
                let sq_pos = sq.and_then(|t| child(d, t, "pos"));
                let sq_vo = num(d, sq_pos, "vertOffset", 0.0);
                let sq_top = start
                    + max(
                        0.0,
                        if sq_vo >= 2147483648.0 {
                            sq_vo - 4294967296.0
                        } else {
                            sq_vo
                        },
                    )
                    + num(d, sq.and_then(|t| child(d, t, "outMargin")), "top", 0.0);
                if let Some(rem) = rem {
                    if sq.is_some()
                        && !own.is_empty()
                        && own.iter().all(|&sg| {
                            num(d, Some(sg), "vertpos", 0.0) + num(d, Some(sg), "vertsize", 0.0)
                                <= sq_top
                        })
                    {
                        cursor = 0.0;
                        float_floor = rem;
                        zones.clear();
                        split_page = true
                    } else if let Some(&first) = own.first() {
                        let delta = rem - num(d, Some(first), "vertpos", 0.0);
                        for sg in own {
                            put(d, sg, "vertpos", num(d, Some(sg), "vertpos", 0.0) + delta)
                        }
                        cursor = n(&res, "textBottom") + delta;
                        zones.clear();
                        split_page = true
                    }
                }
            } else if body_h > 0.0
                && off == 0.0
                && tf::band_table_over(d, p, start - nz(&g, "spaceBefore"), lim, ctx)?
            {
                let rem = split(
                    d,
                    p,
                    start - nz(&g, "spaceBefore"),
                    if buh { lim_own } else { lim },
                    body_h,
                    styles,
                    true,
                    if buh {
                        Some((page_notes + own.len() as f64, page_foot + own_sum, foot))
                    } else {
                        None
                    },
                    ctx,
                    state,
                )?;
                if let Some(rem) = rem {
                    cursor = 0.0;
                    float_floor = rem;
                    zones.clear();
                    split_page = true;
                    let own = if buh && state.split.moved {
                        segments(d, p)
                    } else {
                        Vec::new()
                    };
                    if let Some(&first) = own.first() {
                        let delta = rem - num(d, Some(first), "vertpos", 0.0);
                        for sg in own {
                            put(d, sg, "vertpos", num(d, Some(sg), "vertpos", 0.0) + delta)
                        }
                        cursor = n(&res, "textBottom") + delta;
                        float_floor = 0.0
                    }
                }
            }
        }
        let keep_cursor = hidden && !explicit;
        hid_prev = if hidden { hid_prev + 1 } else { 0 };
        prev_placed = if off == 0.0 && !(explicit || first_over || orphan) {
            Some(Previous {
                p,
                start,
                top: nz(&g, "spaceBefore"),
                h: para_h,
                space_after: n(&res, "spaceAfter"),
                keep: truth(&g["keepWithNext"]),
            })
        } else {
            None
        };
        let moved = explicit || first_over || orphan || off != 0.0;
        page_limit = if reserve > 0.0 {
            body_h - reserve
        } else if moved {
            body_h
        } else {
            min(
                page_limit,
                if reserve > 0.0 {
                    body_h - reserve
                } else {
                    page_limit
                },
            )
        };
        if moved
            && !refs.is_empty()
            && cut.is_some_and(|c| c > 0)
            && !orphan
            && !explicit
            && !first_over
        {
            let cut = cut.unwrap();
            let last = lines[cut - 1];
            let from_kept = num(d, Some(last), "textpos", 0.0);
            let cut_tp = num(d, Some(lines[cut]), "textpos", 0.0);
            let kept = refs
                .iter()
                .filter(|r| r[1] >= from_kept && r[1] < cut_tp)
                .collect::<Vec<_>>();
            let before = refs.iter().filter(|r| r[1] < from_kept).collect::<Vec<_>>();
            let sum = before.iter().chain(kept.iter()).fold(0.0, |a, r| a + r[0]);
            let pushed = !kept.is_empty()
                && cut > 1
                && line_bottom(d, last)
                    > body_h
                        - foot.area(
                            page_notes + before.len() as f64 + kept.len() as f64,
                            page_foot + sum,
                        );
            let from = if pushed { from_kept } else { cut_tp };
            let mv = refs.iter().filter(|r| r[1] >= from).collect::<Vec<_>>();
            page_notes = mv.len() as f64;
            page_foot = mv.iter().fold(0.0, |a, r| a + r[0]);
            page_notes += carry_notes;
            page_foot += carry_foot;
            carry_notes = 0.0;
            carry_foot = 0.0;
        } else if moved {
            let previous = if off != 0.0 && cut.is_some_and(|c| c > 0) {
                let i = lines[cut.unwrap() - 1];
                num(d, Some(i), "vertpos", 0.0) + num(d, Some(i), "vertsize", 0.0)
            } else {
                content_bottom
            };
            let excess = foot.area(page_notes, page_foot) - (body_h - previous);
            if excess > 0.0 && page_notes > 0.0 {
                carry_notes += 1.0;
                carry_foot += excess
            }
            page_notes = fns.len() as f64 + carry_notes;
            page_foot = fn_sum + carry_foot;
            carry_notes = 0.0;
            carry_foot = 0.0;
        } else if split_page {
            page_notes = state.split.notes_n;
            page_foot = state.split.notes_s;
            carry_notes = 0.0;
            carry_foot = 0.0;
        } else {
            let last = lines.last().copied();
            let last_refs = if lines.len() > 1 {
                let tp = num(d, last, "textpos", 0.0);
                refs.iter().filter(|r| r[1] >= tp).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let sum = last_refs.iter().fold(0.0, |a, r| a + r[0]);
            if !last_refs.is_empty()
                && line_bottom(d, last.unwrap())
                    > body_h - foot.area(page_notes + fns.len() as f64, page_foot + fn_sum)
            {
                page_notes += fns.len() as f64 - last_refs.len() as f64;
                page_foot += fn_sum - sum;
                carry_notes += last_refs.len() as f64;
                carry_foot += sum
            } else {
                page_notes += fns.len() as f64;
                page_foot += fn_sum
            }
        }
        let last = segments(d, p).last().copied();
        let lb = last
            .map(|i| num(d, Some(i), "vertpos", 0.0) + num(d, Some(i), "vertsize", 0.0))
            .unwrap_or(0.0);
        content_bottom = if split_page {
            float_floor
        } else if keep_cursor {
            content_bottom
        } else {
            max(
                lb,
                if n(&res, "floatFloor") > 0.0 && off == 0.0 {
                    float_floor
                } else {
                    0.0
                },
            )
        };
        split_page = false;
        if keep_cursor {
            cursor = cursor_before
        } else {
            prev_space = n(&res, "spaceAfter")
        }
        counter["n"] = (n(counter, "n") + 1.0).into();
    }
    Ok(())
}

pub fn flow_end_notes(
    d: &mut ParsedXml,
    root: usize,
    body_h: f64,
    ctx: &mut Context,
) -> Result<(), String> {
    let mut notes = Vec::new();
    let mut last = None;
    for &p in &d.nodes[root].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        if let Some(&seg) = segments(d, p).last() {
            last = Some(seg)
        }
        for &r in &d.nodes[p].children {
            if d.nodes[r].local != "run" {
                continue;
            }
            for &c in &d.nodes[r].children {
                if d.nodes[c].local != "ctrl" {
                    continue;
                }
                if let Some(en) = child(d, c, "endNote")
                    && !ctx.is_deleted_control(en)
                    && let Some(sub) = child(d, en, "subList")
                {
                    notes.push(sub)
                }
            }
        }
    }
    let Some(last) = last.filter(|_| !notes.is_empty()) else {
        return Ok(());
    };
    let pr = pm::find_first(d, root, "endNotePr", 0);
    let sp = pr.and_then(|p| child(d, p, "noteSpacing"));
    let mut y = num(d, Some(last), "vertpos", 0.0)
        + num(d, Some(last), "vertsize", 0.0)
        + num(d, sp, "aboveLine", 0.0)
        + num(d, sp, "belowLine", 567.0);
    let mut pend = 0.0;
    for (ni, sub) in notes.into_iter().enumerate() {
        if ni > 0 {
            y += num(d, sp, "betweenNotes", 283.0)
        }
        let (mut base, mut off, mut shift) = (y, y, 0.0);
        let mut forced: Option<(f64, f64)> = None;
        let floats = tf::sub_float_bottoms(d, sub, ctx)?;
        let mut done = HashSet::new();
        for p in d.nodes[sub].children.clone() {
            if d.nodes[p].local != "p" {
                continue;
            }
            for seg in segments(d, p) {
                let raw0 = num(d, Some(seg), "vertpos", 0.0);
                let vs = num(d, Some(seg), "vertsize", 0.0);
                for (i, f) in floats.iter().enumerate() {
                    if done.contains(&i) || raw0 < n(f, "bottom") {
                        continue;
                    }
                    done.insert(i);
                    if !(base + n(f, "lineBottom") - shift <= body_h
                        && base + n(f, "bottom") - shift > body_h)
                    {
                        continue;
                    }
                    let rows = list(f, "rows");
                    if !rows.is_empty() && attr(d, id(&f["el"]), "pageBreak", "") == "CELL" {
                        let mut yy = base + n(f, "bottom") - n(f, "carry") - shift + n(f, "outT");
                        let mut k = 0;
                        while k < rows.len() && yy + number(&rows[k]) <= body_h {
                            yy += number(&rows[k]);
                            k += 1
                        }
                        if k == rows.len() {
                            continue;
                        }
                        forced = Some((
                            n(f, "bottom"),
                            n(f, "outT")
                                + rows[k..].iter().fold(0.0, |a, b| a + number(b))
                                + n(f, "outB"),
                        ));
                        continue;
                    }
                    shift += n(f, "bottom") - n(f, "hostEnd");
                    pend = max(pend, n(f, "carry"));
                }
                let raw = raw0 - shift;
                let mut v = base + raw;
                if let Some((at, rem)) = forced.filter(|(at, _)| raw0 >= *at) {
                    base = rem - at + shift;
                    v = base + raw;
                    off = 0.0;
                    forced = None
                } else if v + vs > body_h && v > 0.0 {
                    base = pend - raw;
                    v = pend;
                    off = 0.0;
                    pend = 0.0
                }
                put(d, seg, "vertpos", v - off);
                y = v + vs;
            }
        }
    }
    Ok(())
}
pub fn reflow_section(
    d: &mut ParsedXml,
    root: usize,
    styles: &Value,
    geom: &Value,
    mode: &str,
    ctx: &mut Context,
    state: &mut BlockState,
) -> Result<f64, String> {
    let mut end_w = Value::Undefined;
    let body_w = n(geom, "BODY_W");
    for &p in &d.nodes[root].children {
        if d.nodes[p].local == "p" {
            let w = tf::column_width_in(d, p, body_w);
            if !matches!(w, Value::Null | Value::Undefined) {
                end_w = w
            }
        }
    }
    let mut counter = FlowCounter::new(body_w);
    counter.fields["endNoteW"] = end_w;
    reflow_block_flow(
        d,
        root,
        styles,
        body_w,
        mode,
        &mut counter,
        n(geom, "BODY_H"),
        ctx,
        state,
    )?;
    if n(geom, "BODY_H") > 0.0 {
        flow_end_notes(d, root, n(geom, "BODY_H"), ctx)?
    }
    Ok(n(&counter.fields, "n"))
}
#[cfg(test)]
fn snapshot(d: &ParsedXml, i: usize) -> Value {
    json!({"name":d.nodes[i].name,"attrs":d.nodes[i].attributes().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"children":d.nodes[i].children.iter().map(|&i|snapshot(d,i)).collect::<Vec<_>>()})
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    let mut d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let mut roots = vec![d.root];
    for x in list(&input, "documents") {
        roots.push(d.import_document(parse_style_xml(
            x.as_str().ok_or("추가 문서가 문자열이 아닙니다.")?,
        )?)?)
    }
    let styles = if let Some(xml) = input["header"].as_str() {
        head_styles::parse_render_styles(xml, input.get("styleOpts"))?
    } else {
        input["styles"].clone()
    };
    let mut ctx = Context::default();
    if truth(&input["prepare"]) {
        for &r in &roots {
            pm::prepare_deleted_ranges(&d, r, 0, &mut ctx)
        }
    }
    let mut state = BlockState::default();
    let mut counter = FlowCounter {
        fields: input
            .get("counter")
            .cloned()
            .unwrap_or_else(|| json!({"n":0,"bodyW":input["areaW"]})),
        nums: CounterState::from_value(&input["counter"]["nums"])?,
    };
    let steps = if operation == "sequence" {
        list(&input, "steps").to_vec()
    } else {
        vec![input.clone()]
    };
    let mut output = Vec::new();
    for step in steps {
        let mut p = *roots
            .get(step["document"].as_u64().unwrap_or(0) as usize)
            .ok_or("문서 번호가 올바르지 않습니다.")?;
        for path in list(&step, "path") {
            p = *d.nodes[p]
                .children
                .get(id(path).ok_or("노드 경로 오류")?)
                .ok_or("노드 경로 범위 오류")?
        }
        let op = step["operation"].as_str().unwrap_or(operation);
        let mode = step["mode"].as_str().unwrap_or("keep");
        let result = match op {
            "reflowBlockFlow" => reflow_block_flow(
                &mut d,
                p,
                &styles,
                n(&step, "areaW"),
                mode,
                &mut counter,
                n(&step, "bodyH"),
                &mut ctx,
                &mut state,
            )
            .map(|_| Value::Null),
            "reflowSection" => reflow_section(
                &mut d,
                p,
                &styles,
                &step["geom"],
                mode,
                &mut ctx,
                &mut state,
            )
            .map(Value::from),
            "flowEndNotes" => {
                flow_end_notes(&mut d, p, n(&step, "bodyH"), &mut ctx).map(|_| Value::Null)
            }
            _ => return Err("알 수 없는 문단 흐름 연산입니다.".into()),
        };
        let value = match result {
            Ok(v) => json!({"ok":true,"result":v}),
            Err(_) => json!({"ok":false}),
        };
        output.push(json!({"value":value,"counter":counter.to_value(),"split":state.split.to_value(),"documents":roots.iter().map(|&r|snapshot(&d,r)).collect::<Vec<_>>()}));
    }
    Ok(json!(output).to_json())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_budget_checks_depth_and_work_boundaries() {
        assert!(budget(2, 3, 2, 3).is_ok());
        assert!(budget(3, 3, 2, 3).is_err());
        assert!(budget(2, 4, 2, 3).is_err());
    }
    #[test]
    fn cloned_number_arrays_drop_non_index_properties_like_slice() {
        let original = CounterState::from_value(
            &json!([["n",{"length":2,"entries":[["0",3],["extra",9],["-1",7]]}]]),
        )
        .unwrap();
        let cloned = copy_nums(&original).unwrap().to_value();
        assert_eq!(cloned[0][1]["length"], 2);
        assert_eq!(cloned[0][1]["entries"], json!([["0", 3]]));
    }
    #[test]
    fn deleted_endnotes_leave_saved_positions_unchanged() {
        let mut d=parse_style_xml("<sec><p><run><ctrl><endNote><subList><p><linesegarray><lineseg vertpos='77' vertsize='500'/></linesegarray></p></subList></endNote></ctrl></run><linesegarray><lineseg vertpos='100' vertsize='1000'/></linesegarray></p></sec>").unwrap();
        let note = d.nodes.iter().position(|n| n.local == "endNote").unwrap();
        let mut ctx = Context::default();
        ctx.deleted_controls.insert(note, true);
        let before = snapshot(&d, d.root);
        let root = d.root;
        flow_end_notes(&mut d, root, 2000.0, &mut ctx).unwrap();
        assert_eq!(snapshot(&d, d.root), before);
    }
}
