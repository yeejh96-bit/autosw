//! kordoc svg-render.mjs final SVG/page assembly, MIT, Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE. This stage does not load, reflow, validate, or write.
#[cfg(test)]
use super::section_render::RenderedSection;
use super::{
    layout::maximum,
    layout_text::{LayoutText, TextRef},
    numbering::add,
    section_render::SectionRenderResult,
    svg_para::{pt, round},
    value::{Value, native_json as json, number, string},
};
use std::collections::HashSet;

pub const SVG_FONT_FAMILY: &str =
    "'HCR Batang','함초롬바탕','Hancom Batang',AppleMyungjo,'Noto Serif CJK KR',serif";

#[derive(Clone, Debug)]
pub enum PageSelection {
    All,
    /// Set.has(pageNo): numeric keys match numbers only. Keep values native so
    /// a string key such as "1" does not accidentally select page one.
    Pages(Vec<Value>),
}
impl PageSelection {
    fn includes(&self, page: usize) -> bool {
        match self {
            Self::All => true,
            Self::Pages(values) => values
                .iter()
                .any(|v| matches!(v,Value::Number(n) if *n==page as f64)),
        }
    }
}
#[derive(Clone, Debug)]
pub struct AssembledSvg {
    pub svg: LayoutText,
    pub width: Value,
    pub height: Value,
    pub page_count: usize,
    pub warnings: Vec<String>,
    pub stats: Value,
    pub regions: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct AssembledPages {
    pub scene: Value,
    /// Insertion order is the original Map order, independent of selection order.
    pub page_svgs: Vec<(usize, LayoutText)>,
}
fn append(out: &mut LayoutText, s: &str) {
    out.append(&TextRef::Utf8(s));
}
fn join(parts: &[LayoutText], separator: &str) -> LayoutText {
    let mut out = LayoutText::default();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            append(&mut out, separator);
        }
        out.append(&part.as_text_ref());
    }
    out
}
fn join_defs<'a>(defs: impl IntoIterator<Item = &'a Value>) -> LayoutText {
    let mut out = LayoutText::default();
    for def in defs {
        if !matches!(def, Value::Null | Value::Undefined) {
            append(&mut out, &string(def));
        }
    }
    out
}

/// The body after renderHwpxToSvg's internal rendering await. No SVG rejection
/// belongs here: the frozen assembly function also returns malformed content.
pub fn assemble_svg(r: &SectionRenderResult) -> AssembledSvg {
    let mut clip_defs = Vec::new();
    let mut groups = Vec::new();
    let mut y = Value::Number(0.0);
    let mut max_pw = 0.0;
    let mut page_no = 0;
    for rs in &r.sections {
        max_pw = maximum(max_pw, number(&rs.pw));
        let pw = pt(number(&rs.pw));
        let ph = pt(number(&rs.page_h));
        clip_defs.push(LayoutText::from(format!(
            "<clipPath id=\"{}\"><rect x=\"0\" y=\"0\" width=\"{pw}\" height=\"{ph}\"/></clipPath>",
            rs.clip_id
        )));
        for buf in &rs.pages {
            page_no += 1;
            let mut group = LayoutText::from(format!(
                "<g data-page=\"{page_no}\" transform=\"translate(0 {})\"><rect width=\"{pw}\" height=\"{ph}\" fill=\"white\" stroke=\"#c9c7c4\" stroke-width=\"0.75\"/><g clip-path=\"url(#{})\">\n",
                pt(number(&y)),
                rs.clip_id
            ));
            group.append(&join(buf, "\n").as_text_ref());
            append(&mut group, "\n</g></g>");
            groups.push(group);
            y = add(&y, &add(&rs.page_h, &2400.into()));
        }
    }
    let total_h = maximum(0.0, number(&y) - 2400.0);
    let pw = pt(max_pw);
    let ph = pt(total_h);
    let mut svg = LayoutText::from(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {pw} {ph}\" width=\"{pw}pt\" height=\"{ph}pt\" font-family=\"{SVG_FONT_FAMILY}\" xml:space=\"preserve\">\n<defs>"
    ));
    svg.append(&join(&clip_defs, "").as_text_ref());
    svg.append(&join_defs(r.defs.iter()).as_text_ref());
    append(&mut svg, "</defs>\n");
    svg.append(&join(&groups, "\n").as_text_ref());
    append(&mut svg, "\n</svg>");
    AssembledSvg {
        svg,
        width: (round(max_pw) / 100.0).into(),
        height: (round(total_h) / 100.0).into(),
        page_count: page_no,
        warnings: r.warnings.clone(),
        stats: r.stats.clone(),
        regions: r.regions.clone(),
    }
}

