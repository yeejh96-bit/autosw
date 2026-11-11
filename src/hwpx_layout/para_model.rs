//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Paragraph/control/cell model; see data/kordoc-LICENSE and PROVENANCE.
use super::{
    layout, notes,
    style_dom::{ParsedXml, XmlPart},
    value::{Value, native_json as json},
};
use std::collections::HashMap;

pub const FORM_TAGS: &[&str] = &[
    "checkBtn",
    "radioBtn",
    "btn",
    "comboBox",
    "listBox",
    "edit",
    "scrollBar",
];
pub const OBJ_TAGS: &[&str] = &[
    "tbl",
    "pic",
    "container",
    "equation",
    "rect",
    "ellipse",
    "polygon",
    "curv",
    "curve",
    "line",
    "arc",
    "ole",
    "textart",
    "checkBtn",
    "radioBtn",
    "btn",
    "comboBox",
    "listBox",
    "edit",
    "scrollBar",
];
pub const DEFAULT_TAB_HU: f64 = 4000.0;
pub fn child(d: &ParsedXml, i: usize, name: &str) -> Option<usize> {
    d.nodes[i]
        .children
        .iter()
        .copied()
        .find(|&c| d.nodes[c].local == name)
}
pub fn attr<'a>(d: &'a ParsedXml, i: Option<usize>, name: &str) -> Option<&'a str> {
    i.and_then(|i| d.nodes[i].attr(name))
}
pub fn num(d: &ParsedXml, i: Option<usize>, name: &str, fallback: f64) -> f64 {
    match attr(d, i, name) {
        None | Some("") => fallback,
        Some(s) => layout::to_int32(super::value::number(&s.into()), fallback),
    }
}
pub fn find_first(d: &ParsedXml, i: usize, name: &str, depth: usize) -> Option<usize> {
    if depth > 64 {
        return None;
    }
    for &c in &d.nodes[i].children {
        if d.nodes[c].local == name {
            return Some(c);
        }
        if let Some(found) = find_first(d, c, name, depth + 1) {
            return Some(found);
        }
    }
    None
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
fn n(v: &Value) -> f64 {
    super::value::number(v)
}
fn round(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        v
    } else {
        let low = v.floor();
        let out = if v - low < 0.5 { low } else { low + 1.0 };
        if out == 0.0 && v < 0.0 { -0.0 } else { out }
    }
}
fn node_id(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|v| usize::try_from(v).ok())
}

