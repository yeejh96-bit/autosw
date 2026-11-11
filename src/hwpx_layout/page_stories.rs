//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/page-stories.mjs; see data/kordoc-LICENSE and kordoc-PROVENANCE.
//! Node handles refer to the caller's DOM arena. Only the probe serializes them.
#[cfg(test)]
use super::value::native_json as json;
#[cfg(test)]
use super::{para_model, style_dom::parse_style_xml};
use super::{
    para_model::{Context, build_para, child, find_first, num},
    style_dom::ParsedXml,
    value::{Value, number, string},
};
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

pub type ParaPages = HashMap<usize, Vec<Value>>;
pub struct PageOptions<'a> {
    pub count: &'a Value,
    pub base: &'a Value,
    pub geometry: &'a Value,
}
#[derive(Clone, Debug)]
pub struct Story {
    pub sub: usize,
    pub page: Value,
    pub x: Value,
    pub y: Value,
    pub width: Value,
    pub layer: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub page: Value,
    pub x: Value,
    pub y: Value,
    pub length: f64,
    pub width_mm: f64,
    pub color: String,
}
#[derive(Clone, Debug)]
pub struct FootNoteStories {
    pub stories: Vec<Story>,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug)]
pub struct EndNotePara {
    pub p: usize,
    pub pages: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct EndNoteLayout {
    pub paras: Vec<EndNotePara>,
    pub rules: Vec<Rule>,
    pub page_count: Value,
}

fn nullish(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Undefined)
}
fn page_at(pages: &[Value], at: usize, fallback: &Value) -> Value {
    pages
        .get(at)
        .filter(|value| !nullish(value))
        .unwrap_or(fallback)
        .clone()
}
fn same_key(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b || a.is_nan() && b.is_nan(),
        _ => a == b,
    }
}
fn map_key(value: &Value) -> Value {
    // JavaScript Map canonicalizes a negative-zero key to positive zero.
    if matches!(value,Value::Number(n) if *n==0.0) {
        0.0.into()
    } else {
        value.clone()
    }
}
fn primitive_string(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Array(_) | Value::Object(_))
}
fn add(a: &Value, b: &Value) -> Value {
    if primitive_string(a) || primitive_string(b) {
        format!("{}{}", string(a), string(b)).into()
    } else {
        (number(a) + number(b)).into()
    }
}
fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() && b.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else if a > b {
        a
    } else {
        b
    }
}
fn min(a: f64, b: f64) -> f64 {
    -max(-a, -b)
}
fn round(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let low = value.floor();
    let result = if value - low < 0.5 { low } else { low + 1.0 };
    if result == 0.0 && value < 0.0 {
        -0.0
    } else {
        result
    }
}
fn truth_number(n: f64) -> bool {
    n != 0.0 && !n.is_nan()
}
fn attr<'a>(d: &'a ParsedXml, node: Option<usize>, key: &str) -> Option<&'a str> {
    node.and_then(|i| d.nodes[i].attr(key))
}
fn direct(d: &ParsedXml, root: usize, name: &str) -> Vec<usize> {
    d.nodes[root]
        .children
        .iter()
        .copied()
        .filter(|&i| d.nodes[i].local == name)
        .collect()
}
fn controls(d: &ParsedXml, p: usize) -> Vec<usize> {
    direct(d, p, "run")
        .into_iter()
        .flat_map(|run| direct(d, run, "ctrl"))
        .collect()
}
fn segs(d: &ParsedXml, p: usize) -> Vec<usize> {
    child(d, p, "linesegarray").map_or_else(Vec::new, |lsa| direct(d, lsa, "lineseg"))
}
fn story(sub: usize, page: Value, geometry: &Value, y: Value, layer: Option<f64>) -> Story {
    Story {
        sub,
        page,
        x: geometry["ML"].clone(),
        y,
        width: geometry["BODY_W"].clone(),
        layer,
    }
}