/// Match the two ASCII-only JavaScript regexes without decoding or replacing
/// surrounding UTF-16. A match can occur anywhere (including xlink:href).
fn binary_ids(units: &[u16], prefix: &str) -> Vec<Vec<u16>> {
    let prefix = prefix.encode_utf16().collect::<Vec<_>>();
    let mut ids = Vec::new();
    let mut at = 0;
    while at + prefix.len() <= units.len() {
        if units[at..].starts_with(&prefix) {
            let start = at + prefix.len();
            let mut end = start;
            while units.get(end).is_some_and(|u| (48..=57).contains(u)) {
                end += 1;
            }
            if end > start && units.get(end) == Some(&34) {
                let mut id = "bin".encode_utf16().collect::<Vec<_>>();
                id.extend_from_slice(&units[start..end]);
                ids.push(id);
                at = end + 1;
                continue;
            }
        }
        at += 1;
    }
    ids
}
pub fn assemble_page_svgs(
    r: &SectionRenderResult,
    format: Value,
    selection: &PageSelection,
) -> AssembledPages {
    let mut pages = Vec::new();
    let mut page_svgs = Vec::new();
    let mut page_no = 0;
    for rs in &r.sections {
        for buf in &rs.pages {
            page_no += 1;
            pages.push(json!({"page":page_no,"width":round(number(&rs.pw))/100.0,"height":round(number(&rs.page_h))/100.0}));
            if !selection.includes(page_no) {
                continue;
            }
            let body = join(buf, "\n");
            let used = binary_ids(&body.0, "href=\"#bin")
                .into_iter()
                .collect::<HashSet<_>>();
            let defs = r.defs.iter().filter(|def| {
                let text = string(def).encode_utf16().collect::<Vec<_>>();
                binary_ids(&text, "<symbol id=\"bin")
                    .first()
                    .is_none_or(|id| used.contains(id))
            });
            let pw = pt(number(&rs.pw));
            let ph = pt(number(&rs.page_h));
            let clip_id = format!("pgclip{page_no}");
            let mut svg = LayoutText::from(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {pw} {ph}\" width=\"{pw}pt\" height=\"{ph}pt\" font-family=\"{SVG_FONT_FAMILY}\" xml:space=\"preserve\" data-page=\"{page_no}\">\n<defs><clipPath id=\"{clip_id}\"><rect x=\"0\" y=\"0\" width=\"{pw}\" height=\"{ph}\"/></clipPath>"
            ));
            svg.append(&join_defs(defs).as_text_ref());
            append(
                &mut svg,
                &format!(
                    "</defs>\n<rect width=\"{pw}\" height=\"{ph}\" fill=\"white\"/>\n<g clip-path=\"url(#{clip_id})\">\n"
                ),
            );
            svg.append(&body.as_text_ref());
            append(&mut svg, "\n</g>\n</svg>");
            page_svgs.push((page_no, svg));
        }
    }
    let scene = json!({"format":format,"pages":pages,"regions":r.regions,"warnings":r.warnings,"stats":{"texts":r.stats["texts"],"tables":r.stats["tables"],"images":r.stats["images"],"shapes":r.stats["shapes"]}});
    AssembledPages { scene, page_svgs }
}
/// The selector is evaluated exactly once, before producing any page metadata.
pub fn assemble_page_svgs_with<F>(
    r: &SectionRenderResult,
    format: Value,
    select: F,
) -> Result<AssembledPages, String>
where
    F: FnOnce(usize) -> Result<PageSelection, String>,
{
    let page_count = r.sections.iter().map(|s| s.pages.len()).sum();
    let chosen = select(page_count)?;
    Ok(assemble_page_svgs(r, format, &chosen))
}
/// Explicit storage boundary. Assembly above remains lossless and unchecked;
/// the application calls this only where its original SVG validator belonged.
pub fn validated_svg(svg: &LayoutText) -> Result<String, String> {
    let text = svg
        .to_string_strict()
        .map_err(|_| "XML에 쓸 수 없는 문자입니다.".to_owned())?;
    super::svg_safety::validate_rendered_svg(&text)?;
    Ok(text)
}