#[derive(Default)]
pub struct Context {
    pub deleted_starts: HashMap<usize, usize>,
    pub deleted_controls: HashMap<usize, bool>,
    pub note_formats: HashMap<usize, Value>,
    pub memoize: bool,
    pub cell_memo: HashMap<usize, f64>,
    pub table_memo: HashMap<usize, f64>,
    pub para_geom: HashMap<String, Value>,
}
impl Context {
    pub fn is_deleted_control(&self, node: usize) -> bool {
        self.deleted_controls.get(&node) == Some(&true)
    }
}
/// Memo-free legacy measurements still share document deletion and note state.
pub fn without_extent_memo<T>(
    ctx: &mut Context,
    operation: impl FnOnce(&mut Context) -> Result<T, String>,
) -> Result<T, String> {
    let memoize = ctx.memoize;
    ctx.memoize = false;
    let result = operation(ctx);
    ctx.memoize = memoize;
    result
}
fn deletion_delta(d: &ParsedXml, i: usize, mut deleted: usize, depth: usize) -> usize {
    if depth > 64 {
        return deleted;
    }
    match d.nodes[i].local.as_str() {
        "deleteBegin" => deleted + 1,
        "deleteEnd" => deleted.saturating_sub(1),
        "ctrl" => {
            for &c in &d.nodes[i].children {
                deleted = deletion_delta(d, c, deleted, depth + 1);
            }
            deleted
        }
        _ => deleted,
    }
}
pub fn prepare_deleted_ranges(d: &ParsedXml, root: usize, depth: usize, ctx: &mut Context) {
    if depth > 64 {
        return;
    }
    let mut deleted = 0;
    fn scan(d: &ParsedXml, i: usize, depth: usize, deleted: &mut usize, ctx: &mut Context) {
        if depth > 64 {
            return;
        }
        if matches!(d.nodes[i].local.as_str(), "ctrl" | "t") {
            for &c in &d.nodes[i].children {
                scan(d, c, depth + 1, deleted, ctx);
            }
        } else {
            if matches!(d.nodes[i].local.as_str(), "deleteBegin" | "deleteEnd") {
                *deleted = deletion_delta(d, i, *deleted, 0);
            }
            ctx.deleted_controls.insert(i, *deleted > 0);
        }
    }
    for &p in &d.nodes[root].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        ctx.deleted_starts.insert(p, deleted);
        for &c in &d.nodes[p].children {
            match d.nodes[c].local.as_str() {
                "run" => {
                    for &e in &d.nodes[c].children {
                        scan(d, e, depth + 2, &mut deleted, ctx);
                    }
                }
                "deleteBegin" | "deleteEnd" => scan(d, c, depth + 1, &mut deleted, ctx),
                _ => {}
            }
        }
    }
    fn nested(d: &ParsedXml, i: usize, depth: usize, ctx: &mut Context) {
        if depth > 64 {
            return;
        }
        for &c in &d.nodes[i].children {
            if d.nodes[c].local == "subList" {
                prepare_deleted_ranges(d, c, depth + 1, ctx);
            } else {
                nested(d, c, depth + 1, ctx);
            }
        }
    }
    nested(d, root, depth, ctx);
}
pub fn push_fillers(chars: &mut Vec<Value>, count: usize, pr_id: Option<&str>) {
    for _ in 0..count {
        chars.push(json!({"ch":"","prId":pr_id}));
    }
}
pub fn push_text_slots(
    d: &ParsedXml,
    t: usize,
    chars: &mut Vec<Value>,
    pr_id: Option<&str>,
    depth: usize,
    deleted: &mut usize,
) {
    if depth > 32 {
        return;
    }
    for part in &d.nodes[t].parts {
        match part {
            XmlPart::Text(text) => {
                for cp in text.chars() {
                    chars.push(
                        json!({"ch":if *deleted>0{String::new()}else{cp.to_string()},"prId":pr_id}),
                    );
                    if cp.len_utf16() == 2 {
                        push_fillers(chars, 1, pr_id);
                    }
                }
            }
            XmlPart::Node(i) => {
                let tag = d.nodes[*i].local.as_str();
                match tag {
                    "deleteBegin" | "deleteEnd" => *deleted = deletion_delta(d, *i, *deleted, 0),
                    "tab" => {
                        if *deleted > 0 {
                            push_fillers(chars, 8, pr_id);
                        } else {
                            let width = num(d, Some(*i), "width", -1.0);
                            let mut c = json!({"ch":"","prId":pr_id,"tab":true});
                            if width >= 0.0 {
                                c["tabW"] = width.into();
                            }
                            if d.nodes[*i].attr("type") == Some("2") {
                                c["tabRight"] = true.into();
                            }
                            chars.push(c);
                            push_fillers(chars, 7, pr_id);
                        }
                    }
                    "titleMark" => push_fillers(chars, 8, pr_id),
                    "lineBreak" | "hyphen" | "hypen" | "nbSpace" | "fwSpace" => {
                        let mut c = json!({"ch":"","prId":pr_id});
                        if *deleted == 0 {
                            match tag {
                                "nbSpace" => {
                                    c["ch"] = " ".into();
                                    c["nb"] = true.into();
                                }
                                "lineBreak" => c["br"] = true.into(),
                                "fwSpace" => c["ch"] = "\u{2005}".into(),
                                _ => {}
                            }
                        }
                        chars.push(c);
                    }
                    _ => push_text_slots(d, *i, chars, pr_id, depth + 1, deleted),
                }
            }
        }
    }
}
pub fn build_para(d: &ParsedXml, p: usize, ctx: &mut Context) -> Result<Value, String> {
    let mut chars = Vec::new();
    let mut objs = Vec::new();
    let mut segs: Vec<Value> = Vec::new();
    let mut runs = Vec::new();
    let mut deleted = ctx.deleted_starts.get(&p).copied().unwrap_or(0);
    let mut sec_slots = 0;
    for &run in &d.nodes[p].children {
        match d.nodes[run].local.as_str() {
            "run" => {
                let pr_id = d.nodes[run].attr("charPrIDRef");
                runs.push(json!({"at":chars.len(),"prId":pr_id}));
                for &c in &d.nodes[run].children {
                    let tag = d.nodes[c].local.as_str();
                    if tag == "t" {
                        push_text_slots(d, c, &mut chars, pr_id, 0, &mut deleted);
                    } else if OBJ_TAGS.contains(&tag) {
                        let sz = child(d, c, "sz");
                        let pos = child(d, c, "pos");
                        let cur = child(d, c, "curSz");
                        let org = child(d, c, "orgSz");
                        let rot = num(d, child(d, c, "rotationInfo"), "angle", 0.0) % 360.0;
                        let boxed = child(d, c, "drawText").is_some()
                            && rot == 0.0
                            && num(d, cur, "width", 0.0) > 0.0
                            && num(d, cur, "height", 0.0) > 0.0
                            && num(d, cur, "height", 0.0) < 2147483648.0;
                        let dimension = |name| {
                            if boxed {
                                num(d, cur, name, 0.0)
                            } else {
                                let a = num(d, sz, name, 0.0);
                                let b = num(d, cur, name, 0.0);
                                if a != 0.0 {
                                    a
                                } else if b != 0.0 {
                                    b
                                } else {
                                    num(d, org, name, 0.0)
                                }
                            }
                        };
                        let inline = attr(d, pos, "treatAsChar") == Some("1");
                        let om = if inline {
                            child(d, c, "outMargin")
                        } else {
                            None
                        };
                        if deleted == 0 {
                            objs.push(json!({"el":c,"tag":tag,"index":chars.len(),"inline":inline,"width":dimension("width"),"height":dimension("height"),"omL":num(d,om,"left",0.0),"omR":num(d,om,"right",0.0)}));
                        }
                        let at = chars.len();
                        push_fillers(&mut chars, 8, pr_id);
                        if !inline {
                            chars[at]["ctrl"] = true.into();
                        }
                    } else {
                        if matches!(tag, "ctrl" | "deleteBegin" | "deleteEnd") {
                            deleted = deletion_delta(d, c, deleted, 0);
                        }
                        let at = chars.len();
                        push_fillers(&mut chars, 8, pr_id);
                        if tag == "secPr" || tag == "ctrl" && child(d, c, "colPr").is_some() {
                            sec_slots += 8;
                        } else {
                            let slot = &mut chars[at];
                            slot["ctrl"] = true.into();
                            let an = if tag == "ctrl" {
                                child(d, c, "autoNum")
                            } else {
                                None
                            };
                            let note = if tag == "ctrl" {
                                child(d, c, "footNote").or_else(|| child(d, c, "endNote"))
                            } else {
                                None
                            };
                            let text = if let Some(an) = an {
                                super::layout_text::LayoutText::from(notes::note_auto_num_of(
                                    d, an,
                                )?)
                            } else if let Some(note) = note {
                                let root = d.document_root(note)?;
                                let formats = ctx
                                    .note_formats
                                    .entry(root)
                                    .or_insert_with(|| notes::read_section_note_formats(d, root));
                                let key = if d.nodes[note].local == "endNote" {
                                    "endnote"
                                } else {
                                    "footnote"
                                };
                                notes::note_ref_mark_utf16(
                                    &notes::note_attrs_of(d, note),
                                    formats.get(key),
                                )?
                                .into()
                            } else {
                                super::layout_text::LayoutText::default()
                            };
                            if !text.0.is_empty() {
                                if let Ok(text) = text.to_string_strict() {
                                    slot["ctrlText"] = text.into();
                                } else {
                                    slot["ctrlTextUnits"] = json!(text.0);
                                }
                            }
                            if let Some(note) = note {
                                slot["ctrlSup"] = true.into();
                                slot["note"] = note.into();
                            }
                            if tag == "ctrl" && child(d, c, "fieldEnd").is_some() {
                                slot["fieldEnd"] = true.into();
                            }
                            if tag == "compose"
                                && d.nodes[c]
                                    .attr("composeText")
                                    .is_some_and(|s| !s.is_empty())
                            {
                                slot["ctrlText"] = "\u{3000}".into();
                            }
                        }
                    }
                }
            }
            "deleteBegin" | "deleteEnd" => deleted = deletion_delta(d, run, deleted, 0),
            "linesegarray" => {
                segs=d.nodes[run].children.iter().copied().filter(|&i|d.nodes[i].local=="lineseg").map(|i|{let th=num(d,Some(i),"textheight",1000.0);let bl=num(d,Some(i),"baseline",850.0);json!({"textpos":num(d,Some(i),"textpos",0.0),"vertpos":num(d,Some(i),"vertpos",0.0),"horzpos":num(d,Some(i),"horzpos",0.0),"horzsize":num(d,Some(i),"horzsize",0.0),"textheight":th,"baseline":if bl==0.0&&th>0.0{round(th*0.85)}else{bl},"spacing":num(d,Some(i),"spacing",0.0)})}).collect();
            }
            _ => {}
        }
    }
    if sec_slots > 0 && segs.len() > 1 {
        let mut lead = 0;
        while lead < chars.len()
            && chars[lead]["ch"].as_str() == Some("")
            && !truth(&chars[lead]["ctrl"])
            && !objs
                .iter()
                .any(|o| o["index"].as_u64() == Some(lead as u64))
        {
            lead += 1;
        }
        if segs[1..].iter().any(|s| n(&s["textpos"]) < lead as f64) {
            for s in &mut segs[1..] {
                s["textpos"] = (n(&s["textpos"]) + sec_slots as f64).into();
            }
        }
    }
    Ok(
        json!({"chars":chars,"segs":segs,"objs":objs,"paraPrId":d.nodes[p].attr("paraPrIDRef"),"runs":runs}),
    )
}
pub fn caption_of(d: &ParsedXml, i: usize) -> Option<Value> {
    let cap = child(d, i, "caption")?;
    let sub = child(d, cap, "subList")?;
    let mut h: f64 = 0.0;
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        if let Some(lsa) = child(d, p, "linesegarray") {
            for &s in &d.nodes[lsa].children {
                if d.nodes[s].local == "lineseg" {
                    h = layout::maximum(
                        h,
                        num(d, Some(s), "vertpos", 0.0) + num(d, Some(s), "vertsize", 0.0),
                    );
                }
            }
        }
    }
    let side = d.nodes[cap].attr("side").unwrap_or("BOTTOM");
    let w = num(
        d,
        Some(cap),
        if matches!(side, "LEFT" | "RIGHT") {
            "width"
        } else {
            "lastWidth"
        },
        0.0,
    );
    Some(
        json!({"side":side,"h":h,"gap":num(d,Some(cap),"gap",0.0),"width":if w!=0.0{w}else{num(d,Some(cap),"width",0.0)},"sub":sub}),
    )
}
pub fn caption_extra(d: &ParsedXml, i: usize) -> f64 {
    caption_of(d, i)
        .filter(|v| matches!(v["side"].as_str(), Some("TOP" | "BOTTOM")))
        .map_or(0.0, |v| n(&v["h"]) + n(&v["gap"]))
}
pub fn para_head_advance(head: &Value, offset_height: f64, width: f64) -> f64 {
    width
        + if truth(&head["offsetPct"]) {
            (offset_height * n(&head["offset"]) / 100.0).trunc()
        } else {
            n(&head["offset"])
        }
}
pub fn tab_advance(x: f64, first_line: bool, geom: Option<&Value>, saved: Option<f64>) -> f64 {
    let g = geom.unwrap_or(&Value::Undefined);
    let intent = n(&g["marginIntent"]);
    let hang = if intent < 0.0 { -intent } else { 0.0 };
    if first_line && truth(&g["autoTabLeft"]) && hang > 0.0 && x < hang - 0.5 {
        return hang - x;
    }
    if let Some(s) = saved.filter(|s| *s >= 0.0) {
        return s;
    }
    ((x / DEFAULT_TAB_HU).floor() + 1.0) * DEFAULT_TAB_HU - x
}
pub fn cell_columns(d: &ParsedXml, sub: Option<usize>) -> Option<Value> {
    let p = sub.and_then(|s| {
        d.nodes[s]
            .children
            .iter()
            .copied()
            .find(|&i| d.nodes[i].local == "p")
    });
    let cp = p.and_then(|p| find_first(d, p, "colPr", 0));
    let count = cp.map_or(1.0, |i| {
        let value = super::value::number(&d.nodes[i].attr("colCount").into());
        if value == 0.0 || value.is_nan() {
            1.0
        } else {
            value
        }
    });
    (count > 1.0).then(|| json!({"n":count,"gap":num(d,cp,"sameGap",0.0)}))
}
pub fn cell_margin_of(d: &ParsedXml, cm: Option<usize>, side: &str, default: f64) -> f64 {
    let v = num(d, cm, side, default);
    if v >= 32768.0 { default } else { v }
}
pub fn collect_cells(d: &ParsedXml, tbl: usize) -> Vec<Value> {
    let im = child(d, tbl, "inMargin");
    let mut cells = Vec::new();
    for &tr in &d.nodes[tbl].children {
        if d.nodes[tr].local != "tr" {
            continue;
        }
        for &tc in &d.nodes[tr].children {
            if d.nodes[tc].local != "tc" {
                continue;
            }
            let addr = child(d, tc, "cellAddr");
            let span = child(d, tc, "cellSpan");
            let sz = child(d, tc, "cellSz");
            if addr.is_none() || sz.is_none() {
                continue;
            }
            let cm = if d.nodes[tc].attr("hasMargin") == Some("0") {
                None
            } else {
                child(d, tc, "cellMargin")
            };
            let mut cell = json!({"el":tc,"ca":layout::maximum(0.0,num(d,addr,"colAddr",0.0)),"ra":layout::maximum(0.0,num(d,addr,"rowAddr",0.0)),"cs":layout::maximum(1.0,num(d,span,"colSpan",1.0)),"rs":layout::maximum(1.0,num(d,span,"rowSpan",1.0)),"w":num(d,sz,"width",0.0),"h":num(d,sz,"height",0.0),"bfId":d.nodes[tc].attr("borderFillIDRef"),"sub":child(d,tc,"subList")});
            for (side, key) in [
                ("left", "marginL"),
                ("right", "marginR"),
                ("top", "marginT"),
                ("bottom", "marginB"),
            ] {
                cell[key] = cell_margin_of(d, cm, side, num(d, im, side, 141.0)).into();
            }
            cells.push(cell);
        }
    }
    cells
}
pub fn cell_content_extent(d: &ParsedXml, cell: &Value, ctx: &mut Context) -> Result<f64, String> {
    let Some(sub) = node_id(&cell["sub"]) else {
        return Ok(0.0);
    };
    let el = node_id(&cell["el"]).ok_or("셀 노드가 없습니다.")?;
    if ctx.memoize
        && let Some(hit) = ctx.cell_memo.get(&el)
    {
        return Ok(*hit);
    }
    let cols = cell_columns(d, Some(sub));
    let mut ext: f64 = 0.0;
    let mut shift = 0.0;
    let mut prev_v = f64::NEG_INFINITY;
    let mut prev_h = 0.0;
    let mut prev_end = 0.0;
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        let model = build_para(d, p, ctx)?;
        let segs = model["segs"].as_array().unwrap();
        let para_start = prev_end;
        let mut first_shift = shift;
        let mut line_tops = Vec::new();
        for (k, s) in segs.iter().enumerate() {
            let v = n(&s["vertpos"]);
            let h = n(&s["horzpos"]);
            if (v < prev_v || v == prev_v && h <= prev_h && prev_end - shift > v) && cols.is_none()
            {
                shift = prev_end - v;
            }
            if k == 0 {
                first_shift = shift;
            }
            line_tops.push(v + shift);
            prev_v = v;
            prev_h = h;
            prev_end = v + shift + n(&s["textheight"]) + n(&s["spacing"]);
            ext = layout::maximum(ext, v + n(&s["textheight"]) + shift);
        }
        let base_v = segs.first().map_or(0.0, |s| n(&s["vertpos"])) + first_shift;
        for o in model["objs"].as_array().unwrap() {
            let node = node_id(&o["el"]).ok_or("개체 노드가 없습니다.")?;
            let height = n(&o["height"]);
            if truth(&o["inline"]) {
                let h = if o["tag"].as_str() == Some("tbl") {
                    layout::maximum(height, measure_table_height(d, node, ctx)?)
                } else {
                    height
                };
                let mut line = 0;
                while line + 1 < segs.len() && n(&segs[line + 1]["textpos"]) <= n(&o["index"]) {
                    line += 1;
                }
                ext = layout::maximum(ext, line_tops.get(line).copied().unwrap_or(base_v) + h);
                continue;
            }
            let pos = child(d, node, "pos");
            if attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA"
                || attr(d, pos, "flowWithText") == Some("0")
            {
                continue;
            }
            let om = child(d, node, "outMargin");
            let vo = num(d, pos, "vertOffset", 0.0);
            let top = num(d, om, "top", 0.0);
            let bottom = num(d, om, "bottom", 0.0);
            let pushed = base_v - (layout::maximum(0.0, vo) + top + height + bottom);
            let space_before = if ctx.memoize {
                ctx.para_geom
                    .get(d.nodes[p].attr("paraPrIDRef").unwrap_or(""))
                    .and_then(|g| g.get("spaceBefore"))
                    .map_or(0.0, n)
            } else {
                0.0
            };
            let anchor = if pushed
                >= if para_start > 0.0 {
                    para_start - 100.0
                } else {
                    -100.0
                } {
                pushed
            } else {
                base_v - space_before
            };
            let oh = if o["tag"].as_str() == Some("tbl") {
                layout::maximum(height, measure_table_height(d, node, ctx)?)
            } else {
                height
            };
            ext = layout::maximum(ext, anchor + top + layout::maximum(0.0, vo) + oh + bottom);
        }
    }
    if ctx.memoize {
        ctx.cell_memo.insert(el, ext);
    }
    Ok(ext)
}
fn in_cell(d: &ParsedXml, i: usize) -> bool {
    let mut p = d.nodes[i].parent;
    while let Some(i) = p {
        if d.nodes[i].local == "tc" {
            return true;
        }
        p = d.nodes[i].parent;
    }
    false
}
fn split_saved(d: &ParsedXml, cell: &Value, ctx: &mut Context) -> Result<bool, String> {
    let Some(sub) = node_id(&cell["sub"]) else {
        return Ok(false);
    };
    let mut prev = f64::NEG_INFINITY;
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        let model = build_para(d, p, ctx)?;
        for s in model["segs"].as_array().unwrap() {
            let v = n(&s["vertpos"]);
            if v < prev {
                return Ok(true);
            }
            prev = v;
        }
    }
    Ok(false)
}
pub fn measure_table_row_heights(
    d: &ParsedXml,
    tbl: usize,
    ctx: &mut Context,
) -> Result<Vec<f64>, String> {
    let cells = collect_cells(d, tbl);
    if cells.is_empty() || cells.len() > 4096 {
        return Ok(Vec::new());
    }
    let count = cells
        .iter()
        .map(|c| n(&c["ra"]) + n(&c["rs"]))
        .fold(f64::NEG_INFINITY, layout::maximum);
    if !count.is_finite() || count < 0.0 || count.fract() != 0.0 {
        return Err("Invalid array length".into());
    }
    if count > 1_000_000.0 {
        return Err("표 행 계산 한도를 넘었습니다.".into());
    }
    let sp = num(d, Some(tbl), "cellSpacing", 0.0);
    let mut row_cells = Vec::new();
    for c in &cells {
        let content = cell_content_extent(d, c, ctx)? + n(&c["marginT"]) + n(&c["marginB"]);
        let span = n(&c["rs"]);
        row_cells.push(layout::CellRow {
            row_addr: n(&c["ra"]),
            row_span: span,
            height: if span > 1.0 {
                layout::maximum(n(&c["h"]), content)
            } else {
                n(&c["h"])
            } - sp * (span - 1.0),
            content_h: Some(content),
        });
    }
    let maximum = num(d, child(d, tbl, "sz"), "height", 0.0)
        - if sp > 0.0 { sp * (count + 1.0) } else { 0.0 };
    let mut shrink = !in_cell(d, tbl);
    if shrink {
        for c in &cells {
            if n(&c["ra"]) + n(&c["rs"]) == count && split_saved(d, c, ctx)? {
                shrink = false;
                break;
            }
        }
    }
    let mut rows = layout::solve_row_heights(
        &row_cells,
        count as usize,
        if maximum != 0.0 && !maximum.is_nan() {
            maximum
        } else {
            f64::INFINITY
        },
        shrink,
    );
    if sp > 0.0 {
        for r in &mut rows {
            *r += sp;
        }
        if let Some(last) = rows.last_mut() {
            *last += sp;
        }
    }
    Ok(rows)
}
pub fn measure_table_height(d: &ParsedXml, tbl: usize, ctx: &mut Context) -> Result<f64, String> {
    if ctx.memoize
        && let Some(hit) = ctx.table_memo.get(&tbl)
    {
        return Ok(*hit);
    }
    let rows = measure_table_row_heights(d, tbl, ctx)?;
    let mut sum = 0.0;
    for h in rows {
        sum += h;
    }
    if ctx.memoize {
        ctx.table_memo.insert(tbl, sum);
    }
    Ok(sum)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    let mut ctx = Context {
        memoize: truth(&input["memo"]),
        ..Context::default()
    };
    if let Some(entries) = input["paraGeom"].as_array() {
        for entry in entries {
            if let Some(key) = entry[0].as_str() {
                ctx.para_geom.insert(key.into(), entry[1].clone());
            }
        }
    }
    let out = match operation {
        "tabAdvance" => json!(tab_advance(
            n(&input["x"]),
            truth(&input["firstLine"]),
            input.get("geom"),
            input
                .get("saved")
                .filter(|v| !matches!(v, Value::Null | Value::Undefined))
                .map(n)
        )),
        "paraHeadAdvance" => json!(para_head_advance(
            &input["head"],
            n(&input["offsetHeight"]),
            n(&input["width"])
        )),
        _ => {
            let d = super::style_dom::parse_style_xml(
                input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?,
            )?;
            let mut root = d.root;
            if let Some(path) = input["path"].as_array() {
                for i in path {
                    root = *d.nodes[root]
                        .children
                        .get(i.as_u64().ok_or("잘못된 노드 경로")? as usize)
                        .ok_or("노드 경로가 없습니다.")?;
                }
            }
            if truth(&input["prepare"]) {
                prepare_deleted_ranges(&d, d.root, 0, &mut ctx);
            }
            match operation {
                "buildPara" => build_para(&d, root, &mut ctx)?,
                "paragraphs" => {
                    let mut out = Vec::new();
                    for (i, node) in d.nodes.iter().enumerate() {
                        if node.local == "p" {
                            out.push(json!({"el":i,"model":build_para(&d,i,&mut ctx)?}));
                        }
                    }
                    json!({"paragraphs":out,"deletedControls":(0..d.nodes.len()).map(|i|ctx.is_deleted_control(i)).collect::<Vec<_>>()})
                }
                "captionOf" => caption_of(&d, root).into(),
                "captionExtra" => caption_extra(&d, root).into(),
                "cellColumns" => cell_columns(&d, Some(root)).into(),
                "collectCells" => collect_cells(&d, root).into(),
                "measureTableRowHeights" => measure_table_row_heights(&d, root, &mut ctx)?.into(),
                "measureTableHeight" => measure_table_height(&d, root, &mut ctx)?.into(),
                "cellContentExtent" => {
                    let cells = collect_cells(&d, root);
                    let i = input.get("cellIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
                    cell_content_extent(&d, cells.get(i).ok_or("선택한 셀이 없습니다.")?, &mut ctx)?
                        .into()
                }
                "pushTextSlots" => {
                    let mut chars = Vec::new();
                    let mut deleted =
                        input.get("deleted").and_then(Value::as_u64).unwrap_or(0) as usize;
                    push_text_slots(
                        &d,
                        root,
                        &mut chars,
                        input["prId"].as_str(),
                        0,
                        &mut deleted,
                    );
                    json!({"chars":chars,"deleted":deleted})
                }
                _ => return Err("알 수 없는 문단 모델 연산입니다.".into()),
            }
        }
    };
    Ok(out.to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_arena_keeps_note_formats_per_original_document() {
        let xml = |prefix: &str| {
            format!(
                "<hp:sec xmlns:hp=\"urn:hp\"><hp:footNotePr><hp:autoNumFormat type=\"DIGIT\" prefixChar=\"{prefix}\" suffixChar=\"!\"/></hp:footNotePr><hp:p><hp:run><hp:ctrl><hp:footNote number=\"2\"/></hp:ctrl></hp:run></hp:p></hp:sec>"
            )
        };
        let mut d = super::super::style_dom::parse_style_xml(&xml("A")).unwrap();
        let root_a = d.root;
        let root_b = d
            .import_document(super::super::style_dom::parse_style_xml(&xml("B")).unwrap())
            .unwrap();
        let p_a = d.nodes[root_a].children[1];
        let p_b = d.nodes[root_b].children[1];
        let mut ctx = Context::default();
        assert_eq!(
            build_para(&d, p_a, &mut ctx).unwrap()["chars"][0]["ctrlText"],
            "A2!"
        );
        assert_eq!(
            build_para(&d, p_b, &mut ctx).unwrap()["chars"][0]["ctrlText"],
            "B2!"
        );
        assert_eq!(
            build_para(&d, p_a, &mut ctx).unwrap()["chars"][0]["ctrlText"],
            "A2!"
        );
        assert_eq!(ctx.note_formats.len(), 2);
    }
    #[test]
    fn uncached_measurement_retains_deletions_and_restores_cache_on_failure() {
        let mut ctx = Context {
            memoize: true,
            ..Context::default()
        };
        ctx.deleted_starts.insert(7, 1);
        ctx.cell_memo.insert(9, 1200.0);
        let result: Result<(), String> = without_extent_memo(&mut ctx, |ctx| {
            assert!(!ctx.memoize);
            assert_eq!(ctx.deleted_starts.get(&7), Some(&1));
            Err("합성 측정 오류".into())
        });
        assert!(result.is_err());
        assert!(ctx.memoize);
        assert_eq!(ctx.cell_memo.get(&9), Some(&1200.0));
    }
}
