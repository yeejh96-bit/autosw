//! Derived from kordoc (MIT), Copyright 2026 chrisryugj; data/kordoc-LICENSE.
//! Dispatch nested content to the caller's native paragraph flow function.
#[cfg(test)]
use super::value::native_json as json;
use super::{
    layout::maximum,
    para_model::{cell_margin_of, child, num},
    style_dom::ParsedXml,
    value::{Value, string},
};
use std::{cmp::Ordering, collections::HashMap};
pub type FlowCallback<'a> =
    dyn FnMut(&mut ParsedXml, usize, Value, &mut Value) -> Result<(), String> + 'a;
fn truth(v: &Value) -> bool {
    super::numbering::truth(v)
}
fn key(n: f64) -> u64 {
    if n.is_nan() {
        f64::NAN.to_bits()
    } else if n == 0.0 {
        0
    } else {
        n.to_bits()
    }
}
fn array_key(n: f64) -> String {
    string(&Value::from(n))
}
pub fn reflow_sub_lists_in(
    d: &mut ParsedXml,
    p: usize,
    counter: &mut Value,
    note_w: Option<&Value>,
    flow: &mut FlowCallback<'_>,
) -> Result<(), String> {
    let note_w = note_w.cloned().unwrap_or_else(|| counter["bodyW"].clone());
    fn visit(
        d: &mut ParsedXml,
        el: usize,
        counter: &mut Value,
        note_w: &Value,
        flow: &mut FlowCallback<'_>,
    ) -> Result<(), String> {
        for ch in d.nodes[el].children.clone() {
            match d.nodes[ch].local.as_str() {
                "footNote" | "endNote" | "header" | "footer" => {
                    let sub = child(d, ch, "subList");
                    let w = match d.nodes[ch].local.as_str() {
                        "endNote" => {
                            if matches!(counter["endNoteW"], Value::Null | Value::Undefined) {
                                note_w.clone()
                            } else {
                                counter["endNoteW"].clone()
                            }
                        }
                        "footNote" => note_w.clone(),
                        _ => counter["bodyW"].clone(),
                    };
                    if let Some(sub) = sub.filter(|_| truth(&w)) {
                        flow(d, sub, w, counter)?
                    }
                }
                "drawText" | "caption" => {
                    let sub = child(d, ch, "subList");
                    let tm = child(d, ch, "textMargin");
                    let last = num(d, Some(ch), "lastWidth", 0.0);
                    let text_w = if last != 0.0 {
                        last
                    } else if d.nodes[ch].local == "caption" {
                        num(d, Some(ch), "width", 0.0)
                    } else {
                        num(d, child(d, el, "sz"), "width", 0.0)
                    };
                    let w = text_w - num(d, tm, "left", 0.0) - num(d, tm, "right", 0.0);
                    if let Some(sub) = sub.filter(|_| w > 0.0) {
                        flow(d, sub, w.into(), counter)?
                    }
                }
                "tr" | "subList" | "linesegarray" => {}
                _ => visit(d, ch, counter, note_w, flow)?,
            }
        }
        Ok(())
    }
    for run in d.nodes[p].children.clone() {
        if d.nodes[run].local == "run" {
            visit(d, run, counter, &note_w, flow)?
        }
    }
    Ok(())
}
pub fn reflow_tables_in(
    d: &mut ParsedXml,
    p: usize,
    counter: &mut Value,
    flow: &mut FlowCallback<'_>,
) -> Result<(), String> {
    for run in d.nodes[p].children.clone() {
        if d.nodes[run].local != "run" {
            continue;
        }
        for obj in d.nodes[run].children.clone() {
            if d.nodes[obj].local != "tbl" {
                continue;
            }
            let im = child(d, obj, "inMargin");
            let mut col_max: HashMap<u64, f64> = HashMap::new();
            for &tr in &d.nodes[obj].children {
                for &tc in &d.nodes[tr].children {
                    if d.nodes[tc].local != "tc"
                        || num(d, child(d, tc, "cellSpan"), "colSpan", 1.0) != 1.0
                    {
                        continue;
                    }
                    let c = num(d, child(d, tc, "cellAddr"), "colAddr", 0.0);
                    let w = num(d, child(d, tc, "cellSz"), "width", 0.0);
                    col_max.insert(key(c), maximum(*col_max.get(&key(c)).unwrap_or(&0.0), w));
                }
            }
            let mut cl = Vec::new();
            for &tr in &d.nodes[obj].children {
                for &tc in &d.nodes[tr].children {
                    if d.nodes[tc].local == "tc" {
                        cl.push((
                            num(d, child(d, tc, "cellAddr"), "colAddr", 0.0),
                            num(d, child(d, tc, "cellSpan"), "colSpan", 1.0),
                            num(d, child(d, tc, "cellSz"), "width", 0.0),
                        ));
                    }
                }
            }
            cl.sort_by(|p, q| (p.0 - q.0).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
            let mut xs = HashMap::from([("0".to_owned(), 0.0)]);
            for (c, sp, w) in cl {
                let end = array_key(c + sp);
                let v = maximum(
                    *xs.get(&end).unwrap_or(&0.0),
                    *xs.get(&array_key(c)).unwrap_or(&0.0) + w,
                );
                xs.insert(end, v);
            }
            for tr in d.nodes[obj].children.clone() {
                if d.nodes[tr].local != "tr" {
                    continue;
                }
                for tc in d.nodes[tr].children.clone() {
                    if d.nodes[tc].local != "tc" {
                        continue;
                    }
                    let sz = child(d, tc, "cellSz");
                    let cm = if d.nodes[tc].attr("hasMargin") == Some("0") {
                        None
                    } else {
                        child(d, tc, "cellMargin")
                    };
                    let span = num(d, child(d, tc, "cellSpan"), "colSpan", 1.0);
                    let ca = num(d, child(d, tc, "cellAddr"), "colAddr", 0.0);
                    let mut w = if span == 1.0 {
                        col_max
                            .get(&key(ca))
                            .copied()
                            .unwrap_or_else(|| num(d, sz, "width", 0.0))
                    } else if span == num(d, Some(obj), "colCnt", 0.0) {
                        let v = num(d, child(d, obj, "sz"), "width", 0.0);
                        if v != 0.0 {
                            v
                        } else {
                            num(d, sz, "width", 0.0)
                        }
                    } else {
                        num(d, sz, "width", 0.0)
                    };
                    let gw = match (xs.get(&array_key(ca + span)), xs.get(&array_key(ca))) {
                        (Some(b), Some(a)) => b - a,
                        _ => 0.0,
                    };
                    if gw > w {
                        w = gw
                    }
                    let ml = cell_margin_of(d, cm, "left", num(d, im, "left", 141.0));
                    let mr = cell_margin_of(d, cm, "right", num(d, im, "right", 141.0));
                    let area_w = maximum(500.0, w - ml - mr);
                    if let Some(sub) = child(d, tc, "subList") {
                        flow(d, sub, area_w.into(), counter)?
                    }
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let xml = input["xml"].as_str().ok_or("xml이 필요합니다.")?;
    let mut d = super::style_dom::parse_style_xml(xml)?;
    let mut p = d.root;
    if let Some(path) = input["path"].as_array() {
        for n in path {
            p = *d.nodes[p]
                .children
                .get(n.as_u64().ok_or("path 번호 오류")? as usize)
                .ok_or("path 범위 오류")?
        }
    }
    let input = Value::from_json(input);
    let mut counter = input.get("counter").cloned().unwrap_or_else(|| json!({}));
    let mut calls = Vec::new();
    let mut flow = |_: &mut ParsedXml,
                    sub: usize,
                    width: Value,
                    counter: &mut Value|
     -> Result<(), String> {
        calls.push(json!({"sub":sub,"width":width,"mode":input["mode"],"start":0,"counter":counter.clone()}));
        if input["throwAt"].as_u64() == Some((calls.len() - 1) as u64) {
            return Err("합성 flow 실패".into());
        }
        if let Some(patch) = input["updates"]
            .get(calls.len() - 1)
            .and_then(Value::as_object)
        {
            for (k, v) in patch.iter() {
                counter[k.as_str()] = v.clone();
            }
        }
        Ok(())
    };
    let result = match op {
        "reflowSubListsIn" => {
            reflow_sub_lists_in(&mut d, p, &mut counter, input.get("noteW"), &mut flow)
        }
        "reflowTablesIn" => reflow_tables_in(&mut d, p, &mut counter, &mut flow),
        _ => return Err("알 수 없는 하위 배치 연산입니다.".into()),
    };
    Ok(json!({"calls":calls,"counter":counter,"error":result.err()}).to_json())
}
