//! kordoc svg-render.mjs table rendering, MIT, Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE and PROVENANCE. DOM edits deliberately remain on error.
#[cfg(test)]
use super::style_dom::parse_style_xml;
#[cfg(test)]
use super::svg_para::ObjectArgs;
use super::{
    fill_paint,
    layout::{self, maximum as max, minimum as min},
    layout_text::{LayoutText, TextRef},
    numbering::{add, truth},
    object_anchor,
    para_model::{self as pm, Context, child, num},
    shape_geometry,
    style_dom::ParsedXml,
    svg_para::{self, DrawObjects, RenderContext},
    table_flow,
    table_pages::{self, FloatSpan, PlanOptions, RowPiece},
    value::{Map, Value, native_json as json, number, string},
};

fn n(v: &Value, k: &str) -> f64 {
    number(&v[k])
}
fn nullish(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null)
}
fn id(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or("XML 노드 번호 오류".into())
}
fn idx(v: f64, len: usize) -> Result<usize, String> {
    if v >= 0.0 && v.fract() == 0.0 && v < (len as f64) {
        Ok(v as usize)
    } else {
        Err("표 행·열 위치 오류".into())
    }
}
fn at(a: &[f64], v: f64) -> f64 {
    idx(v, a.len()).map_or(f64::NAN, |i| a[i])
}
fn round(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 || v.abs() >= 4503599627370496.0 {
        v
    } else if (-0.5..0.0).contains(&v) {
        -0.0
    } else {
        let f = v.floor();
        if v - f < 0.5 { f } else { f + 1.0 }
    }
}
fn pt(v: &Value) -> String {
    string(&(round(number(v)) / 100.0).into())
}
fn escaped(v: &Value) -> Result<LayoutText, String> {
    Ok(LayoutText::from(v.as_str().ok_or("XML 속성값이 문자열이 아닙니다.")?).escape_xml())
}
fn get<'a>(styles: &'a Value, key: &str, id: &Value) -> &'a Value {
    styles[key]
        .as_array()
        .and_then(|a| a.iter().rev().find(|e| e[0] == *id))
        .map_or(&Value::Undefined, |v| &v[1])
}
fn children(d: &ParsedXml, p: usize, name: &str) -> Vec<usize> {
    d.nodes[p]
        .children
        .iter()
        .copied()
        .filter(|&i| d.nodes[i].local == name)
        .collect()
}
fn segments(d: &ParsedXml, p: usize) -> Vec<usize> {
    child(d, p, "linesegarray")
        .map(|a| children(d, a, "lineseg"))
        .unwrap_or_default()
}
fn put(d: &mut ParsedXml, i: usize, key: &str, v: &Value) {
    d.nodes[i].set_attr(key, string(v));
}
fn group(id: &str, kind: &str, page: f64) -> LayoutText {
    format!(
        "<g data-kordoc-id=\"{id}\" data-kordoc-type=\"{kind}\" data-kordoc-page=\"{}\">",
        string(&page.into())
    )
    .into()
}

pub fn edge_line(
    x1: &Value,
    y1: &Value,
    x2: &Value,
    y2: &Value,
    e: &Value,
) -> Result<LayoutText, String> {
    let ty = string(&e["type"]);
    let dash = if ty.contains("DASH") || ty.contains("DOT") {
        if !matches!(e["type"], Value::String(_)) {
            return Err("테두리 종류가 문자열이 아닙니다.".into());
        }
        format!(
            " stroke-dasharray=\"{}\"",
            if ty.contains("DOT") { "1,1.5" } else { "3,1.5" }
        )
    } else {
        String::new()
    };
    let Value::Number(width) = e["widthPt"] else {
        return Err("테두리 굵기가 숫자가 아닙니다.".into());
    };
    let mut out = LayoutText::from(format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"",
        pt(x1),
        pt(y1),
        pt(x2),
        pt(y2)
    ));
    out.append(&escaped(&e["color"])?.as_text_ref());
    out.append(&TextRef::Utf8(&format!(
        "\" stroke-width=\"{}\"{dash}/>",
        shape_geometry::fixed2(width)
    )));
    Ok(out)
}

