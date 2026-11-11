//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! render/font-env.mjs; see data/kordoc-LICENSE.
#[cfg(test)]
use super::head_styles;
use super::{
    header_xml_compat::parse_header_xml,
    para_model::{Context, child, num},
    reflow_block::{BlockState, reflow_section},
    style_dom::ParsedXml,
    value::{Value, native_json as json},
};
use std::sync::LazyLock;

pub fn has_kopub_subst(head_xml: &str) -> bool {
    // ECMAScript uses ASCII word boundaries and its own whitespace set.
    static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"<hh:font(?-u:\b)[^>]*(?-u:\b)face="KoPub[^"]*"[^>]*(?-u:\b)type="TTF"[^>]*>[\t\n\v\f\r \u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]*<hh:substFont(?-u:\b)"#).unwrap()
    });
    PATTERN.is_match(head_xml)
}

fn descendants_named(d: &ParsedXml, root: usize, name: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = d.nodes[root]
        .children
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    while let Some(i) = stack.pop() {
        if d.nodes[i].name == name {
            out.push(i);
        }
        stack.extend(d.nodes[i].children.iter().rev().copied());
    }
    out
}

/// Exact qualified hp:p descendants, excluding the supplied root itself.
pub fn line_starts(d: &ParsedXml, root: usize) -> Vec<Option<String>> {
    descendants_named(d, root, "hp:p")
        .into_iter()
        .map(|p| {
            let segs = child(d, p, "linesegarray")
                .map(|a| {
                    d.nodes[a]
                        .children
                        .iter()
                        .copied()
                        .filter(|&i| d.nodes[i].local == "lineseg")
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (segs.len() >= 2).then(|| {
                segs.iter()
                    .map(|&s| d.nodes[s].attr("textpos").unwrap_or("0"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
        })
        .collect()
}

/// Parse an independent document for each candidate: probes never mutate output DOMs.
pub fn match_count(xml: &str, saved: &[Option<String>], styles: &Value) -> Result<usize, String> {
    let mut d = parse_header_xml(xml)?;
    let root = d.root;
    let Some(page) = descendants_named(&d, root, "hp:pagePr").first().copied() else {
        return Ok(0);
    };
    let margin = descendants_named(&d, page, "hp:margin").first().copied();
    let mut pw = num(&d, Some(page), "width", 59528.0);
    let mut ph = num(&d, Some(page), "height", 84188.0);
    if d.nodes[page].attr("landscape") == Some("NARROWLY") && pw < ph {
        std::mem::swap(&mut pw, &mut ph);
    }
    let gutter_tb = d.nodes[page].attr("gutterType") == Some("TOP_BOTTOM");
    let gutter = num(&d, margin, "gutter", 0.0);
    let body_w = pw
        - num(&d, margin, "left", 8504.0)
        - num(&d, margin, "right", 8504.0)
        - if gutter_tb { 0.0 } else { gutter };
    let body_h = ph
        - num(&d, margin, "top", 5668.0)
        - num(&d, margin, "header", 0.0)
        - num(&d, margin, "bottom", 4252.0)
        - num(&d, margin, "footer", 0.0)
        - if gutter_tb { gutter } else { 0.0 };
    for p in descendants_named(&d, root, "hp:p") {
        if let Some(a) = child(&d, p, "linesegarray") {
            d.remove_child(p, a)?;
        }
    }
    reflow_section(
        &mut d,
        root,
        styles,
        &json!({"BODY_W":body_w,"BODY_H":body_h}),
        "keep",
        &mut Context::default(),
        &mut BlockState::default(),
    )?;
    let got = line_starts(&d, root);
    Ok(saved
        .iter()
        .enumerate()
        .filter(|(i, s)| s.is_some() && got.get(*i) == Some(*s))
        .count())
}

pub fn kopub_substituted(sec_xmls: &[String], base: &Value, alt: &Value) -> Result<bool, String> {
    let (mut a, mut b, mut seen) = (0usize, 0usize, 0usize);
    for xml in sec_xmls {
        let d = parse_header_xml(xml)?;
        let saved = line_starts(&d, d.root);
        a += match_count(xml, &saved, base)?;
        b += match_count(xml, &saved, alt)?;
        seen += saved.iter().filter(|s| s.is_some()).count();
        if seen >= 60 {
            break;
        }
    }
    Ok(b >= a + 5 && b as f64 > a as f64 * 1.1)
}

#[cfg(test)]
fn styles(input: &Value, key: &str, alt: bool) -> Result<Value, String> {
    if let Some(header) = input["header"].as_str() {
        let mut opts = input.get("styleOpts").cloned().unwrap_or_else(|| json!({}));
        if alt {
            opts["substKoPub"] = true.into();
        }
        head_styles::parse_render_styles(header, Some(&opts))
    } else {
        Ok(input[key].clone())
    }
}

#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let input = Value::from_json(input);
    let xml = || {
        input["xml"]
            .as_str()
            .ok_or_else(|| "xml 문자열이 필요합니다.".to_owned())
    };
    Ok(match operation {
        "hasKoPubSubst" => json!(has_kopub_subst(
            input["headXml"]
                .as_str()
                .ok_or("headXml 문자열이 필요합니다.")?
        )),
        "lineStarts" => {
            let d = parse_header_xml(xml()?)?;
            json!(line_starts(&d, d.root))
        }
        "matchCount" => {
            let saved = input["saved"]
                .as_array()
                .ok_or("saved 배열이 필요합니다.")?
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            json!(match_count(
                xml()?,
                &saved,
                &styles(&input, "styles", false)?
            )?)
        }
        "kopubSubstituted" => {
            let sections = input["secXmls"]
                .as_array()
                .ok_or("secXmls 배열이 필요합니다.")?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or("구역 XML 문자열이 필요합니다.")
                })
                .collect::<Result<Vec<_>, _>>()?;
            json!(kopub_substituted(
                &sections,
                &styles(&input, "base", false)?,
                &styles(&input, "alt", true)?
            )?)
        }
        _ => return Err("알 수 없는 글꼴 환경 명령입니다.".into()),
    }
    .to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regexp_whitespace_and_order() {
        assert!(has_kopub_subst(
            "<hh:font face=\"KoPub X\" type=\"TTF\">\u{feff}<hh:substFont/>"
        ));
        assert!(!has_kopub_subst(
            "<hh:font type=\"TTF\" face=\"KoPub X\"><hh:substFont/>"
        ));
        assert!(!has_kopub_subst(
            "<hh:font face=\"KoPub X\" type=\"TTF\">\u{85}<hh:substFont/>"
        ));
    }
    #[test]
    fn exact_qname_and_root_exclusion() {
        let d=parse_header_xml("<hp:p xmlns:hp=\"u\" xmlns:x=\"u\"><hp:linesegarray><hp:lineseg/><hp:lineseg/></hp:linesegarray><x:p/><hp:p><x:linesegarray><x:lineseg textpos=\"\"/><x:lineseg/></x:linesegarray></hp:p></hp:p>").unwrap();
        assert_eq!(line_starts(&d, d.root), vec![Some(",0".into())]);
    }
}