/// Master-page and header/footer selection retains document declaration order.
pub fn page_stories(
    d: &ParsedXml,
    root: usize,
    masters: &[usize],
    pages: &ParaPages,
    options: PageOptions<'_>,
    ctx: &Context,
) -> Result<Vec<Story>, String> {
    let count = number(options.count);
    if count > 100_000.0 {
        return Err("머리말·바탕쪽 배치의 100,000쪽 처리 한도를 넘었습니다.".into());
    }
    let geometry = options.geometry;
    let section = find_first(d, root, "secPr", 0);
    let visibility = section.and_then(|i| child(d, i, "visibility"));
    let margin = section
        .and_then(|i| child(d, i, "pagePr"))
        .and_then(|i| child(d, i, "margin"));
    let mut declarations: Vec<(usize, Value, &str)> = Vec::new();
    let mut hidden: Vec<(Value, HashSet<&str>)> = Vec::new();
    for p in direct(d, root, "p") {
        let page = page_at(pages.get(&p).map_or(&[], Vec::as_slice), 0, &0.into());
        for ctrl in controls(d, p) {
            for &el in &d.nodes[ctrl].children {
                if ctx.is_deleted_control(el) {
                    continue;
                }
                let kind = d.nodes[el].local.as_str();
                if matches!(kind, "header" | "footer") {
                    declarations.push((el, page.clone(), kind));
                } else if kind == "pageHiding" {
                    let at = hidden
                        .iter()
                        .position(|(key, _)| same_key(key, &page))
                        .unwrap_or_else(|| {
                            hidden.push((page.clone(), HashSet::new()));
                            hidden.len() - 1
                        });
                    for name in ["Header", "Footer", "MasterPage"] {
                        if num(d, Some(el), &format!("hide{name}"), 0.0) != 0.0 {
                            hidden[at].1.insert(name);
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut page = 0usize;
    while (page as f64) < count {
        let page_value: Value = page.into();
        let n = number(&add(&add(options.base, &page_value), &1.into()));
        let hides = |name: &str| {
            hidden
                .iter()
                .any(|(key, set)| same_key(key, &page_value) && set.contains(name))
                || page == 0 && num(d, visibility, &format!("hideFirst{name}"), 0.0) != 0.0
        };
        if !hides("MasterPage") {
            let parity = if truth_number(n % 2.0) { "ODD" } else { "EVEN" };
            let matching = masters
                .iter()
                .copied()
                .rfind(|&m| d.nodes[m].attr("type") == Some(parity));
            let both = masters.iter().copied().rfind(|&m| {
                d.nodes[m]
                    .attr("type")
                    .filter(|s| !s.is_empty())
                    .unwrap_or("BOTH")
                    == "BOTH"
            });
            let normal = matching.or(both);
            let special = masters.iter().copied().rfind(|&m| {
                if d.nodes[m].attr("type") == Some("LAST_PAGE") {
                    page as f64 == count - 1.0
                } else {
                    d.nodes[m].attr("type") == Some("OPTIONAL_PAGE")
                        && num(d, Some(m), "pageNumber", 0.0) == page as f64 + 1.0
                }
            });
            let mut selected = Vec::new();
            if special.is_some_and(|m| num(d, Some(m), "pageDuplicate", 0.0) == 0.0) {
                selected.extend(special);
            } else {
                selected.extend(normal);
                selected.extend(special);
            }
            for master in selected {
                if let Some(sub) = child(d, master, "subList") {
                    out.push(story(
                        sub,
                        page_value.clone(),
                        geometry,
                        geometry["MT"].clone(),
                        Some(if num(d, Some(master), "pageFront", 0.0) != 0.0 {
                            2.0
                        } else {
                            -2.0
                        }),
                    ));
                }
            }
        }
        for kind in ["header", "footer"] {
            if hides(if kind == "header" { "Header" } else { "Footer" }) {
                continue;
            }
            let declaration = declarations.iter().rfind(|(el, start, k)| {
                let apply = d.nodes[*el]
                    .attr("applyPageType")
                    .filter(|s| !s.is_empty())
                    .unwrap_or("BOTH");
                *k == kind
                    && number(start) <= page as f64
                    && (apply == "BOTH"
                        || apply == "ODD" && n % 2.0 == 1.0
                        || apply == "EVEN" && n % 2.0 == 0.0)
            });
            if let Some(sub) = declaration.and_then(|(el, _, _)| child(d, *el, "subList")) {
                let y = if kind == "header" {
                    num(d, margin, "top", 5668.0)
                } else {
                    number(&geometry["PH"])
                        - num(d, margin, "bottom", 4252.0)
                        - num(d, margin, "footer", 0.0)
                };
                out.push(story(sub, page_value.clone(), geometry, y.into(), None));
            }
        }
        page += 1;
    }
    Ok(out)
}

static FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[+-]?(?:Infinity|(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)").unwrap()
});
fn line_width(d: &ParsedXml, line: Option<usize>) -> f64 {
    let source = attr(d, line, "width")
        .unwrap_or("0.12")
        .trim_start_matches(crate::input::is_space);
    let parsed = FLOAT
        .find(source)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(f64::NAN);
    if truth_number(parsed) { parsed } else { 0.12 }
}
fn rule(d: &ParsedXml, line: Option<usize>, page: Value, x: Value, y: Value) -> Rule {
    let length = num(d, line, "length", -1.0);
    Rule {
        page,
        x,
        y,
        length: if length > 0.0 { length } else { 14173.0 },
        width_mm: line_width(d, line),
        color: attr(d, line, "color").unwrap_or("#000000").into(),
    }
}

pub fn foot_note_stories(
    d: &ParsedXml,
    root: usize,
    pages: &ParaPages,
    geometry: &Value,
    ctx: &mut Context,
) -> Result<FootNoteStories, String> {
    let properties = find_first(d, root, "footNotePr", 0);
    let spacing = properties.and_then(|i| child(d, i, "noteSpacing"));
    let line = properties.and_then(|i| child(d, i, "noteLine"));
    let above = num(d, spacing, "aboveLine", 850.0);
    let below = num(d, spacing, "belowLine", 567.0);
    let between = num(d, spacing, "betweenNotes", 283.0);
    let mut by_page: Vec<(Value, Vec<(usize, f64)>)> = Vec::new();
    for p in direct(d, root, "p") {
        let para_pages: &[Value] = pages.get(&p).map_or(&[], Vec::as_slice);
        let model = if para_pages.len() > 1 {
            Some(build_para(d, p, ctx)?)
        } else {
            None
        };
        for ctrl in controls(d, p) {
            let Some(note) = child(d, ctrl, "footNote").filter(|&i| !ctx.is_deleted_control(i))
            else {
                continue;
            };
            let Some(sub) = child(d, note, "subList") else {
                continue;
            };
            let mut page = page_at(para_pages, 0, &0.into());
            if let Some(model) = &model {
                let at = model["chars"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|c| c["note"].as_u64() == Some(note as u64));
                if let Some(at) = at {
                    let mut k = 0;
                    for (i, seg) in model["segs"].as_array().unwrap().iter().enumerate() {
                        if number(&seg["textpos"]) <= at as f64 {
                            k = i;
                        }
                    }
                    page = page_at(para_pages, k, &page);
                }
            }
            let mut height = 0.0;
            for q in direct(d, sub, "p") {
                for seg in segs(d, q) {
                    height = max(
                        height,
                        num(d, Some(seg), "vertpos", 0.0) + num(d, Some(seg), "vertsize", 0.0),
                    );
                }
            }
            let at = by_page
                .iter()
                .position(|(key, _)| same_key(key, &page))
                .unwrap_or_else(|| {
                    by_page.push((map_key(&page), Vec::new()));
                    by_page.len() - 1
                });
            by_page[at].1.push((sub, height));
        }
    }
    let mut stories = Vec::new();
    let mut rules = Vec::new();
    for (page, notes) in by_page {
        let area = above
            + below
            + notes.iter().fold(0.0, |sum, (_, h)| sum + h)
            + between * (notes.len() as f64 - 1.0);
        let top = number(&add(&geometry["MT"], &geometry["BODY_H"])) - area;
        if attr(d, line, "type").unwrap_or("SOLID") != "NONE" {
            rules.push(rule(
                d,
                line,
                page.clone(),
                geometry["ML"].clone(),
                (top + above).into(),
            ));
        }
        let mut y = top + above + below;
        for (sub, height) in notes {
            stories.push(story(sub, page.clone(), geometry, y.into(), None));
            y += height + between;
        }
    }
    Ok(FootNoteStories { stories, rules })
}

/// Mutates the same in-memory lineseg nodes used by subsequent paragraph layout.
/// It never writes an original XML file or reparses/reindexes the caller's tree.
pub fn end_note_layout(
    d: &mut ParsedXml,
    root: usize,
    pages: &ParaPages,
    geometry: &Value,
    ctx: &Context,
) -> Result<Option<EndNoteLayout>, String> {
    let mut notes = Vec::new();
    let mut last = None;
    let mut end_columns = None;
    for p in direct(d, root, "p") {
        let segments = segs(d, p);
        let para_pages: &[Value] = pages.get(&p).map_or(&[], Vec::as_slice);
        if let Some(&segment) = segments.last() {
            last = Some((
                segment,
                page_at(
                    para_pages,
                    segments.len() - 1,
                    &page_at(para_pages, 0, &0.into()),
                ),
            ));
        }
        for ctrl in controls(d, p) {
            end_columns = child(d, ctrl, "colPr").or(end_columns);
            if let Some(sub) = child(d, ctrl, "endNote")
                .filter(|&i| !ctx.is_deleted_control(i))
                .and_then(|i| child(d, i, "subList"))
            {
                notes.push(sub);
            }
        }
    }
    let Some((last_segment, mut page)) = last else {
        return Ok(None);
    };
    if notes.is_empty() {
        return Ok(None);
    }
    let properties = find_first(d, root, "endNotePr", 0);
    let spacing = properties.and_then(|i| child(d, i, "noteSpacing"));
    let line = properties.and_then(|i| child(d, i, "noteLine"));
    let above = num(d, spacing, "aboveLine", 0.0);
    let below = num(d, spacing, "belowLine", 567.0);
    let between = num(d, spacing, "betweenNotes", 283.0);
    let columns = end_columns.or_else(|| find_first(d, root, "colPr", 0));
    let n_col = max(1.0, num(d, columns, "colCount", 1.0));
    let gap = if n_col > 1.0 {
        num(d, columns, "sameGap", 0.0)
    } else {
        0.0
    };
    let col_width = (number(&geometry["BODY_W"]) - gap * (n_col - 1.0)) / n_col;
    let mut col = if n_col > 1.0 {
        min(
            n_col - 1.0,
            max(
                0.0,
                round(num(d, Some(last_segment), "horzpos", 0.0) / (col_width + gap)),
            ),
        )
    } else {
        0.0
    };
    let mut y = num(d, Some(last_segment), "vertpos", 0.0)
        + num(d, Some(last_segment), "vertsize", 0.0)
        + above;
    let mut rules = Vec::new();
    if attr(d, line, "type").unwrap_or("SOLID") != "NONE" {
        rules.push(rule(
            d,
            line,
            page.clone(),
            add(&geometry["ML"], &(col * (col_width + gap)).into()),
            add(&geometry["MT"], &y.into()),
        ));
    }
    y += below;
    let mut out = Vec::new();
    for (ni, sub) in notes.into_iter().enumerate() {
        if ni > 0 {
            y += between;
        }
        let mut base = y;
        let mut previous = f64::NEG_INFINITY;
        let mut flow_end = y;
        for p in direct(d, sub, "p") {
            let mut assigned = Vec::new();
            for segment in segs(d, p) {
                let raw = num(d, Some(segment), "vertpos", 0.0);
                if raw < previous {
                    base = flow_end - raw;
                }
                previous = raw;
                let mut v = base + raw;
                let size = num(d, Some(segment), "vertsize", 0.0);
                if v + size > number(&geometry["BODY_H"]) && v > 0.0 {
                    let shift = v;
                    col += 1.0;
                    if col >= n_col {
                        col = 0.0;
                        page = (number(&page) + 1.0).into();
                    }
                    base -= shift;
                    v = 0.0;
                }
                let x = num(d, Some(segment), "horzpos", 0.0) + col * (col_width + gap);
                d.nodes[segment].set_attr("vertpos", string(&v.into()));
                d.nodes[segment].set_attr("horzpos", string(&x.into()));
                assigned.push(page.clone());
                y = v + size;
                flow_end = y + num(d, Some(segment), "spacing", 0.0);
            }
            out.push(EndNotePara { p, pages: assigned });
        }
    }
    Ok(Some(EndNoteLayout {
        paras: out,
        rules,
        page_count: add(&page, &1.into()),
    }))
}

#[cfg(test)]
fn story_report(s: &Story) -> Value {
    let mut out = json!({"sub":s.sub,"page":s.page,"x":s.x,"y":s.y,"width":s.width});
    if let Some(layer) = s.layer {
        out["layer"] = layer.into();
    }
    out
}
#[cfg(test)]
fn rule_report(r: &Rule) -> Value {
    json!({"page":r.page,"x":r.x,"y":r.y,"length":r.length,"widthMm":r.width_mm,"color":r.color})
}
#[cfg(test)]
fn foot_report(f: &FootNoteStories) -> Value {
    json!({"stories":f.stories.iter().map(story_report).collect::<Vec<_>>(),"rules":f.rules.iter().map(rule_report).collect::<Vec<_>>()})
}
#[cfg(test)]
fn end_report(e: Option<&EndNoteLayout>) -> Value {
    match e {
        None => Value::Null,
        Some(e) => {
            json!({"paras":e.paras.iter().map(|p|json!({"p":p.p,"pages":p.pages})).collect::<Vec<_>>(),"rules":e.rules.iter().map(rule_report).collect::<Vec<_>>(),"pageCount":e.page_count})
        }
    }
}
#[cfg(test)]
fn native_report(value: &Value) -> serde_json::Value {
    match value {
        Value::Undefined => serde_json::json!({"$undefined":true}),
        Value::Number(n) if !n.is_finite() => serde_json::json!({"$number":string(value)}),
        Value::Number(n) if *n == 0.0 && n.is_sign_negative() => {
            serde_json::json!({"$number":"-0"})
        }
        Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(native_report).collect())
        }
        Value::Object(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(k, v)| (k.clone(), native_report(v)))
                .collect(),
        ),
        _ => value.to_json(),
    }
}
#[cfg(test)]
fn select(d: &ParsedXml, root: usize, path: &Value) -> Result<usize, String> {
    let mut node = root;
    if let Some(path) = path.as_array() {
        for index in path {
            node = *d.nodes[node]
                .children
                .get(index.as_u64().ok_or("잘못된 노드 경로")? as usize)
                .ok_or("노드 경로가 없습니다.")?;
        }
    }
    Ok(node)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    if operation == "batch" {
        let cases = input["cases"]
            .as_array()
            .ok_or("cases 배열이 필요합니다.")?;
        return Ok(serde_json::Value::Array(
            cases
                .iter()
                .map(|c| {
                    let result = c["operation"]
                        .as_str()
                        .ok_or_else(|| "연산 이름이 필요합니다.".to_string())
                        .and_then(|op| {
                            if op == "batch" {
                                Err("중첩 batch는 지원하지 않습니다.".into())
                            } else {
                                run(op, &c["input"])
                            }
                        });
                    match result {
                        Ok(value) => serde_json::json!({"ok":true,"value":value}),
                        Err(error) => serde_json::json!({"ok":false,"error":error}),
                    }
                })
                .collect(),
        ));
    }
    let input = Value::from_json(input);
    let mut d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let root = select(&d, d.root, &input["path"])?;
    let masters = if let Some(paths) = input["masterPaths"].as_array() {
        paths
            .iter()
            .map(|p| select(&d, d.root, p))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        d.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (n.local == "masterPage").then_some(i))
            .collect()
    };
    let mut pages = ParaPages::new();
    for (index, p) in direct(&d, root, "p").iter().enumerate() {
        if let Some(value) = input["paraPages"][index].as_array() {
            pages.insert(*p, value.clone());
        }
    }
    let mut ctx = Context::default();
    if input["prepare"] == true {
        para_model::prepare_deleted_ranges(&d, root, 0, &mut ctx);
    }
    let geometry = &input["geom"];
    let output = match operation {
        "pageStories" => json!(
            page_stories(
                &d,
                root,
                &masters,
                &pages,
                PageOptions {
                    count: &input["pageCount"],
                    base: &input["pageBase"],
                    geometry
                },
                &ctx
            )?
            .iter()
            .map(story_report)
            .collect::<Vec<_>>()
        ),
        "footNoteStories" => foot_report(&foot_note_stories(&d, root, &pages, geometry, &mut ctx)?),
        "endNoteLayout" => {
            end_report(end_note_layout(&mut d, root, &pages, geometry, &ctx)?.as_ref())
        }
        "endNoteThenBuild" => {
            let result = end_note_layout(&mut d, root, &pages, geometry, &ctx)?;
            let mut models = Vec::new();
            if let Some(result) = &result {
                for para in &result.paras {
                    models.push(json!({"p":para.p,"model":build_para(&d,para.p,&mut ctx)?}));
                }
            }
            json!({"layout":end_report(result.as_ref()),"models":models})
        }
        "sequence" => {
            let page = page_stories(
                &d,
                root,
                &masters,
                &pages,
                PageOptions {
                    count: &input["pageCount"],
                    base: &input["pageBase"],
                    geometry,
                },
                &ctx,
            )?;
            let foot = foot_note_stories(&d, root, &pages, geometry, &mut ctx)?;
            let end = end_note_layout(&mut d, root, &pages, geometry, &ctx)?;
            json!({"pageStories":page.iter().map(story_report).collect::<Vec<_>>(),"footNoteStories":foot_report(&foot),"endNoteLayout":end_report(end.as_ref())})
        }
        _ => return Err("알 수 없는 쪽 부속 본문 연산입니다.".into()),
    };
    let segments = d
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.local == "lineseg")
        .map(|(i, n)| json!({"node":i,"vertpos":n.attr("vertpos"),"horzpos":n.attr("horzpos")}))
        .collect::<Vec<_>>();
    let out = json!({"result":output,"segments":segments});
    Ok(if input["native"] == true {
        native_report(&out)
    } else {
        out.to_json()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_geometry_stays_undefined_or_nan_until_reporting() {
        let d=parse_style_xml("<sec><p><run><ctrl><header><subList/></header><footer><subList/></footer></ctrl></run></p></sec>").unwrap();
        let count = 1.into();
        let base = 0.into();
        let geometry = json!({});
        let stories = page_stories(
            &d,
            d.root,
            &[],
            &ParaPages::new(),
            PageOptions {
                count: &count,
                base: &base,
                geometry: &geometry,
            },
            &Context::default(),
        )
        .unwrap();
        assert!(stories[0].x.is_undefined());
        assert_eq!(number(&stories[0].y), 5668.0);
        assert!(number(&stories[1].y).is_nan());
    }
    #[test]
    fn line_width_keeps_parse_float_and_infinity() {
        for (text, expected) in [
            ("0.25 mm", 0.25),
            ("0", 0.12),
            ("-0", 0.12),
            ("1e+", 1.0),
            ("Infinity", f64::INFINITY),
            ("no", 0.12),
        ] {
            let d = parse_style_xml(&format!("<line width='{text}'/>")).unwrap();
            assert_eq!(line_width(&d, Some(d.root)), expected);
        }
    }
}