/// Same memo is threaded through nested float callbacks; no cache snapshots.
pub fn table_cont_pages(
    d: &ParsedXml,
    tbl: usize,
    opts: PlanOptions,
    ctx: &mut Context,
) -> Result<FloatSpan, String> {
    table_cont_pages_at(d, tbl, opts, ctx, 0)
}
fn plan_with_floats(
    d: &ParsedXml,
    tbl: usize,
    cells: &[Value],
    row_h: &[f64],
    opts: PlanOptions,
    ctx: &mut Context,
    depth: usize,
) -> Result<Vec<Vec<RowPiece>>, String> {
    if depth > 64 {
        return Err("중첩 표 분할 처리 한도를 넘었습니다.".into());
    }
    let mut cb = |seg, top, cont_mt, ctx: &mut Context| {
        cell_float_span_at(
            d,
            seg,
            top,
            cont_mt,
            opts.mt,
            opts.body_h,
            opts.slack,
            ctx,
            depth + 1,
        )
    };
    table_pages::plan_table_rows(d, tbl, cells, row_h, opts, ctx, Some(&mut cb))
}
fn table_cont_pages_at(
    d: &ParsedXml,
    tbl: usize,
    opts: PlanOptions,
    ctx: &mut Context,
    depth: usize,
) -> Result<FloatSpan, String> {
    let cells = pm::collect_cells(d, tbl);
    let rows = pm::measure_table_row_heights(d, tbl, ctx)?;
    let plan = plan_with_floats(d, tbl, &cells, &rows, opts, ctx, depth)?;
    let mut last = (0.0, opts.ty, 0.0);
    let mut first = true;
    for p in plan.iter().flatten() {
        if first || p.page > last.0 || p.page == last.0 && p.y + p.h > last.1 + last.2 {
            last = (p.page, p.y, p.h);
            first = false;
        }
    }
    Ok(FloatSpan {
        el: None,
        pages: last.0,
        end_y: last.1 + last.2,
    })
}
#[allow(clippy::too_many_arguments)]
pub fn cell_float_span(
    d: &ParsedXml,
    seg: usize,
    top: f64,
    cont_mt: f64,
    mt: f64,
    body_h: f64,
    slack: f64,
    ctx: &mut Context,
) -> Result<Option<FloatSpan>, String> {
    cell_float_span_at(d, seg, top, cont_mt, mt, body_h, slack, ctx, 0)
}
#[allow(clippy::too_many_arguments)]
fn cell_float_span_at(
    d: &ParsedXml,
    seg: usize,
    top: f64,
    cont_mt: f64,
    mt: f64,
    body_h: f64,
    slack: f64,
    ctx: &mut Context,
    depth: usize,
) -> Result<Option<FloatSpan>, String> {
    if depth > 64 {
        return Err("중첩 표 분할 처리 한도를 넘었습니다.".into());
    }
    let Some(lsa) = d.nodes[seg].parent else {
        return Ok(None);
    };
    let Some(p) = d.nodes[lsa].parent.filter(|&p| d.nodes[p].local == "p") else {
        return Ok(None);
    };
    let segs = children(d, lsa, "lineseg");
    let k = segs.iter().position(|&s| s == seg);
    let m = pm::build_para(d, p, ctx)?;
    let mut best = None;
    for o in m["objs"].as_array().ok_or("문단 개체 배열 없음")? {
        if o["tag"] != "tbl" || truth(&o["inline"]) {
            continue;
        }
        let el = id(&o["el"])?;
        let pos = child(d, el, "pos");
        if pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA" {
            continue;
        }
        let ms = m["segs"].as_array().ok_or("문단 줄 배열 없음")?;
        let sk = ms
            .iter()
            .enumerate()
            .position(|(i, s)| {
                n(s, "textpos") <= n(o, "index")
                    && (i == ms.len() - 1 || n(&ms[i + 1], "textpos") > n(o, "index"))
            })
            .unwrap_or(0);
        if k != Some(sk) {
            continue;
        }
        let sv = num(d, Some(seg), "vertpos", 0.0);
        let styles =
            json!({"paraGeom":ctx.para_geom.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>()});
        let pt = svg_para::cell_para_top(d, p, &styles);
        let start = if object_anchor::line_pushed_by(d, o, &sv.into(), &pt)? {
            top - (sv - if nullish(&pt) { sv } else { number(&pt) })
        } else {
            top
        };
        let ty = start
            + num(d, child(d, el, "outMargin"), "top", 0.0)
            + max(0.0, num(d, pos, "vertOffset", 0.0));
        let mut span = table_cont_pages_at(
            d,
            el,
            PlanOptions {
                ty,
                mt,
                body_h,
                slack,
                nested: true,
                cont_mt,
            },
            ctx,
            depth + 1,
        )?;
        if span.pages > 0.0
            && best
                .as_ref()
                .is_none_or(|b: &FloatSpan| span.pages > b.pages)
        {
            span.el = Some(el);
            best = Some(span)
        }
    }
    Ok(best)
}

#[derive(Clone)]
struct Geometry {
    c: Value,
    page: Value,
    x: Value,
    y: Value,
    w: f64,
    h: f64,
    pieces: Option<Vec<RowPiece>>,
}
fn row_piece(page: f64, y: f64, h: f64) -> RowPiece {
    RowPiece {
        page,
        y,
        h,
        range: None,
        place: None,
        fill: None,
    }
}
fn piece_range(p: &RowPiece, el: usize) -> Option<&Value> {
    p.range.as_ref().and_then(|m| m.get(&el))
}
fn piece_of(pieces: &[RowPiece], el: usize, v: f64) -> usize {
    pieces
        .iter()
        .position(|p| piece_range(p, el).is_some_and(|r| v >= number(&r[0]) && v < number(&r[1])))
        .unwrap_or(0)
}
fn from(p: &RowPiece, el: usize) -> f64 {
    piece_range(p, el).map_or(0.0, |r| number(&r[0]))
}

