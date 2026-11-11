//! kordoc svg-render.mjs section orchestration, MIT, Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE and kordoc-PROVENANCE. Drawing stays UTF-16.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    fill_paint, head_styles,
    layout::{maximum, minimum},
    layout_text::{LayoutText, TextRef, has_control_text},
    metrics,
    numbering::{add, truth},
    object_anchor,
    page_stories::{self, ParaPages, Story},
    para_model::{self as pm, attr, child, find_first, num},
    para_number,
    reflow_block::{self, BlockState},
    style_dom::ParsedXml,
    svg_objects::PaintJob,
    svg_para::{self, RenderContext, draw_para, draw_para_native, pt},
    svg_renderer::Renderer,
    svg_table,
    table_pages::PlanOptions,
    value::{Map, Value, native_json as json, number, string},
};
use std::collections::HashMap;

pub struct SectionInput {
    pub root: usize,
    pub index: Value,
    pub masters: Vec<usize>,
    pub has_deletion_markers: bool,
}
pub struct RenderOptions {
    pub reflow: bool,
    pub reflow_mode: String,
    /// Already trimmed/lowercased, as at the original input boundary.
    pub highlights: Vec<LayoutText>,
}
pub struct RenderedSection {
    pub pages: Vec<Vec<LayoutText>>,
    pub pw: Value,
    pub page_h: Value,
    pub clip_id: String,
}
pub struct SectionRenderResult {
    pub sections: Vec<RenderedSection>,
    pub defs: Vec<Value>,
    pub warnings: Vec<String>,
    pub stats: Value,
    pub regions: Vec<Value>,
}
fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn lookup<'a>(v: &'a Value, key: &str) -> &'a Value {
    static UNDEFINED: Value = Value::Undefined;
    v.as_array()
        .and_then(|a| a.iter().find(|e| e[0].as_str() == Some(key)))
        .map(|e| &e[1])
        .unwrap_or(&UNDEFINED)
}
fn nn(v: &Value, fallback: Value) -> Value {
    if matches!(v, Value::Null | Value::Undefined) {
        fallback
    } else {
        v.clone()
    }
}
fn pars(d: &ParsedXml, root: usize) -> Vec<usize> {
    d.nodes[root]
        .children
        .iter()
        .copied()
        .filter(|&p| d.nodes[p].local == "p")
        .collect()
}
fn descendants(d: &ParsedXml, root: usize) -> Vec<usize> {
    let mut todo = d.nodes[root]
        .children
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    while let Some(n) = todo.pop() {
        out.push(n);
        todo.extend(d.nodes[n].children.iter().rev().copied())
    }
    out
}
fn segs(d: &ParsedXml, p: usize) -> Vec<usize> {
    child(d, p, "linesegarray")
        .map(|a| {
            d.nodes[a]
                .children
                .iter()
                .copied()
                .filter(|&s| d.nodes[s].local == "lineseg")
                .collect()
        })
        .unwrap_or_default()
}
fn n(d: &ParsedXml, p: usize, k: &str) -> f64 {
    num(d, Some(p), k, 0.0)
}
fn remove_lines(d: &mut ParsedXml, p: usize) -> Result<(), String> {
    if let Some(a) = child(d, p, "linesegarray") {
        d.remove_child(p, a)?
    }
    Ok(())
}
fn content(m: &Value) -> Result<bool, String> {
    if !list(m, "objs").is_empty() {
        return Ok(true);
    }
    for c in list(m, "chars") {
        if !crate::input::trim(c["ch"].as_str().unwrap_or("")).is_empty() || has_control_text(c)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn read_section_geom(d: &ParsedXml, root: usize) -> Value {
    let page = find_first(d, root, "pagePr", 0);
    let margin = page.and_then(|p| child(d, p, "margin"));
    let (mut pw, mut ph) = (
        num(d, page, "width", 59528.0),
        num(d, page, "height", 84188.0),
    );
    if attr(d, page, "landscape") == Some("NARROWLY") && pw < ph {
        std::mem::swap(&mut pw, &mut ph)
    }
    let gt = attr(d, page, "gutterType").unwrap_or("LEFT_ONLY");
    let gutter = num(d, margin, "gutter", 0.0);
    let gl = if gt == "TOP_BOTTOM" { 0.0 } else { gutter };
    let gtop = if gt == "TOP_BOTTOM" { gutter } else { 0.0 };
    let ml = num(d, margin, "left", 8504.0) + gl;
    let mt = num(d, margin, "top", 5668.0) + num(d, margin, "header", 0.0) + gtop;
    let bh = ph - mt - num(d, margin, "bottom", 4252.0) - num(d, margin, "footer", 0.0);
    let bw = pw - ml - num(d, margin, "right", 8504.0);
    let dx = if gt == "LEFT_RIGHT" {
        num(d, margin, "right", 8504.0) - ml
    } else {
        0.0
    };
    let mut g = json!({"PW":pw,"PH":ph,"ML":ml,"MT":mt,"BODY_W":bw,"BODY_H":bh});
    if truth(&dx.into()) {
        g["mirrorDx"] = dx.into()
    }
    g
}
pub fn stale_top_cache(d: &ParsedXml, root: usize, bh: f64) -> bool {
    if bh <= 0.0 {
        return false;
    }
    let (mut run, mut sum) = (0, 0.0);
    for p in pars(d, root) {
        let ss = segs(d, p);
        let Some(&first) = ss.first() else { continue };
        if n(d, first, "vertpos") != 0.0
            || d.nodes[p].attr("pageBreak") == Some("1")
            || d.nodes[p].attr("columnBreak") == Some("1")
        {
            run = 0;
            sum = 0.0;
            continue;
        }
        if run >= 2 && sum <= bh / 2.0 {
            return true;
        }
        let last = *ss.last().unwrap();
        sum += n(d, last, "vertpos") + n(d, last, "vertsize") + n(d, last, "spacing");
        run += 1;
    }
    false
}
pub fn overrun_top_cache(d: &ParsedXml, root: usize, bh: f64) -> bool {
    if bh <= 0.0 {
        return false;
    }
    let mut over = 0;
    for p in pars(d, root) {
        for s in segs(d, p) {
            over = if n(d, s, "vertpos") > bh { over + 1 } else { 0 };
            if over >= 3 {
                return true;
            }
        }
    }
    false
}
pub fn absolute_top_cache(d: &ParsedXml, root: usize, ml: f64, bw: f64) -> bool {
    let (mut total, mut over) = (0, 0);
    for p in pars(d, root) {
        for s in segs(d, p) {
            if n(d, s, "horzsize") <= 0.0 {
                continue;
            }
            total += 1;
            let hp = n(d, s, "horzpos");
            if hp >= ml - 500.0 && hp + n(d, s, "horzsize") > bw + 500.0 {
                over += 1
            }
        }
    }
    over >= 3 && over * 5 >= total
}
fn span(
    d: &ParsedXml,
    t: usize,
    ty: f64,
    mt: f64,
    bh: f64,
    ph: f64,
    ctx: &mut RenderContext,
) -> Result<super::table_pages::FloatSpan, String> {
    svg_table::table_cont_pages(
        d,
        t,
        PlanOptions {
            ty,
            mt,
            body_h: bh,
            slack: ph - mt - bh,
            nested: false,
            cont_mt: mt,
        },
        &mut ctx.extent,
    )
}
fn mirror(base: f64, dx: f64, page: f64) -> f64 {
    if truth(&dx.into()) && (base + page + 1.0) % 2.0 == 0.0 {
        dx
    } else {
        0.0
    }
}
fn set_body_dx(ctx: &mut RenderContext, base: f64, dx: f64) {
    ctx.body_dx = Some(Box::new(move |p| Ok(mirror(base, dx, number(p)).into())))
}
fn wrap_mirror(bufs: &mut [Vec<LayoutText>], from: &[usize], base: f64, dx: f64) {
    for (page, buf) in bufs.iter_mut().enumerate() {
        let shift = mirror(base, dx, page as f64);
        let at = from.get(page).copied().unwrap_or(0);
        if truth(&shift.into()) && buf.len() > at {
            buf.insert(
                at,
                format!("<g transform=\"translate({} 0)\">", pt(shift)).into(),
            );
            buf.push("</g>".into())
        }
    }
}
fn draw_story(
    d: &mut ParsedXml,
    story: &Story,
    ctx: &mut RenderContext,
    base: f64,
    dx: f64,
    renderer: &mut Renderer,
) -> Result<(), String> {
    ctx.page = story.page.clone();
    let x = add(&story.x, &mirror(base, dx, number(&story.page)).into());
    for p in pars(d, story.sub) {
        draw_para_native(
            d,
            p,
            x.clone(),
            story.y.clone(),
            number(&story.width),
            ctx,
            0,
            None,
            renderer,
        )?
    }
    Ok(())
}
fn draw_rule(rule: &page_stories::Rule, ctx: &mut RenderContext) -> Result<(), String> {
    ctx.page = rule.page.clone();
    let color = LayoutText::from(rule.color.as_str())
        .escape_xml()
        .to_string_strict()?;
    ctx.emit(
        format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
            pt(number(&rule.x)),
            pt(number(&rule.y)),
            pt(number(&add(&rule.x, &rule.length.into()))),
            pt(number(&rule.y)),
            color,
            super::shape_geometry::fixed2(rule.width_mm * 2.834645)
        )
        .into(),
    )
}
enum Job {
    Story(Story),
    Object(PaintJob),
}
impl Job {
    fn layer(&self) -> f64 {
        match self {
            Self::Story(s) => s.layer.unwrap(),
            Self::Object(j) => j.layer as f64,
        }
    }
    fn z(&self) -> f64 {
        match self {
            Self::Story(_) => 0.0,
            Self::Object(j) => j.z,
        }
    }
}

/// The context holds document-wide shared maps and counters. Per-section fields
/// are reset exactly where the original shallow context copy acquired them.
#[allow(clippy::too_many_arguments)]
pub fn render_section_to_pages(
    d: &mut ParsedXml,
    section: &SectionInput,
    geom: &Value,
    ctx: &mut RenderContext,
    options: &RenderOptions,
    last_section: bool,
    state: &mut BlockState,
) -> Result<f64, String> {
    let root = section.root;
    let (pw, ph, ml, mt, bw, bh) = (
        number(&geom["PW"]),
        number(&geom["PH"]),
        number(&geom["ML"]),
        number(&geom["MT"]),
        number(&geom["BODY_W"]),
        number(&geom["BODY_H"]),
    );
    let _ = pw;
    if section.has_deletion_markers {
        pm::prepare_deleted_ranges(d, root, 0, &mut ctx.extent)
    }
    let degenerate = stale_top_cache(d, root, bh) || overrun_top_cache(d, root, bh);
    if degenerate {
        for p in pars(d, root) {
            remove_lines(d, p)?
        }
        ctx.warnings
            .push("조판 캐시가 맞지 않아(최상위 문단이 모두 같은 자리) 다시 조판해 렌더".into())
    }
    for tc in descendants(d, root)
        .into_iter()
        .filter(|&i| d.nodes[i].local == "tc")
        .collect::<Vec<_>>()
    {
        let paras = child(d, tc, "subList")
            .map(|s| pars(d, s))
            .unwrap_or_default();
        let vs = paras
            .iter()
            .flat_map(|&p| segs(d, p))
            .map(|s| n(d, s, "vertpos"))
            .collect::<Vec<_>>();
        let repeated = vs.len() >= 3
            && (vs.iter().all(|v| *v == vs[0])
                || vs.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]));
        let bad_height = if repeated {
            false
        } else {
            paras.iter().any(|&p| {
                let runs = d.nodes[p]
                    .children
                    .iter()
                    .copied()
                    .filter(|&r| d.nodes[r].local == "run")
                    .collect::<Vec<_>>();
                if runs.is_empty()
                    || runs
                        .iter()
                        .any(|&r| d.nodes[r].children.iter().any(|&c| d.nodes[c].local != "t"))
                {
                    return false;
                }
                let ch = runs.iter().fold(f64::NEG_INFINITY, |h, &r| {
                    let st = lookup(
                        &ctx.styles["charPr"],
                        d.nodes[r].attr("charPrIDRef").unwrap_or(""),
                    );
                    maximum(
                        h,
                        number(
                            &if matches!(st, Value::Null | Value::Undefined) {
                                head_styles::default_char()
                            } else {
                                st.clone()
                            }["height"],
                        ),
                    )
                });
                segs(d, p).iter().any(|&s| n(d, s, "textheight") > ch * 1.5)
            })
        };
        if repeated || bad_height {
            for p in paras {
                remove_lines(d, p)?
            }
        }
    }
    let absolute = !degenerate && absolute_top_cache(d, root, ml, bw);
    if absolute {
        for s in descendants(d, root)
            .into_iter()
            .filter(|&s| d.nodes[s].local == "linesegarray")
            .collect::<Vec<_>>()
        {
            if let Some(p) = d.nodes[s].parent {
                d.remove_child(p, s)?
            }
        }
        ctx.warnings
            .push("조판 캐시가 쪽 기준 좌표로 적혀 있어 다시 조판해 렌더".into())
    }
    let mixed = !options.reflow
        && !degenerate
        && !absolute
        && descendants(d, root)
            .iter()
            .any(|&p| d.nodes[p].local == "p" && child(d, p, "linesegarray").is_none());
    if options.reflow || degenerate || absolute || mixed {
        reflow_block::reflow_section(
            d,
            root,
            &ctx.styles,
            &json!({"BODY_W":bw,"BODY_H":bh}),
            &options.reflow_mode,
            &mut ctx.extent,
            state,
        )?;
    }
    let col_pr = find_first(d, root, "colPr", 0);
    let multi = num(d, col_pr, "colCount", 1.0) > 1.0;
    let ncol = if multi {
        num(d, col_pr, "colCount", 1.0)
    } else {
        1.0
    };
    let gap = if multi {
        num(d, col_pr, "sameGap", 0.0)
    } else {
        0.0
    };
    let cw = (bw - gap * (ncol - 1.0)) / ncol;
    let local_cols = multi
        && !pars(d, root)
            .iter()
            .flat_map(|&p| segs(d, p))
            .any(|s| n(d, s, "horzpos") >= cw);
    let mut pages = ParaPages::new();
    let mut tops = HashMap::<usize, Value>::new();
    let mut back = HashMap::new();
    let geom0 = ctx.styles["paraGeom"].clone();
    let (mut table_end, mut count, mut max_top) = (-1.0, 1.0, 0.0);
    let (mut prev_v, mut prev_h, mut prev_end) =
        (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let (mut cur, mut col) = (0.0, 0.0);
    let (mut pending_end, mut pending_y, mut pending_top) = (0.0, 0.0, -1.0);
    let (mut last_end, mut last_page) = (0.0, -1.0);
    let mut last_sa = Value::from(0.0);
    let mut prev_seg: Option<(usize, usize, usize, f64, f64)> = None;
    for p in pars(d, root) {
        let ss = segs(d, p);
        let mut pg = Vec::<Value>::new();
        let mut first = true;
        for &s in &ss {
            let v = n(d, s, "vertpos");
            let h = n(d, s, "horzpos");
            if local_cols {
                if v < prev_v || first && v == prev_v && h <= prev_h && v == 0.0 && prev_end > v {
                    if first && d.nodes[p].attr("pageBreak") == Some("1") {
                        col = 0.0;
                        cur += 1.0
                    } else {
                        col += 1.0;
                        if col >= ncol {
                            col = 0.0;
                            cur += 1.0
                        }
                    }
                }
                if col > 0.0 {
                    d.nodes[s].set_attr(
                        "horzpos",
                        string(&(h + svg_para::round(col * (cw + gap))).into()),
                    )
                }
            } else if first && pending_end > cur && !(v > prev_v && v < pending_top - 100.0) {
                cur = if v < pending_y - 100.0 {
                    pending_end + 1.0
                } else {
                    pending_end
                };
                pending_end = 0.0;
            } else {
                let brk = if v < prev_v {
                    !multi || h <= prev_h
                } else {
                    v == prev_v && h <= prev_h && prev_end > v && v < bh / 2.0
                };
                let mut pbrk = false;
                if !brk
                    && first
                    && prev_v != f64::NEG_INFINITY
                    && d.nodes[p].attr("pageBreak") == Some("1")
                {
                    pbrk = v < bh / 2.0;
                    if !pbrk {
                        let m = pm::build_para(d, p, &mut ctx.extent)?;
                        for o in list(&m, "objs") {
                            let el = o["el"].as_u64().ok_or("개체 노드 오류")? as usize;
                            if !truth(&o["inline"])
                                && d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM")
                                    == "TOP_AND_BOTTOM"
                                && attr(d, child(d, el, "pos"), "vertRelTo").unwrap_or("PARA")
                                    == "PARA"
                                && object_anchor::line_pushed_by(d, o, &v.into(), &0.into())?
                            {
                                pbrk = true;
                                break;
                            }
                        }
                    }
                }
                let obrk = !brk
                    && !pbrk
                    && !multi
                    && first
                    && v > prev_v
                    && v < prev_end - 100.0
                    && prev_end + n(d, s, "vertsize") > bh
                    && v < bh / 2.0;
                if brk || pbrk || obrk {
                    cur += 1.0
                }
                if let Some((old_s, old_p, idx, vs, sp)) = prev_seg
                    && brk
                    && prev_v + vs > bh
                    && v == vs + sp
                {
                    let old_pages = if old_p == p {
                        Some(&mut pg)
                    } else {
                        pages.get_mut(&old_p)
                    };
                    if let Some(old_pages) = old_pages
                        && old_pages.get(idx).is_some_and(|v| number(v) == cur - 1.0)
                    {
                        old_pages[idx] = cur.into();
                        d.nodes[old_s].set_attr("vertpos", "0".into());
                        if idx == 0 {
                            tops.insert(old_p, 0.into());
                        }
                        if old_p != p {
                            last_page = cur;
                            last_end = v;
                            last_sa = 0.into();
                        }
                    }
                }
            }
            first = false;
            pg.push(cur.into());
            prev_seg = Some((s, p, pg.len() - 1, n(d, s, "vertsize"), n(d, s, "spacing")));
            max_top = maximum(max_top, v + num(d, Some(s), "textheight", 1000.0));
            prev_v = v;
            prev_h = h;
            prev_end = v + n(d, s, "vertsize") + n(d, s, "spacing");
        }
        let (prev_end0, prev_page0) = (last_end, last_page);
        if !ss.is_empty() {
            tops.insert(
                p,
                if number(&pg[0]) == last_page {
                    add(&last_end.into(), &last_sa)
                } else {
                    0.into()
                },
            );
            let last = *ss.last().unwrap();
            last_end = n(d, last, "vertpos") + n(d, last, "vertsize") + n(d, last, "spacing");
            last_page = number(pg.last().unwrap());
            last_sa = nn(
                &lookup(&geom0, d.nodes[p].attr("paraPrIDRef").unwrap_or(""))["spaceAfter"],
                0.into(),
            );
        }
        if !local_cols && !ss.is_empty() {
            let m = pm::build_para(d, p, &mut ctx.extent)?;
            let mut square = None;
            for o in list(&m, "objs") {
                if o["tag"].as_str() != Some("tbl") {
                    continue;
                }
                let el = o["el"].as_u64().ok_or("개체 노드 오류")? as usize;
                let pos = child(d, el, "pos");
                let om = child(d, el, "outMargin");
                let inline = truth(&o["inline"]);
                if !inline && attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA" {
                    continue;
                }
                let ms = list(&m, "segs");
                let k = ms
                    .iter()
                    .enumerate()
                    .position(|(i, s)| {
                        number(&s["textpos"]) <= number(&o["index"])
                            && (i + 1 == ms.len()
                                || number(&ms[i + 1]["textpos"]) > number(&o["index"]))
                    })
                    .unwrap_or(0);
                let sg = ss.get(k).copied();
                let sv = num(d, sg, "vertpos", 0.0);
                let h = maximum(
                    number(&o["height"]),
                    pm::measure_table_height(d, el, &mut ctx.extent)?,
                );
                let ot = num(d, om, "top", 0.0);
                let ob = num(d, om, "bottom", 0.0);
                let pushed = sv - (ot + h + ob);
                if !inline
                    && k == 0
                    && pushed < -100.0
                    && prev_page0 >= 0.0
                    && pg.first().is_some_and(|v| number(v) == prev_page0 + 1.0)
                    && d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM") == "TOP_AND_BOTTOM"
                {
                    let sp = span(d, el, mt + prev_end0 + ot, mt, bh, ph, ctx)?;
                    if sp.pages == 1.0 && (sp.end_y - mt + ob - sv).abs() <= 300.0 {
                        back.insert(el, json!({"page":prev_page0,"y":mt+prev_end0+ot}));
                        continue;
                    }
                }
                let sq = if !inline {
                    if square.is_none() {
                        square = Some(object_anchor::square_stack_tops(
                            d,
                            list(&m, "objs"),
                            &mut ctx.extent,
                        )?)
                    }
                    square.as_ref().unwrap().get(&el).cloned()
                } else {
                    None
                };
                if let Some(sq) = sq {
                    let anchor = pg.get(k).cloned().unwrap_or(Value::Undefined);
                    let y0 = add(
                        &add(&tops.get(&p).cloned().unwrap_or(0.into()), &sq),
                        &ot.into(),
                    );
                    let next = number(&add(
                        &y0,
                        &maximum(
                            number(&o["height"]),
                            pm::measure_table_height(d, el, &mut ctx.extent)?,
                        )
                        .into(),
                    )) > bh;
                    let place = if next {
                        json!({"page":add(&anchor,&1.into()),"y":mt+ot})
                    } else {
                        json!({"page":anchor,"y":add(&mt.into(),&y0)})
                    };
                    back.insert(el, place.clone());
                    let sp = span(d, el, number(&place["y"]), mt, bh, ph, ctx)?;
                    let end = number(&add(&place["page"], &sp.pages.into()));
                    if end > pending_end {
                        pending_end = end;
                        pending_y = sp.end_y - mt;
                        pending_top = -1.0
                    }
                    continue;
                }
                let nt = list(&m, "objs")
                    .iter()
                    .filter(|q| {
                        !truth(&q["inline"])
                            && q["el"].as_u64().is_some_and(|id| {
                                d.nodes[id as usize]
                                    .attr("textWrap")
                                    .unwrap_or("TOP_AND_BOTTOM")
                                    == "TOP_AND_BOTTOM"
                            })
                    })
                    .count();
                let wrap = d.nodes[el].attr("textWrap").unwrap_or("TOP_AND_BOTTOM");
                let ptop = tops.get(&p).cloned().unwrap_or(Value::Undefined);
                let sb = if matches!(m["paraPrId"], Value::Null | Value::Undefined) {
                    0.into()
                } else {
                    nn(
                        &lookup(&geom0, &string(&m["paraPrId"]))["spaceBefore"],
                        0.into(),
                    )
                };
                // Preserve the source short-circuit order of fallible anchoring.
                #[allow(clippy::nonminimal_bool)]
                let stay = nt < 2
                    && !list(&m, "objs").iter().any(|q| truth(&q["inline"]))
                    && !list(&m, "chars").iter().any(|c| {
                        c["ch"].as_str() != Some("")
                            && !truth(&c["nb"])
                            && !crate::input::trim(c["ch"].as_str().unwrap_or("")).is_empty()
                    })
                    && !(!inline
                        && wrap == "TOP_AND_BOTTOM"
                        && object_anchor::line_pushed_by(d, o, &sv.into(), &ptop)?);
                let nonpush = if wrap != "TOP_AND_BOTTOM"
                    && !ptop.is_undefined()
                    && sv > number(&add(&add(&ptop, &sb), &100.into()))
                {
                    Some(add(&ptop, &sb))
                } else {
                    None
                };
                let vo = maximum(
                    0.0,
                    super::layout::to_int32(
                        number(
                            &attr(d, pos, "vertOffset")
                                .map(Value::from)
                                .unwrap_or(Value::Undefined),
                        ),
                        0.0,
                    ),
                );
                let start = if k == 0 && !ptop.is_undefined() && number(&ptop) < sv {
                    number(&ptop)
                } else {
                    sv
                };
                let below = !inline
                    && wrap == "TOP_AND_BOTTOM"
                    && vo + ot >= num(d, sg, "textheight", 0.0)
                    && !object_anchor::line_pushed_by(d, o, &sv.into(), &ptop)?;
                let ty = if inline {
                    mt + sv + ot
                } else if let Some(np) = &nonpush {
                    number(&add(&add(&add(&mt.into(), np), &ot.into()), &vo.into()))
                } else {
                    mt + if below {
                        sv - if k == 0 { number(&sb) } else { 0.0 }
                    } else if stay {
                        sv - number(&sb)
                    } else if pushed - vo >= -100.0 {
                        pushed - vo
                    } else {
                        start
                    } + ot
                        + vo
                };
                let spn = span(d, el, ty, mt, bh, ph, ctx)?;
                let anchor = pg.get(k).map(number).unwrap_or(f64::NAN);
                let sp = if !inline
                    && !stay
                    && nonpush.is_none()
                    && sv - (ot + number(&o["height"]) + ob) >= -100.0
                    && pg
                        .iter()
                        .enumerate()
                        .any(|(j, v)| j > k && number(v) > anchor)
                {
                    Some(span(
                        d,
                        el,
                        mt + sv - number(&o["height"]) - ob,
                        mt,
                        bh,
                        ph,
                        ctx,
                    )?)
                } else {
                    None
                };
                if let Some(sp) = sp
                    && sp.pages > 0.0
                {
                    for v in pg.iter_mut().skip(k + 1) {
                        if number(v) > anchor {
                            *v = add(v, &sp.pages.into())
                        }
                    }
                    cur += sp.pages;
                    last_page = pg.last().map(number).unwrap_or(f64::NAN);
                    continue;
                }
                let first_page = pg.first().map(number).unwrap_or(cur);
                if spn.pages > 0.0
                    && d.nodes[el].attr("pageBreak") != Some("NONE")
                    && first_page + spn.pages > pending_end
                {
                    pending_end = first_page + spn.pages;
                    pending_y = spn.end_y - mt;
                    pending_top = if below && vo > 0.0 && ty - mt < bh {
                        ty - mt
                    } else {
                        -1.0
                    };
                }
            }
        }
        pages.insert(p, pg);
        count = maximum(maximum(count, cur + 1.0), pending_end + 1.0);
        table_end = maximum(table_end, pending_end);
    }
    if last_section {
        let mut last_content = -1.0;
        for p in pars(d, root) {
            if let Some(pg) = pages.get(&p)
                && !pg.is_empty()
                && content(&pm::build_para(d, p, &mut ctx.extent)?)?
            {
                for v in pg {
                    last_content = maximum(last_content, number(v))
                }
            }
        }
        let content_only = last_content;
        last_content = maximum(last_content, table_end);
        let mut keep = false;
        if last_content >= 0.0 && count - 1.0 > last_content && table_end <= content_only {
            let first_trail = pars(d, root).into_iter().find(|p| {
                pages
                    .get(p)
                    .is_some_and(|pg| pg.iter().any(|v| number(v) > last_content))
            });
            let mut fits = true;
            for p in pars(d, root) {
                let Some(pg) = pages.get(&p) else { continue };
                if !pg.iter().any(|v| number(v) == last_content) {
                    continue;
                }
                let m = pm::build_para(d, p, &mut ctx.extent)?;
                for (k, s) in list(&m, "segs").iter().enumerate() {
                    if pg.get(k).is_none_or(|v| number(v) != last_content) {
                        continue;
                    }
                    let mut h = number(&s["textheight"]);
                    for o in list(&m, "objs") {
                        if truth(&o["inline"]) {
                            h = maximum(h, number(&o["height"]))
                        }
                    }
                    if number(&s["vertpos"]) + h > bh {
                        fits = false
                    }
                }
            }
            keep = first_trail.is_some_and(|p| d.nodes[p].attr("pageBreak") != Some("1")) && fits;
        }
        if last_content >= 0.0 && count - 1.0 > last_content && !keep {
            for pg in pages.values_mut() {
                if pg.iter().any(|v| number(v) > last_content) {
                    for v in pg {
                        *v = minimum(number(v), last_content).into()
                    }
                }
            }
            count = last_content + 1.0
        }
    }
    let page_h = if count == 1.0 {
        maximum(ph, mt + max_top + 2000.0)
    } else {
        ph
    };
    let ens = page_stories::end_note_layout(d, root, &pages, geom, &ctx.extent)?;
    let page_count = maximum(
        count,
        ens.as_ref().map(|e| number(&e.page_count)).unwrap_or(0.0),
    );
    if !page_count.is_finite() || !(0.0..=500_000.0).contains(&page_count) {
        return Err("출력 쪽 수가 DOM 노드 한도를 넘었습니다.".into());
    }
    ctx.pages = vec![Vec::new(); page_count as usize];
    ctx.page = 0.into();
    ctx.geom = geom.clone();
    ctx.page_h = page_h;
    ctx.paint_jobs = Some(Vec::new());
    ctx.region_rotations.clear();
    ctx.cols = Value::Undefined;
    ctx.para_tops.clear();
    ctx.obj_place.clear();
    ctx.body_dx = None;
    ctx.esc_origin = Value::Undefined;
    ctx.split_cell = Value::Undefined;
    ctx.split_cont_mt = Value::Undefined;
    let base = number(&ctx.page_base);
    let dx = if truth(&geom["mirrorDx"]) {
        number(&geom["mirrorDx"])
    } else {
        0.0
    };
    let mut renderer = Renderer;
    let mut story_jobs = Vec::new();
    let stories = page_stories::page_stories(
        d,
        root,
        &section.masters,
        &pages,
        page_stories::PageOptions {
            count: &(ctx.pages.len()).into(),
            base: &ctx.page_base,
            geometry: geom,
        },
        &ctx.extent,
    )?;
    for story in stories {
        pm::prepare_deleted_ranges(d, story.sub, 0, &mut ctx.extent);
        if options.reflow {
            reflow_block::reflow_section(
                d,
                story.sub,
                &ctx.styles,
                &json!({"BODY_W":story.width,"BODY_H":0}),
                &options.reflow_mode,
                &mut ctx.extent,
                state,
            )?;
        }
        if story.layer.is_some() {
            story_jobs.push(Job::Story(story))
        } else {
            draw_story(d, &story, ctx, base, dx, &mut renderer)?
        }
    }
    ctx.page = 0.into();
    let body_start = ctx.pages.iter().map(Vec::len).collect::<Vec<_>>();
    if truth(&dx.into()) {
        set_body_dx(ctx, base, dx)
    }
    ctx.cols = if multi {
        json!({"w":cw,"gap":gap})
    } else {
        Value::Undefined
    };
    ctx.para_tops = tops;
    ctx.obj_place.extend(back);
    for p in pars(d, root) {
        draw_para(
            d,
            p,
            ml,
            mt,
            bw,
            ctx,
            0,
            pages.get(&p).map(Vec::as_slice),
            &mut renderer,
        )?
    }
    ctx.cols = Value::Undefined;
    ctx.para_tops.clear();
    let notes = page_stories::foot_note_stories(d, root, &pages, geom, &mut ctx.extent)?;
    for rule in &notes.rules {
        draw_rule(rule, ctx)?
    }
    for story in &notes.stories {
        draw_story(d, story, ctx, base, 0.0, &mut renderer)?
    }
    if let Some(e) = &ens {
        for rule in &e.rules {
            draw_rule(rule, ctx)?
        }
        ctx.cols = if multi {
            json!({"w":cw,"gap":gap})
        } else {
            Value::Undefined
        };
        for p in &e.paras {
            draw_para(d, p.p, ml, mt, bw, ctx, 0, Some(&p.pages), &mut renderer)?
        }
        ctx.cols = Value::Undefined;
    }
    ctx.body_dx = None;
    wrap_mirror(&mut ctx.pages, &body_start, base, dx);
    let mut all_pages = pages.clone();
    if let Some(e) = &ens {
        for p in &e.paras {
            all_pages.insert(p.p, p.pages.clone());
        }
    }
    draw_page_decor(
        d,
        root,
        &all_pages,
        ctx,
        if local_cols { ncol } else { 1.0 },
        cw,
        gap,
    )?;
    draw_page_numbers(d, root, &pages, ctx)?;
    ctx.page = 0.into();
    let normal = std::mem::take(&mut ctx.pages);
    let jobs = ctx.paint_jobs.take().unwrap_or_default();
    story_jobs.extend(jobs.into_iter().map(Job::Object));
    story_jobs.sort_by(|a, b| {
        let l = a.layer() - b.layer();
        let v = if l != 0.0 && !l.is_nan() {
            l
        } else {
            a.z() - b.z()
        };
        v.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let (mut behind, mut front) = (
        vec![Vec::new(); normal.len()],
        vec![Vec::new(); normal.len()],
    );
    for job in story_jobs {
        let target = if job.layer() < 0.0 {
            &mut behind
        } else {
            &mut front
        };
        ctx.pages = std::mem::take(target);
        let before = ctx.pages.iter().map(Vec::len).collect::<Vec<_>>();
        let body = matches!(&job,Job::Object(j)if j.body);
        if body {
            set_body_dx(ctx, base, dx)
        } else {
            ctx.body_dx = None
        }
        match job {
            Job::Story(s) => draw_story(d, &s, ctx, base, dx, &mut renderer)?,
            Job::Object(j) => j.replay(d, ctx, &mut renderer)?,
        }
        ctx.body_dx = None;
        if body && truth(&dx.into()) {
            wrap_mirror(&mut ctx.pages, &before, base, dx)
        }
        *target = std::mem::take(&mut ctx.pages);
    }
    ctx.pages = normal
        .into_iter()
        .zip(behind)
        .zip(front)
        .map(|((mut normal, mut behind), front)| {
            behind.append(&mut normal);
            behind.extend(front);
            behind
        })
        .collect();
    Ok(page_h)
}

fn visible_fill(d: &ParsedXml, node: Option<usize>, styles: &Value) -> Option<Value> {
    let f = node?;
    let b = lookup(
        &styles["borderFill"],
        d.nodes[f].attr("borderFillIDRef").unwrap_or(""),
    );
    ["top", "bottom", "left", "right", "fill", "gradient"]
        .iter()
        .any(|key| truth(&b[*key]))
        .then(|| b.clone())
}
fn parse_float(s: &str) -> f64 {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[+-]?(?:Infinity|(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)")
            .unwrap()
    });
    RE.find(s.trim_start_matches(crate::input::is_space))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(f64::NAN)
}
pub fn draw_page_decor(
    d: &ParsedXml,
    root: usize,
    pages: &ParaPages,
    ctx: &mut RenderContext,
    ncol: f64,
    cw: f64,
    gap: f64,
) -> Result<(), String> {
    let (pw, ph, ml, mt) = (
        number(&ctx.geom["PW"]),
        number(&ctx.geom["PH"]),
        number(&ctx.geom["ML"]),
        number(&ctx.geom["MT"]),
    );
    let fills = find_first(d, root, "secPr", 0)
        .map(|s| {
            d.nodes[s]
                .children
                .iter()
                .copied()
                .filter(|&c| d.nodes[c].local == "pageBorderFill")
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut bottoms = Vec::<(Value, f64)>::new();
    if ncol > 1.0 {
        for p in pars(d, root) {
            let pg = pages.get(&p).map(Vec::as_slice).unwrap_or(&[]);
            let parent = child(d, p, "linesegarray").unwrap_or(p);
            for (k, s) in d.nodes[parent]
                .children
                .iter()
                .copied()
                .filter(|&s| d.nodes[s].local == "lineseg")
                .enumerate()
            {
                let page = pg.get(k).or(pg.first()).cloned().unwrap_or(0.into());
                let value = n(d, s, "vertpos") + n(d, s, "vertsize");
                if let Some((_, b)) = bottoms.iter_mut().find(|(v, _)| *v == page) {
                    *b = maximum(*b, value)
                } else {
                    bottoms.push((page, maximum(0.0, value)))
                }
            }
        }
    }
    let col_line = find_first(d, root, "colPr", 0).and_then(|c| child(d, c, "colLine"));
    // A corrupt count must not create an unbounded column-line loop.
    if ncol > 500_000.0 {
        return Err("단 구분선 수가 DOM 노드 한도를 넘었습니다.".into());
    }
    for page in 0..ctx.pages.len() {
        let global = number(&ctx.page_base) + page as f64 + 1.0;
        let both = fills
            .iter()
            .copied()
            .find(|&f| d.nodes[f].attr("type").unwrap_or("BOTH") == "BOTH");
        let parity = fills.iter().copied().find(|&f| {
            d.nodes[f].attr("type") == Some(if global % 2.0 != 0.0 { "ODD" } else { "EVEN" })
        });
        let pf = if visible_fill(d, both, &ctx.styles).is_some() {
            both
        } else {
            parity
        };
        let bf = visible_fill(d, pf, &ctx.styles);
        ctx.page = page.into();
        if let (Some(pf), Some(bf)) = (pf, bf) {
            let off = child(d, pf, "offset");
            let x0 = num(d, off, "left", 0.0);
            let y0 = num(d, off, "top", 0.0);
            let x1 = pw - num(d, off, "right", 0.0);
            let y1 = ph - num(d, off, "bottom", 0.0);
            if (truth(&bf["fill"]) || truth(&bf["gradient"]))
                && d.nodes[pf].attr("fillArea") != Some("NONE")
            {
                ctx.pages[page].insert(
                    0,
                    fill_paint::paint_fill(&bf, x0, y0, x1 - x0, y1 - y0, &mut ctx.defs)?.into(),
                )
            }
            for (key, a, b, c, e) in [
                ("top", x0, y0, x1, y0),
                ("bottom", x0, y1, x1, y1),
                ("left", x0, y0, x0, y1),
                ("right", x1, y0, x1, y1),
            ] {
                if truth(&bf[key]) {
                    ctx.emit(svg_table::edge_line(
                        &a.into(),
                        &b.into(),
                        &c.into(),
                        &e.into(),
                        &bf[key],
                    )?)?
                }
            }
        }
        let bottom = bottoms
            .iter()
            .find(|(v, _)| v.as_f64() == Some(page as f64))
            .map(|(_, b)| *b)
            .unwrap_or(0.0);
        if let Some(line) = col_line
            && d.nodes[line].attr("type").unwrap_or("NONE") != "NONE"
            && truth(&bottom.into())
        {
            let width = parse_float(d.nodes[line].attr("width").unwrap_or("0.12"));
            let width = if truth(&width.into()) { width } else { 0.12 };
            let edge = json!({"type":d.nodes[line].attr("type").unwrap_or("SOLID"),"widthPt":width*2.834645,"color":d.nodes[line].attr("color").unwrap_or("#000000")});
            let mut c = 1.0;
            while c < ncol {
                let x = ml + c * (cw + gap) - gap / 2.0;
                ctx.emit(svg_table::edge_line(
                    &x.into(),
                    &mt.into(),
                    &x.into(),
                    &(mt + bottom).into(),
                    &edge,
                )?)?;
                c += 1.0
            }
        }
    }
    Ok(())
}
pub fn draw_page_numbers(
    d: &ParsedXml,
    root: usize,
    pages: &ParaPages,
    ctx: &mut RenderContext,
) -> Result<(), String> {
    let mut decl = None;
    let mut hidden = Vec::new();
    let mut restarts = Vec::new();
    for p in pars(d, root) {
        let page = pages
            .get(&p)
            .and_then(|p| p.first())
            .cloned()
            .unwrap_or(0.into());
        for &r in &d.nodes[p].children {
            if d.nodes[r].local != "run" {
                continue;
            }
            for &c in &d.nodes[r].children {
                if d.nodes[c].local != "ctrl" {
                    continue;
                }
                if let Some(pn) = child(d, c, "pageNum")
                    && decl.is_none()
                {
                    decl = Some((
                        pn,
                        page.clone(),
                        d.nodes[r].attr("charPrIDRef").map(str::to_owned),
                    ))
                }
                if let Some(ph) = child(d, c, "pageHiding")
                    && truth(&n(d, ph, "hidePageNum").into())
                {
                    hidden.push(page.clone())
                }
                if let Some(nn) = child(d, c, "newNum")
                    && d.nodes[nn].attr("numType").unwrap_or("PAGE") == "PAGE"
                {
                    restarts.push((number(&page), num(d, Some(nn), "num", 1.0)))
                }
            }
        }
    }
    let Some((el, start, pr_id)) = decl else {
        return Ok(());
    };
    let pos = d.nodes[el].attr("pos").unwrap_or("BOTTOM_CENTER");
    if pos == "NONE" {
        return Ok(());
    }
    let side = d.nodes[el].attr("sideChar").unwrap_or("");
    let pn_id = nn(
        &ctx.styles["pageNumCharPr"],
        pr_id.map(Value::from).unwrap_or(Value::Null),
    );
    let st = if matches!(pn_id, Value::Null | Value::Undefined) {
        head_styles::default_char()
    } else {
        nn(
            lookup(&ctx.styles["charPr"], &string(&pn_id)),
            head_styles::default_char(),
        )
    };
    let (ph, ml, bw) = (
        number(&ctx.geom["PH"]),
        number(&ctx.geom["ML"]),
        number(&ctx.geom["BODY_W"]),
    );
    let margin = find_first(d, root, "pagePr", 0).and_then(|p| child(d, p, "margin"));
    let h = if matches!(ctx.styles["pageNumCharPr"], Value::Null | Value::Undefined) {
        1000.0
    } else {
        number(&st["height"])
    };
    let mut page = number(&start);
    if page < 0.0 {
        return Err("쪽 번호 시작 위치가 음수입니다.".into());
    }
    while page < (ctx.pages.len() as f64) {
        if hidden.iter().any(|v| v.as_f64() == Some(page)) {
            page += 1.0;
            continue;
        }
        let restart = restarts.iter().rev().find(|(p, _)| *p <= page);
        let v = if let Some((p, n)) = restart {
            n + page - p
        } else {
            number(&ctx.page_base) + page + 1.0
        };
        let formatted = para_number::format_head_number(
            &v.into(),
            d.nodes[el].attr("formatType").unwrap_or("DIGIT"),
        )?;
        let text = if side.is_empty() {
            string(&formatted)
        } else {
            format!("{side} {} {side}", string(&formatted))
        };
        let w = metrics::measure_text_width(
            &text,
            h,
            100.0,
            Some(&json!({"faceClass":metrics::face_class_of(st["face"].as_str())})),
        );
        let x = if pos.contains("LEFT") {
            ml
        } else if pos.contains("RIGHT") {
            ml + bw - w
        } else {
            ml + (bw - w) / 2.0
        };
        let y = if pos.starts_with("TOP") {
            num(d, margin, "top", 5668.0) - 225.0
        } else if num(d, margin, "footer", 0.0) > 0.0 {
            ph - num(d, margin, "bottom", 4252.0) - 225.0
        } else {
            ph - svg_para::round(num(d, margin, "bottom", 4252.0) / 2.0) - 243.0
        };
        let mut out = LayoutText::from(format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"",
            pt(x),
            pt(y),
            pt(h)
        ));
        if truth(&st["fontFamily"]) {
            out.append(&TextRef::Utf8(" font-family=\""));
            out.append(
                &LayoutText::from(string(&st["fontFamily"]))
                    .escape_xml()
                    .as_text_ref(),
            );
            out.append(&TextRef::Utf8("\""))
        }
        out.append(&TextRef::Utf8(">"));
        out.append(&LayoutText::from(text).escape_xml().as_text_ref());
        out.append(&TextRef::Utf8("</text>"));
        ctx.page = page.into();
        ctx.emit(out)?;
        page += 1.0;
    }
    Ok(())
}
fn new_context(
    styles: Value,
    images: Map<String, Value>,
    warnings: Vec<String>,
    options: &RenderOptions,
) -> RenderContext {
    let mut ctx = RenderContext::new(styles, 0);
    ctx.images = images;
    ctx.warnings = warnings;
    ctx.highlights = options.highlights.clone();
    ctx.extent.memoize = true;
    ctx.extent.para_geom = list(&ctx.styles, "paraGeom")
        .iter()
        .filter_map(|e| e[0].as_str().map(|k| (k.to_owned(), e[1].clone())))
        .collect();
    ctx
}
fn render_roots(
    d: &mut ParsedXml,
    sections: &[SectionInput],
    ctx: &mut RenderContext,
    options: &RenderOptions,
) -> Result<Vec<RenderedSection>, String> {
    let mut last = sections.len().saturating_sub(1);
    while last > 0 && sections[last].masters.is_empty() {
        let mut any = false;
        for p in pars(d, sections[last].root) {
            if content(&pm::build_para(d, p, &mut ctx.extent)?)? {
                any = true;
                break;
            }
        }
        if any {
            break;
        }
        last -= 1;
    }
    let mut output = Vec::new();
    let mut state = BlockState::default();
    for (k, s) in sections.iter().enumerate().take(last + 1) {
        let geom = read_section_geom(d, s.root);
        let height = render_section_to_pages(d, s, &geom, ctx, options, k == last, &mut state)?;
        let pages = std::mem::take(&mut ctx.pages);
        ctx.page_base = add(&ctx.page_base, &pages.len().into());
        output.push(RenderedSection {
            pages,
            pw: geom["PW"].clone(),
            page_h: height.into(),
            clip_id: format!("pgclip{}", string(&s.index)),
        });
    }
    if output.is_empty() {
        return Err("렌더할 구역이 없습니다".into());
    }
    Ok(output)
}
pub fn render_section_roots(
    d: &mut ParsedXml,
    sections: &[SectionInput],
    styles: Value,
    images: Map<String, Value>,
    warnings: Vec<String>,
    options: &RenderOptions,
) -> Result<SectionRenderResult, String> {
    let mut ctx = new_context(styles, images, warnings, options);
    let sections = render_roots(d, sections, &mut ctx, options)?;
    Ok(SectionRenderResult {
        sections,
        defs: ctx.defs,
        warnings: ctx.warnings,
        stats: ctx.stats,
        regions: ctx.regions.regions,
    })
}
#[cfg(test)]
fn snapshot(d: &ParsedXml, i: usize) -> Value {
    json!({"name":d.nodes[i].name,"attrs":d.nodes[i].attributes().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"children":d.nodes[i].children.iter().map(|&i|snapshot(d,i)).collect::<Vec<_>>()})
}
#[cfg(test)]
fn text_report(t: &LayoutText) -> Value {
    json!({"text":t.to_string_strict().ok(),"utf16":t.0.iter().map(|&u|Value::from(u as u64)).collect::<Vec<_>>()})
}
#[cfg(test)]
fn pages_report(pages: &[Vec<LayoutText>]) -> Value {
    pages
        .iter()
        .map(|p| Value::Array(p.iter().map(text_report).collect()))
        .collect::<Vec<_>>()
        .into()
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    let mut d = parse_style_xml("<probe/>")?;
    let mut sections = Vec::new();
    let mut roots = Vec::new();
    for (sidx, s) in list(&input, "sections").iter().enumerate() {
        let root = d.import_document(parse_style_xml(
            s["xml"].as_str().ok_or("구역 XML 문자열이 필요합니다.")?,
        )?)?;
        let mut masters = Vec::new();
        roots.push(root);
        for m in list(s, "masters") {
            let master = d.import_document(parse_style_xml(
                m.as_str().ok_or("바탕쪽 XML 문자열이 필요합니다.")?,
            )?)?;
            masters.push(master);
            roots.push(master)
        }
        sections.push(SectionInput {
            root,
            index: s.get("index").cloned().unwrap_or(sidx.into()),
            masters,
            has_deletion_markers: s.get("hasDeletionMarkers").map(truth).unwrap_or(true),
        })
    }
    if matches!(op, "readSectionGeom" | "caches") {
        let root = sections.first().ok_or("구역이 필요합니다.")?.root;
        let geom = read_section_geom(&d, root);
        return Ok(if op=="readSectionGeom"{geom}else{json!({"stale":stale_top_cache(&d,root,number(&geom["BODY_H"])),"overrun":overrun_top_cache(&d,root,number(&geom["BODY_H"])),"absolute":absolute_top_cache(&d,root,number(&geom["ML"]),number(&geom["BODY_W"]))})}.to_json());
    }
    if !matches!(op, "renderSectionRoots" | "renderSectionToPages") {
        return Err("알 수 없는 구역 렌더링 연산".into());
    }
    let styles = if let Some(header) = input["header"].as_str() {
        head_styles::parse_render_styles(header, input.get("styleOpts"))?
    } else {
        input["styles"].clone()
    };
    let images = input["images"].as_object().cloned().unwrap_or_default();
    let warnings = list(&input, "warnings").iter().map(string).collect();
    let highlights = list(&input, "highlights")
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| "강조할 단어가 문자열이 아닙니다.".to_string())
                .map(|s| crate::input::trim(s).to_lowercase())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(LayoutText::from)
        .collect();
    let options = RenderOptions {
        reflow: truth(&input["reflow"]),
        reflow_mode: input["reflowMode"].as_str().unwrap_or("keep").to_owned(),
        highlights,
    };
    let mut ctx = new_context(styles, images, warnings, &options);
    let result = if op == "renderSectionToPages" {
        let section = sections.first().ok_or("구역이 필요합니다.")?;
        let geom = read_section_geom(&d, section.root);
        render_section_to_pages(
            &mut d,
            section,
            &geom,
            &mut ctx,
            &options,
            input.get("lastSection").map(truth).unwrap_or(true),
            &mut BlockState::default(),
        )
        .map(|height| {
            vec![RenderedSection {
                pages: std::mem::take(&mut ctx.pages),
                pw: geom["PW"].clone(),
                page_h: height.into(),
                clip_id: format!("pgclip{}", string(&section.index)),
            }]
        })
    } else {
        render_roots(&mut d, &sections, &mut ctx, &options)
    };
    let failed = result.is_err();
    let value = match result {
        Ok(sections) => {
            json!({"ok":true,"sections":sections.iter().map(|s|json!({"pages":pages_report(&s.pages),"PW":s.pw,"pageH":s.page_h,"clipId":s.clip_id})).collect::<Vec<_>>()})
        }
        Err(_) => json!({"ok":false}),
    };
    let mut report = json!({"value":value,"warnings":ctx.warnings,"images":Value::Object(ctx.images),"documents":roots.iter().map(|&r|snapshot(&d,r)).collect::<Vec<_>>()});
    // The roots wrapper hides its context on an exception; the private-helper
    // probe exposes and compares the completed shared mutations independently.
    if !failed || op == "renderSectionToPages" {
        report["defs"] = ctx.defs.into();
        report["stats"] = ctx.stats;
        report["regions"] = ctx.regions.regions.into();
    }
    Ok(report.to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_gutter_and_landscape() {
        let d=parse_style_xml("<sec><pagePr width='100' height='200' landscape='NARROWLY' gutterType='LEFT_RIGHT'><margin left='10' right='20' top='5' bottom='6' header='3' footer='4' gutter='7'/></pagePr></sec>").unwrap();
        let g = read_section_geom(&d, d.root);
        assert_eq!(g["PW"].as_f64(), Some(200.0));
        assert_eq!(g["BODY_W"].as_f64(), Some(163.0));
        assert_eq!(g["BODY_H"].as_f64(), Some(82.0));
        assert_eq!(g["mirrorDx"].as_f64(), Some(3.0));
    }
    #[test]
    fn repeated_cache_is_not_an_ordinary_page_break() {
        let p = "<p><linesegarray><lineseg vertpos='0' vertsize='100'/></linesegarray></p>";
        let d = parse_style_xml(&format!("<sec>{p}{p}{p}</sec>")).unwrap();
        assert!(stale_top_cache(&d, d.root, 1000.0));
        let d=parse_style_xml(&format!("<sec>{p}{p}<p pageBreak='1'><linesegarray><lineseg vertpos='0'/></linesegarray></p></sec>")).unwrap();
        assert!(!stale_top_cache(&d, d.root, 1000.0));
    }
}