#[cfg(test)]
fn decode_text(v: &Value) -> Result<LayoutText, String> {
    if let Some(units) = v.get("units") {
        return Ok(LayoutText(
            units
                .as_array()
                .ok_or("UTF16 배열 필요")?
                .iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| u16::try_from(n).ok())
                        .ok_or("UTF16 단위 오류")
                })
                .collect::<Result<_, _>>()?,
        ));
    }
    Ok(if matches!(v, Value::Null | Value::Undefined) {
        LayoutText::default()
    } else {
        string(v).into()
    })
}
#[cfg(test)]
fn read_result(i: &Value) -> Result<SectionRenderResult, String> {
    let mut sections = Vec::new();
    for section in i["sections"].as_array().ok_or("sections 배열 필요")? {
        let pages = section["pages"]
            .as_array()
            .ok_or("pages 배열 필요")?
            .iter()
            .map(|page| {
                page.as_array()
                    .ok_or("page 조각 배열 필요")?
                    .iter()
                    .map(decode_text)
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        sections.push(RenderedSection {
            pages,
            pw: section["PW"].clone(),
            page_h: section["pageH"].clone(),
            clip_id: string(&section["clipId"]),
        });
    }
    Ok(SectionRenderResult {
        sections,
        defs: i["defs"].as_array().ok_or("defs 배열 필요")?.clone(),
        warnings: i["warnings"]
            .as_array()
            .ok_or("warnings 배열 필요")?
            .iter()
            .map(string)
            .collect(),
        stats: i["stats"].clone(),
        regions: i["regions"].as_array().ok_or("regions 배열 필요")?.clone(),
    })
}
#[cfg(test)]
fn selection(v: &Value) -> Result<PageSelection, String> {
    match v {
        Value::Null | Value::Undefined | Value::Bool(false) => Ok(PageSelection::All),
        Value::Array(a) => Ok(PageSelection::Pages(a.clone())),
        _ => Err("쪽 선택은 배열 또는 null이어야 합니다.".into()),
    }
}
#[cfg(test)]
fn validation(svg: &LayoutText) -> Value {
    let result = validated_svg(svg);
    json!({"accepted":result.is_ok(),"error":result.err()})
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut i = Value::from_json(input);
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
    let r = read_result(&i["result"])?;
    let out = match op {
        "assembleSvg" => {
            let a = assemble_svg(&r);
            json!({"svgUnits":a.svg.0,"width":a.width,"height":a.height,"pageCount":a.page_count,"warnings":a.warnings,"stats":a.stats,"regions":a.regions,"validation":validation(&a.svg)})
        }
        "assemblePageSvgs" | "renderHwpxPages" => {
            let mut calls = Vec::new();
            let format = if op == "renderHwpxPages" {
                "hwpx".into()
            } else {
                i["format"].clone()
            };
            let a = if let Some(callback) = i.get("callback") {
                assemble_page_svgs_with(&r, format, |count| {
                    calls.push(count);
                    if let Some(err) = callback["error"].as_str() {
                        return Err(err.into());
                    }
                    match callback["mode"].as_str() {
                        Some("odd") => Ok(PageSelection::Pages(
                            (1..=count)
                                .filter(|n| n % 2 == 1)
                                .map(Value::from)
                                .collect(),
                        )),
                        Some("last") => Ok(PageSelection::Pages(vec![count.into()])),
                        _ => selection(&callback["select"]),
                    }
                })
            } else {
                selection(&i["select"]).map(|s| assemble_page_svgs(&r, format, &s))
            };
            match a {
                Ok(a) => {
                    json!({"scene":a.scene,"pageSvgs":a.page_svgs.iter().map(|(n,svg)|json!([n,svg.0])).collect::<Vec<_>>(),"validation":a.page_svgs.iter().map(|(n,svg)|json!([n,validation(svg)])).collect::<Vec<_>>(),"selectorCalls":calls,"error":null})
                }
                Err(e) => json!({"selectorCalls":calls,"error":e}),
            }
        }
        _ => return Err("알 수 없는 SVG 조립 연산".into()),
    };
    Ok(super::svg_para::report(&out))
}