#[allow(clippy::too_many_arguments)]
pub fn draw_table<R: DrawObjects>(
    d: &mut ParsedXml,
    tbl: usize,
    tx: Value,
    ty: Value,
    ctx: &mut RenderContext,
    depth: usize,
    renderer: &mut R,
) -> Result<(), String> {
    if depth > 16 {
        ctx.warn_once("depth", "중첩 깊이 16 초과 — 이하 생략");
        return Ok(());
    }
    ctx.stats["tables"] = (n(&ctx.stats, "tables") + 1.0).into();
    let cells = pm::collect_cells(d, tbl);
    if cells.is_empty() || cells.len() > 4096 {
        return Ok(());
    }
    let nc = cells
        .iter()
        .map(|c| n(c, "ca") + n(c, "cs"))
        .fold(f64::NEG_INFINITY, max);
    let nr = cells
        .iter()
        .map(|c| n(c, "ra") + n(c, "rs"))
        .fold(f64::NEG_INFINITY, max);
    if !nc.is_finite() || nc < 0.0 || nc.fract() != 0.0 || nc > 100000.0 {
        return Err("표 열 수 오류".into());
    }
    let col_cons = cells
        .iter()
        .map(|c| layout::Constraint {
            a: n(c, "ca"),
            b: n(c, "ca") + n(c, "cs"),
            size: n(c, "w"),
        })
        .collect::<Vec<_>>();
    let tw = num(d, child(d, tbl, "sz"), "width", 0.0);
    let mut row_sum = Vec::<(f64, f64, f64)>::new();
    for c in &cells {
        let ra = n(c, "ra");
        if let Some((_, cover, w)) = row_sum.iter_mut().find(|(r, _, _)| *r == ra) {
            *cover += n(c, "cs");
            *w += n(c, "w")
        } else {
            row_sum.push((ra, n(c, "cs"), n(c, "w")))
        }
    }
    let no_exact = !row_sum
        .iter()
        .any(|(_, cover, w)| *cover == nc && (*w - tw).abs() <= 2.0);
    let sp = num(d, Some(tbl), "cellSpacing", 0.0);
    let total = if tw != 0.0 && sp > 0.0 {
        tw - sp * (nc + 1.0)
    } else {
        tw
    };
    let col_x = layout::table_column_boundaries(
        &col_cons,
        nc as usize,
        if total != 0.0 && !total.is_nan() {
            Some(total)
        } else {
            None
        },
        no_exact,
    );
    let row_h = pm::measure_table_row_heights(d, tbl, &mut ctx.extent)?;
    let mut row_y = vec![0.0];
    if nr > 100000.0 {
        return Err("표 행 수 처리 한도 초과".into());
    }
    for r in 0..(max(0.0, nr).ceil() as usize) {
        row_y.push(row_y[r] + row_h.get(r).copied().unwrap_or(f64::NAN))
    }
    let th = at(&row_y, nr);
    if number(&add(&ty, &th.into())) > ctx.page_h + 100.0 {
        ctx.warn_once(
            "table-split",
            "페이지에 걸친 표 — 분할 지오메트리가 없어 시작 페이지 영역만 region 으로 기록",
        )
    }
    let bbox = ctx.bbox_value(
        &tx,
        &ty,
        &(at(&col_x, nc) + if sp > 0.0 { sp * (nc + 1.0) } else { 0.0 }).into(),
        &th.into(),
    )?;
    let parent = ctx
        .parent_stack
        .last()
        .map_or(Value::Undefined, |v| v.clone().into());
    let source = d.nodes[tbl]
        .attr("id")
        .map_or(Value::Undefined, Value::from);
    let table_id = ctx
        .regions
        .add("table", bbox, &json!({"sourceId":source,"parentId":parent}));
    ctx.emit(group(&table_id, "table", ctx.page_no()))?;
    ctx.parent_stack.push(table_id);
    let start_page = ctx.page.clone();
    let mut hdr = 0.0;
    if d.nodes[tbl].attr("repeatHeader") == Some("1") {
        while hdr < nr
            && cells.iter().any(|c| n(c, "ra") == hdr)
            && cells
                .iter()
                .filter(|c| n(c, "ra") == hdr)
                .all(|c| id(&c["el"]).is_ok_and(|i| d.nodes[i].attr("header") == Some("1")))
        {
            hdr += 1.0
        }
    }
    let mt = n(&ctx.geom, "MT");
    let bh = n(&ctx.geom, "BODY_H");
    let slack = n(&ctx.geom, "PH") - mt - bh;
    let split_here = depth == 1
        || truth(&ctx.split_cell) && pm::attr(d, child(d, tbl, "pos"), "treatAsChar") != Some("1");
    let cont = if depth != 1 && !ctx.split_cont_mt.is_undefined() {
        number(&ctx.split_cont_mt)
    } else {
        mt
    };
    let plan = if split_here {
        plan_with_floats(
            d,
            tbl,
            &cells,
            &row_h,
            PlanOptions {
                ty: number(&ty),
                mt,
                body_h: bh,
                slack,
                nested: depth != 1,
                cont_mt: cont,
            },
            &mut ctx.extent,
            0,
        )?
    } else {
        row_h
            .iter()
            .enumerate()
            .map(|(r, &h)| vec![row_piece(0.0, number(&add(&ty, &row_y[r].into())), h)])
            .collect()
    };
    let saved_split = ctx.split_cell.clone();
    ctx.split_cell = false.into();
    for pc in plan.iter().flatten() {
        if let Some(place) = &pc.place {
            for (el, py) in place {
                ctx.obj_place
                    .insert(*el, json!({"page":add(&start_page,&pc.page.into()),"y":py}));
            }
        }
    }
    let mut cont_pages = Vec::<Value>::new();
    for pc in plan.iter().flatten() {
        if pc.page > 0.0 {
            let pg = add(&start_page, &pc.page.into());
            if !cont_pages.contains(&pg) {
                cont_pages.push(pg)
            }
        }
    }
    let max_page = cont_pages.iter().map(number).fold(number(&start_page), max);
    if max_page >= 500000.0 {
        return Err("출력 쪽 수 처리 한도 초과".into());
    }
    while (ctx.pages.len() as f64) <= max_page {
        ctx.pages.push(Vec::new())
    }
    let xw = |c: &Value| {
        let ca = n(c, "ca");
        let cs = n(c, "cs");
        (
            add(&add(&tx, &at(&col_x, ca).into()), &(sp * ca).into()),
            at(&col_x, min(ca + cs, nc)) - at(&col_x, ca) + sp * (cs - 1.0),
        )
    };
    let mut geometries = Vec::<Geometry>::new();
    for c in &cells {
        let ra = n(c, "ra");
        let rs = n(c, "rs");
        let pcs = &plan[idx(ra, plan.len())?];
        let (x, w) = xw(c);
        if rs == 1.0 && pcs.len() > 1 {
            for (k, pc) in pcs.iter().enumerate() {
                geometries.push(Geometry {
                    c: c.clone(),
                    page: add(&start_page, &pc.page.into()),
                    x: x.clone(),
                    y: pc.y.into(),
                    w,
                    h: pc.h,
                    pieces: Some(if k == 0 { pcs.clone() } else { Vec::new() }),
                })
            }
            continue;
        }
        if rs > 1.0 {
            let end = min(ra + rs, nr).trunc().max(0.0) as usize;
            let span = plan[(ra.trunc().max(0.0) as usize).min(plan.len())..end.min(plan.len())]
                .iter()
                .flatten()
                .collect::<Vec<_>>();
            let mut pages = Vec::new();
            for pc in &span {
                if !pages.contains(&pc.page) {
                    pages.push(pc.page)
                }
            }
            if pages.len() > 1 {
                let bs = table_pages::cell_line_boxes(d, c, Some(bh), &mut ctx.extent)?;
                let el = id(&c["el"])?;
                let groups = pages
                    .iter()
                    .enumerate()
                    .map(|(k, &pg)| {
                        let on = span.iter().filter(|p| p.page == pg).collect::<Vec<_>>();
                        let y = on.iter().map(|p| p.y).fold(f64::INFINITY, min);
                        let end = on.iter().map(|p| p.y + p.h).fold(f64::NEG_INFINITY, max);
                        let from = if k == 0 {
                            0.0
                        } else {
                            bs.iter()
                                .find(|b| b.part == k)
                                .map_or(f64::INFINITY, |b| b.origin)
                        };
                        let to = if k == pages.len() - 1 {
                            f64::INFINITY
                        } else {
                            bs.iter()
                                .find(|b| b.part == k + 1)
                                .map_or(f64::INFINITY, |b| b.top)
                        };
                        let mut p = row_piece(pg, y, end - y);
                        p.range = Some(Map::from_iter([(el, json!([from, to]))]));
                        p
                    })
                    .collect::<Vec<_>>();
                for (k, pc) in groups.iter().enumerate() {
                    geometries.push(Geometry {
                        c: c.clone(),
                        page: add(&start_page, &pc.page.into()),
                        x: x.clone(),
                        y: pc.y.into(),
                        w,
                        h: pc.h,
                        pieces: Some(if k == 0 { groups.clone() } else { Vec::new() }),
                    })
                }
                continue;
            }
        }
        let pc = pcs.first().ok_or("표 조각이 없습니다.")?;
        let h = if rs == 1.0 {
            pc.h
        } else {
            at(&row_y, min(ra + rs, nr)) - at(&row_y, ra)
        };
        geometries.push(Geometry {
            c: c.clone(),
            page: add(&start_page, &pc.page.into()),
            x,
            y: if split_here {
                pc.y.into()
            } else {
                add(&ty, &at(&row_y, ra).into())
            },
            w,
            h,
            pieces: None,
        });
    }
    let cont_base = if depth != 1 && !ctx.split_cont_mt.is_undefined() {
        ctx.split_cont_mt.clone()
    } else {
        ctx.geom["MT"].clone()
    };
    for page in &cont_pages {
        for c in cells.iter().filter(|c| n(c, "ra") < hdr) {
            let (x, w) = xw(c);
            geometries.push(Geometry {
                c: c.clone(),
                page: page.clone(),
                x,
                w,
                y: add(
                    &add(
                        &cont_base,
                        &num(d, child(d, tbl, "outMargin"), "top", 0.0).into(),
                    ),
                    &at(&row_y, n(c, "ra")).into(),
                ),
                h: at(&row_y, min(n(c, "ra") + n(c, "rs"), nr)) - at(&row_y, n(c, "ra")),
                pieces: None,
            })
        }
    }
    if sp > 0.0 {
        for g in &mut geometries {
            g.y = add(&g.y, &sp.into());
            g.h -= sp
                + if n(&g.c, "ra") + n(&g.c, "rs") >= nr {
                    sp
                } else {
                    0.0
                }
        }
    }
    for g in &geometries {
        ctx.page = g.page.clone();
        let bf = get(&ctx.styles, "borderFill", &g.c["bfId"]).clone();
        if truth(&bf["fill"]) || truth(&bf["gradient"]) {
            let output = fill_paint::paint_fill_value(
                &bf,
                &g.x,
                &g.y,
                &g.w.into(),
                &g.h.into(),
                &mut ctx.defs,
            )?;
            ctx.emit(output.into())?;
        }
    }
    for g in &geometries {
        let c = &g.c;
        let Some(sub) = c["sub"].as_u64().map(|i| i as usize) else {
            continue;
        };
        if g.pieces.as_ref().is_some_and(Vec::is_empty) {
            continue;
        }
        ctx.page = g.page.clone();
        if let Some(pieces) = &g.pieces {
            let base = pieces[0].y + n(c, "marginT");
            let el = id(&c["el"])?;
            let boxes = table_pages::cell_line_boxes(d, c, Some(bh), &mut ctx.extent)?;
            let va = d.nodes[sub].attr("vertAlign").unwrap_or("TOP").to_owned();
            let last = pieces.len() - 1;
            let mut offsets = Vec::new();
            for (k, pc) in pieces.iter().enumerate() {
                if va == "TOP" || k > 0 && k < last || pc.fill == Some(true) {
                    offsets.push(0.0);
                    continue;
                }
                let from = from(pc, el);
                let mut bottoms = boxes
                    .iter()
                    .filter(|b| piece_of(pieces, el, b.top) == k)
                    .map(|b| b.bottom - from)
                    .collect::<Vec<_>>();
                if k > 0 {
                    for f in pm::without_extent_memo(&mut ctx.extent, |ctx| {
                        table_flow::sub_float_bottoms(d, sub, ctx)
                    })? {
                        if let Some(b) = boxes
                            .iter()
                            .find(|b| f["seg"].as_u64() == Some(b.seg as u64))
                            && piece_of(pieces, el, b.top) == k
                        {
                            bottoms.push(b.top + n(&f, "bottom") - n(&f, "v0") - from)
                        }
                    }
                }
                if bottoms.is_empty() {
                    offsets.push(0.0);
                    continue;
                }
                let free = pc.h
                    - n(c, "marginT")
                    - n(c, "marginB")
                    - bottoms.into_iter().fold(f64::NEG_INFINITY, max);
                offsets.push(max(0.0, if va == "CENTER" { free / 2.0 } else { free }));
            }
            for p in children(d, sub, "p") {
                let segs = segments(d, p);
                let saved = segs
                    .iter()
                    .map(|&s| d.nodes[s].attr("vertpos").map(str::to_owned))
                    .collect::<Vec<_>>();
                let mut pages = Vec::new();
                for &sg in &segs {
                    let v = boxes
                        .iter()
                        .rev()
                        .find(|b| b.seg == sg)
                        .map_or_else(|| num(d, Some(sg), "vertpos", 0.0), |b| b.top);
                    let k = piece_of(pieces, el, v);
                    let pc = &pieces[k];
                    put(
                        d,
                        sg,
                        "vertpos",
                        &(v - from(pc, el) + pc.y + n(c, "marginT") + offsets[k] - base).into(),
                    );
                    pages.push(add(&start_page, &pc.page.into()))
                }
                ctx.split_cell = split_here.into();
                let saved_mt = ctx.split_cont_mt.clone();
                let cv = if depth > 1 && !saved_mt.is_undefined() {
                    saved_mt.clone()
                } else {
                    ctx.geom["MT"].clone()
                };
                ctx.split_cont_mt = add(
                    &add(
                        &add(&cv, &num(d, child(d, tbl, "outMargin"), "top", 0.0).into()),
                        &at(&row_y, hdr).into(),
                    ),
                    &c["marginT"],
                );
                svg_para::draw_para_native(
                    d,
                    p,
                    add(&g.x, &c["marginL"]),
                    base.into(),
                    g.w - n(c, "marginL") - n(c, "marginR"),
                    ctx,
                    depth + 1,
                    Some(&pages),
                    renderer,
                )?;
                ctx.split_cont_mt = saved_mt;
                ctx.split_cell = false.into();
                for (sg, saved) in segs.into_iter().zip(saved) {
                    d.nodes[sg].set_attr("vertpos", saved.unwrap_or_else(|| "0".into()))
                }
            }
            continue;
        }
        let iw = g.w - n(c, "marginL") - n(c, "marginR");
        let mut wide = false;
        for p in children(d, sub, "p") {
            let m = pm::build_para(d, p, &mut ctx.extent)?;
            if m["objs"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|o| truth(&o["inline"]) && o["tag"] == "pic" && n(o, "width") > iw + 50.0)
            }) {
                wide = true;
                break;
            }
        }
        if wide {
            let cid = format!("cclip{}", ctx.defs.len());
            ctx.defs.push(format!("<clipPath id=\"{cid}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",pt(&g.x),pt(&g.y),pt(&g.w.into()),pt(&g.h.into())).into());
            ctx.emit(format!("<g clip-path=\"url(#{cid})\">").into())?
        }
        let ih = g.h - n(c, "marginT") - n(c, "marginB");
        let extent = pm::cell_content_extent(d, c, &mut ctx.extent)?;
        let va = d.nodes[sub].attr("vertAlign").unwrap_or("TOP");
        let off = if va == "CENTER" {
            max(0.0, (ih - extent) / 2.0)
        } else if va == "BOTTOM" {
            max(0.0, ih - extent)
        } else {
            0.0
        };
        let mut moved = Vec::new();
        if let Some(cols) = pm::cell_columns(d, Some(sub)) {
            let cw = (iw - n(&cols, "gap") * (n(&cols, "n") - 1.0)) / n(&cols, "n");
            let mut col = 0.0;
            let mut prev = f64::NEG_INFINITY;
            for p in children(d, sub, "p") {
                for sg in segments(d, p) {
                    let v = num(d, Some(sg), "vertpos", 0.0);
                    if v < prev {
                        col = min(n(&cols, "n") - 1.0, col + 1.0)
                    }
                    prev = v;
                    if col > 0.0 {
                        moved.push((sg, d.nodes[sg].attr("horzpos").map(str::to_owned)));
                        put(
                            d,
                            sg,
                            "horzpos",
                            &(num(d, Some(sg), "horzpos", 0.0)
                                + round(col * (cw + n(&cols, "gap"))))
                            .into(),
                        );
                    }
                }
            }
        }
        for p in children(d, sub, "p") {
            svg_para::draw_para_native(
                d,
                p,
                add(&g.x, &c["marginL"]),
                add(&add(&g.y, &c["marginT"]), &off.into()),
                iw,
                ctx,
                depth + 1,
                None,
                renderer,
            )?
        }
        for (sg, saved) in moved {
            d.nodes[sg].set_attr("horzpos", saved.unwrap_or_else(|| "0".into()))
        }
        if wide {
            ctx.page = g.page.clone();
            ctx.emit("</g>".into())?
        }
    }
    for g in &geometries {
        ctx.page = g.page.clone();
        let bf = get(&ctx.styles, "borderFill", &g.c["bfId"]).clone();
        if !truth(&bf) {
            continue;
        }
        for (edge, x1, y1, x2, y2) in [
            (
                "top",
                g.x.clone(),
                g.y.clone(),
                add(&g.x, &g.w.into()),
                g.y.clone(),
            ),
            (
                "bottom",
                g.x.clone(),
                add(&g.y, &g.h.into()),
                add(&g.x, &g.w.into()),
                add(&g.y, &g.h.into()),
            ),
            (
                "left",
                g.x.clone(),
                g.y.clone(),
                g.x.clone(),
                add(&g.y, &g.h.into()),
            ),
            (
                "right",
                add(&g.x, &g.w.into()),
                g.y.clone(),
                add(&g.x, &g.w.into()),
                add(&g.y, &g.h.into()),
            ),
        ] {
            if truth(&bf[edge]) {
                ctx.emit(edge_line(&x1, &y1, &x2, &y2, &bf[edge])?)?
            }
        }
    }
    ctx.page = start_page;
    ctx.split_cell = saved_split;
    ctx.parent_stack.pop();
    ctx.emit("</g>".into())?;
    Ok(())
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
fn select(d: &ParsedXml, path: &Value) -> Result<usize, String> {
    let mut p = d.root;
    if let Some(path) = path.as_array() {
        for v in path {
            p = *d.nodes[p].children.get(id(v)?).ok_or("XML 경로 오류")?
        }
    }
    Ok(p)
}
#[cfg(test)]
fn snap(d: &ParsedXml, i: usize) -> Value {
    json!({"name":d.nodes[i].name,"attrs":d.nodes[i].attributes().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"children":d.nodes[i].children.iter().map(|&i|snap(d,i)).collect::<Vec<_>>()})
}
#[cfg(test)]
fn state(ctx: &RenderContext) -> Value {
    json!({"page":ctx.page,"parentStack":ctx.parent_stack,"escOrigin":ctx.esc_origin,"splitCell":ctx.split_cell,"splitContMT":ctx.split_cont_mt,"stats":ctx.stats})
}
#[cfg(test)]
struct Recorder {
    calls: Vec<Value>,
    throw_at: Option<usize>,
    recurse: bool,
    updates: Vec<Value>,
}
#[cfg(test)]
impl DrawObjects for Recorder {
    fn draw_object(
        &mut self,
        d: &mut ParsedXml,
        a: ObjectArgs,
        ctx: &mut RenderContext,
    ) -> Result<(), String> {
        let at = self.calls.len();
        self.calls.push(json!({"object":a.object,"x":a.x,"y":a.y,"baseV":a.base_v,"areaW":a.area_w,"depth":a.depth,"state":state(ctx)}));
        if self.throw_at == Some(at) {
            return Err("합성 drawObject 실패".into());
        }
        if self.recurse && a.object["tag"] == "tbl" {
            draw_table(d, id(&a.object["el"])?, a.x, a.y, ctx, a.depth + 1, self)?
        }
        if let Some(u) = self.updates.get(at) {
            if let Some(v) = u.get("page") {
                ctx.page = v.clone()
            }
            if let Some(v) = u.get("escOrigin") {
                ctx.esc_origin = v.clone()
            }
            if let Some(v) = u.get("splitCell") {
                ctx.split_cell = v.clone()
            }
        }
        Ok(())
    }
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut i = Value::from_json(input);
    if let Some(patches) = input["nativeNumbers"].as_array() {
        for p in patches {
            let n = match p["value"].as_str() {
                Some("NaN") => f64::NAN,
                Some("Infinity") => f64::INFINITY,
                Some("-Infinity") => f64::NEG_INFINITY,
                Some("-0") => -0.0,
                _ => return Err("특수 숫자 오류".into()),
            };
            let mut v = &mut i;
            for k in p["path"].as_array().ok_or("특수 숫자 경로 오류")? {
                v = if let Some(k) = k.as_str() {
                    v.get_mut(k)
                } else {
                    v.get_mut(k.as_u64().ok_or("특수 숫자 키 오류")? as usize)
                }
                .ok_or("특수 숫자 값 없음")?
            }
            *v = n.into()
        }
    }
    if op == "edgeLine" {
        return Ok(report(&json!(
            edge_line(&i["x1"], &i["y1"], &i["x2"], &i["y2"], &i["edge"])?.0
        )));
    }
    let mut d = parse_style_xml(i["xml"].as_str().ok_or("xml 문자열 필요")?)?;
    let mut styles = i["styles"].clone();
    if let Some(header) = i["headerXml"].as_str() {
        styles = super::head_styles::parse_render_styles(header, None)?
    }
    let c = &i["context"];
    let count = c["pageCount"].as_u64().unwrap_or(1) as usize;
    if count > 500000 {
        return Err("출력 쪽 수 처리 한도 초과".into());
    }
    let mut ctx = RenderContext::new(styles, count);
    ctx.geom = c.get("geom").cloned().unwrap_or_else(
        || json!({"PW":20000,"PH":12000,"ML":1000,"MT":1000,"BODY_W":18000,"BODY_H":10000}),
    );
    if let Some(v) = c.get("page") {
        ctx.page = v.clone()
    }
    if let Some(v) = c.get("pageBase") {
        ctx.page_base = v.clone()
    }
    if let Some(v) = c.get("pageH") {
        ctx.page_h = number(v)
    }
    if let Some(v) = c["parentStack"].as_array() {
        ctx.parent_stack = v.iter().map(string).collect()
    }
    if let Some(v) = c["defs"].as_array() {
        ctx.defs = v.clone()
    }
    if let Some(v) = c["regionRotations"].as_array() {
        ctx.region_rotations = v.clone()
    }
    ctx.split_cell = c["splitCell"].clone();
    ctx.split_cont_mt = c["splitContMT"].clone();
    ctx.esc_origin = c["escOrigin"].clone();
    ctx.nums = super::para_number::CounterState::from_value(&c["nums"])?;
    if let Some(a) = c["highlights"].as_array() {
        for s in a {
            let s = string(s);
            let s = crate::input::trim(&s).to_lowercase();
            if !s.is_empty() {
                ctx.highlights.push(s.into())
            }
        }
    }
    ctx.extent.memoize = true;
    if let Some(a) = ctx.styles["paraGeom"].as_array() {
        for e in a {
            if let Some(k) = e[0].as_str() {
                ctx.extent.para_geom.insert(k.into(), e[1].clone());
            }
        }
    }
    pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx.extent);
    if op == "tableContPages" || op == "cellFloatSpan" {
        let target = select(&d, &i["path"])?;
        let mt = i.get("MT").map_or(1000.0, number);
        let bh = i.get("BODY_H").map_or(10000.0, number);
        let slack = i.get("slack").map_or(0.0, number);
        let cont = i.get("contMT").map_or(mt, number);
        let value = if op == "tableContPages" {
            let s = table_cont_pages(
                &d,
                target,
                PlanOptions {
                    ty: i.get("ty").map_or(0.0, number),
                    mt,
                    body_h: bh,
                    slack,
                    nested: truth(&i["nested"]),
                    cont_mt: cont,
                },
                &mut ctx.extent,
            )?;
            json!({"pages":s.pages,"endY":s.end_y})
        } else {
            match cell_float_span(
                &d,
                target,
                i.get("top").map_or(0.0, number),
                cont,
                mt,
                bh,
                slack,
                &mut ctx.extent,
            )? {
                Some(s) => json!({"pages":s.pages,"endY":s.end_y,"el":s.el}),
                None => Value::Undefined,
            }
        };
        return Ok(report(&value));
    }
    if op != "drawTable" && op != "sequence" {
        return Err("알 수 없는 표 그리기 연산".into());
    }
    let body_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
    if let Some(a) = c["bodyDx"].as_array() {
        let a = a.clone();
        let calls = body_calls.clone();
        ctx.body_dx = Some(Box::new(move |page| {
            calls.borrow_mut().push(page.clone());
            Ok(page
                .as_u64()
                .and_then(|p| a.get(p as usize))
                .cloned()
                .unwrap_or(Value::Undefined))
        }));
    }
    let mut renderer = Recorder {
        calls: Vec::new(),
        throw_at: i["throwAt"].as_u64().map(|i| i as usize),
        recurse: truth(&i["recurseTables"]),
        updates: i["updates"].as_array().cloned().unwrap_or_default(),
    };
    let draws = if op == "sequence" {
        i["draws"].as_array().ok_or("draws 배열 필요")?.clone()
    } else {
        vec![i["draw"].clone()]
    };
    let mut errors = Vec::new();
    for draw in draws {
        let tbl = select(&d, &draw["path"])?;
        let r = draw_table(
            &mut d,
            tbl,
            draw.get("tx").cloned().unwrap_or(0.into()),
            draw.get("ty").cloned().unwrap_or(0.into()),
            &mut ctx,
            draw["depth"].as_u64().unwrap_or(1) as usize,
            &mut renderer,
        );
        errors.push(r.err());
    }
    let pages = ctx
        .pages
        .iter()
        .map(|p| p.iter().map(|t| json!(t.0)).collect::<Value>())
        .collect::<Value>();
    let mut table_memo = ctx.extent.table_memo.iter().collect::<Vec<_>>();
    table_memo.sort_by_key(|(k, _)| **k);
    let mut cell_memo = ctx.extent.cell_memo.iter().collect::<Vec<_>>();
    cell_memo.sort_by_key(|(k, _)| **k);
    let mut placed = ctx.obj_place.iter().collect::<Vec<_>>();
    placed.sort_by_key(|(k, _)| **k);
    Ok(report(
        &json!({"pages":pages,"defs":ctx.defs,"regions":ctx.regions.regions,"nums":ctx.nums.to_value(),"warnings":ctx.warnings,"warned":ctx.warned_order,"state":state(&ctx),"calls":renderer.calls,"bodyCalls":body_calls.borrow().clone(),"errors":errors,"dom":snap(&d,d.root),"tableMemo":table_memo.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"cellMemo":cell_memo.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"objPlace":placed.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>()}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_decimal_tie_and_escaped_color() {
        let s = edge_line(
            &0.into(),
            &(-0.0).into(),
            &100.into(),
            &200.into(),
            &json!({"type":"DASH_DOT","widthPt":0.125,"color":"<&"}),
        )
        .unwrap()
        .to_string_strict()
        .unwrap();
        assert_eq!(
            s,
            "<line x1=\"0\" y1=\"0\" x2=\"1\" y2=\"2\" stroke=\"&lt;&amp;\" stroke-width=\"0.13\" stroke-dasharray=\"1,1.5\"/>"
        );
    }
}
