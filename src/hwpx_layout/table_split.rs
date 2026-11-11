//! Derived from kordoc (MIT), Copyright 2026 chrisryugj; data/kordoc-LICENSE.
//! Stateful floating-table continuation. Mutations affect only the in-memory DOM.
#[cfg(test)]
use super::style_dom::parse_style_xml;
use super::{
    layout::{maximum as max, minimum as min},
    para_model::{self as pm, Context, child, num},
    style_dom::ParsedXml,
    table_flow::sub_float_bottoms,
    value::{Map, Value, native_json as json, number, string},
};
use std::cmp::Ordering;

pub const MAX_SPLIT_STEPS: usize = 100_000;
pub const MAX_SPLIT_DEPTH: usize = 64;
const LIMIT: &str = "표 이어 나눔 처리 한도(100000회 또는 중첩 64단계)를 넘었습니다.";
#[derive(Default, Debug)]
pub struct SplitState {
    pub whole: bool,
    pub notes_n: f64,
    pub notes_s: f64,
    pub moved: bool,
}
impl SplitState {
    pub fn to_value(&self) -> Value {
        json!({"whole":self.whole,"notes":{"n":self.notes_n,"s":self.notes_s},"moved":self.moved})
    }
}
pub type AreaCallback<'a> = dyn FnMut(Value, Value) -> Result<Value, String> + 'a;
pub struct Notes<'a> {
    pub n0: Value,
    pub s0: Value,
    pub area: &'a mut AreaCallback<'a>,
}
type Record = Map<usize, Vec<Value>>;
type Carries = Map<usize, Map<u64, f64>>;
#[derive(Default)]
struct Budget(usize);
impl Budget {
    fn tick(&mut self) -> Result<(), String> {
        self.0 += 1;
        if self.0 > MAX_SPLIT_STEPS {
            Err(LIMIT.into())
        } else {
            Ok(())
        }
    }
}
fn key(v: f64) -> u64 {
    if v == 0.0 {
        0
    } else if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        v.to_bits()
    }
}
fn n(v: &Value, k: &str) -> f64 {
    number(&v[k])
}
fn id(v: &Value) -> Option<usize> {
    v.as_u64().map(|v| v as usize)
}
fn truth(v: &Value) -> bool {
    super::numbering::truth(v)
}
fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v[k].as_array().map_or(&[], Vec::as_slice)
}
fn add(a: Value, b: Value) -> Value {
    super::numbering::add(&a, &b)
}
fn segs(d: &ParsedXml, sub: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for &p in &d.nodes[sub].children {
        if d.nodes[p].local != "p" {
            continue;
        }
        if let Some(lsa) = child(d, p, "linesegarray") {
            out.extend(
                d.nodes[lsa]
                    .children
                    .iter()
                    .copied()
                    .filter(|&s| d.nodes[s].local == "lineseg"),
            );
        }
    }
    out
}
fn descendants(d: &ParsedXml, node: usize, name: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = d.nodes[node]
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
fn note_bottom(d: &ParsedXml, note: usize) -> f64 {
    child(d, note, "subList").map_or(0.0, |sub| {
        segs(d, sub).iter().fold(0.0, |b, &s| {
            max(
                b,
                num(d, Some(s), "vertpos", 0.0) + num(d, Some(s), "vertsize", 0.0),
            )
        })
    })
}
fn space_after_at(d: &ParsedXml, sg: usize, styles: Option<&Value>) -> Value {
    let Some(styles) = styles else {
        return 0.into();
    };
    let Some(lsa) = d.nodes[sg].parent else {
        return 0.into();
    };
    let Some(p) = d.nodes[lsa].parent else {
        return 0.into();
    };
    if d.nodes[lsa]
        .children
        .iter()
        .rev()
        .find(|&&s| d.nodes[s].local == "lineseg")
        .copied()
        != Some(sg)
    {
        return 0.into();
    }
    let key = d.nodes[p].attr("paraPrIDRef").unwrap_or("");
    let geom = list(styles, "paraGeom")
        .iter()
        .rev()
        .find(|entry| entry[0].as_str() == Some(key));
    geom.map(|g| g[1]["spaceAfter"].clone())
        .filter(|v| !matches!(v, Value::Null | Value::Undefined))
        .unwrap_or_else(|| 0.into())
}
fn end_with_space(d: &ParsedXml, sg: usize, v: f64, styles: Option<&Value>) -> Value {
    add(
        (v + num(d, Some(sg), "vertsize", 0.0) + num(d, Some(sg), "spacing", 0.0)).into(),
        space_after_at(d, sg, styles),
    )
}
fn floor_at(
    floor: f64,
    body_h: f64,
    moved: bool,
    nn: f64,
    ns: f64,
    notes: &mut Option<&mut Notes<'_>>,
) -> Result<f64, String> {
    if nn > 0.0
        && let Some(notes) = notes.as_deref_mut()
    {
        let n = add(if moved { 0.into() } else { notes.n0.clone() }, nn.into());
        let s = add(if moved { 0.into() } else { notes.s0.clone() }, ns.into());
        return Ok(min(floor, body_h - number(&(notes.area)(n, s)?)));
    }
    Ok(floor)
}

/// Retains prior state on early text rejection, as the original module does.
#[allow(clippy::too_many_arguments)]
pub fn split_float_table_remainder(
    d: &mut ParsedXml,
    p: usize,
    start_v: f64,
    lim: f64,
    body_h: f64,
    styles: Option<&Value>,
    with_text: bool,
    mut notes: Option<&mut Notes<'_>>,
    ctx: &mut Context,
    state: &mut SplitState,
) -> Result<Option<f64>, String> {
    let m = pm::build_para(d, p, ctx)?;
    if !with_text && list(&m, "chars").iter().any(|c| c["ch"] != "") {
        return Ok(None);
    }
    let mut best = None;
    let mut whole = true;
    let mut budget = Budget::default();
    for o in list(&m, "objs") {
        budget.tick()?;
        let el = id(&o["el"]).ok_or("표 노드가 없습니다.")?;
        if truth(&o["inline"])
            || o["tag"] != "tbl"
            || !matches!(
                d.nodes[el].attr("textWrap"),
                Some("TOP_AND_BOTTOM" | "SQUARE")
            )
        {
            continue;
        }
        let pos = child(d, el, "pos");
        if pm::attr(d, pos, "vertRelTo").unwrap_or("PARA") != "PARA" {
            continue;
        }
        if let Some(b) = split_table_rows(
            d,
            el,
            start_v + max(0.0, num(d, pos, "vertOffset", 0.0)),
            lim,
            body_h,
            styles,
            notes.as_deref_mut(),
            ctx,
            state,
            &mut budget,
            0,
        )? {
            best = Some(max(best.unwrap_or(0.0), b));
            whole = whole && state.whole;
        }
    }
    state.whole = best.is_some() && whole;
    Ok(best)
}
#[allow(clippy::too_many_arguments)]
fn split_table_rows(
    d: &mut ParsedXml,
    tbl: usize,
    top: f64,
    lim: f64,
    body_h: f64,
    styles: Option<&Value>,
    mut notes: Option<&mut Notes<'_>>,
    ctx: &mut Context,
    state: &mut SplitState,
    budget: &mut Budget,
    depth: usize,
) -> Result<Option<f64>, String> {
    budget.tick()?;
    if depth > MAX_SPLIT_DEPTH {
        return Err(LIMIT.into());
    }
    state.whole = false;
    state.notes_n = 0.0;
    state.notes_s = 0.0;
    let mode = d.nodes[tbl].attr("pageBreak").unwrap_or("").to_owned();
    if mode != "TABLE" && mode != "CELL" {
        return Ok(None);
    }
    let om = child(d, tbl, "outMargin");
    let out_b = num(d, om, "bottom", 0.0);
    let rows = pm::without_extent_memo(ctx, |ctx| pm::measure_table_row_heights(d, tbl, ctx))?;
    if rows.is_empty() {
        return Ok(None);
    }
    let out_t = num(d, om, "top", 0.0);
    let cap = pm::caption_of(d, tbl).unwrap_or(Value::Undefined);
    let cap_top = if cap["side"] == "TOP" {
        n(&cap, "h") + n(&cap, "gap")
    } else {
        0.0
    };
    let cap_bottom = if cap["side"] == "BOTTOM" {
        n(&cap, "h") + n(&cap, "gap")
    } else {
        0.0
    };
    let all_cells = pm::collect_cells(d, tbl);
    let mut head_h = 0.0;
    if d.nodes[tbl].attr("repeatHeader") == Some("1") {
        let mut heads = std::collections::HashSet::new();
        for c in &all_cells {
            if d.nodes[id(&c["el"]).unwrap()].attr("header") == Some("1") {
                let mut r = n(c, "ra");
                while r < n(c, "ra") + n(c, "rs") {
                    budget.tick()?;
                    heads.insert(key(r));
                    r += 1.0;
                }
            }
        }
        for (r, &h) in rows.iter().enumerate() {
            if !heads.contains(&key(r as f64)) {
                break;
            }
            head_h += h;
        }
    }
    let mut y = top + out_t + cap_top;
    let mut floor = lim;
    let mut moved = false;
    let mut row_notes = vec![Vec::<Vec<f64>>::new(); rows.len()];
    let (mut all_n, mut all_s) = (0.0, 0.0);
    if notes.is_some() {
        for c in &all_cells {
            for note in descendants(d, id(&c["el"]).unwrap(), "hp:footNote") {
                let at = min(n(c, "ra"), (rows.len() - 1) as f64);
                if !at.is_finite() || at < 0.0 || at.fract() != 0.0 {
                    return Err("각주 행 위치가 없습니다.".into());
                }
                row_notes[at as usize].push(
                    descendants(d, note, "hp:lineseg")
                        .iter()
                        .map(|&s| {
                            num(d, Some(s), "vertpos", 0.0) + num(d, Some(s), "vertsize", 0.0)
                        })
                        .collect(),
                );
                all_n += 1.0;
                all_s += note_bottom(d, note);
            }
        }
    }
    let (mut nn, mut ns, mut placed) = (0.0, 0.0, 0.0);
    let sp = num(d, Some(tbl), "cellSpacing", 0.0);
    let mut record = Record::new();
    let mut carries = Carries::new();
    for (r, &row) in rows.iter().enumerate() {
        budget.tick()?;
        let mut h = row;
        for lines in &row_notes[r] {
            if nn == 0.0 && !lines.is_empty() {
                nn = 1.0;
                ns = lines[1.min(lines.len() - 1)];
            }
        }
        if mode == "CELL" {
            let mut from = Map::new();
            let mut bases = Map::new();
            // Evaluate the callback even if the available body height is zero.
            while y + h + out_b > floor_at(floor, body_h, moved, nn, ns, &mut notes)?
                && body_h - out_t - out_b > 0.0
            {
                budget.tick()?;
                let fixed = d.nodes[tbl].attr("noAdjust") == Some("1");
                let avail = floor_at(floor, body_h, moved, nn, ns, &mut notes)?
                    - out_b
                    - y
                    - if moved { 100.0 } else { 20.0 }
                    - 2.0 * sp;
                let piece = floor_at(floor, body_h, moved, nn, ns, &mut notes)?
                    - out_b
                    - y
                    - if fixed { 2.0 } else { 101.0 }
                    - 2.0 * sp;
                let rem = cell_row_remainder(
                    d,
                    &all_cells,
                    r as f64,
                    avail,
                    &mut from,
                    &mut record,
                    &mut bases,
                    &mut carries,
                    styles,
                    body_h - out_t - out_b,
                    piece,
                    fixed,
                    ctx,
                    state,
                    budget,
                    depth,
                )?;
                if rem == Some(0.0) && r == rows.len() - 1 {
                    break;
                }
                let rest = rem.map(|v| v + sp + if r == rows.len() - 1 { sp } else { 0.0 });
                let grow = match rest {
                    Some(v)
                        if v.partial_cmp(&h) != Some(Ordering::Greater)
                            && v.partial_cmp(&h) != Some(Ordering::Equal) =>
                    {
                        h = v;
                        false
                    }
                    _ => true,
                };
                if grow {
                    if moved && y == out_t + head_h {
                        break;
                    }
                    if !moved && r == 0 {
                        state.whole = true;
                    }
                }
                y = out_t + head_h;
                floor = body_h;
                moved = true;
                placed += ns;
                nn = 0.0;
                ns = 0.0;
            }
            y += h;
            continue;
        }
        let mut need = h;
        if !all_cells
            .iter()
            .any(|c| n(c, "ra") < r as f64 && n(c, "ra") + n(c, "rs") > r as f64)
        {
            let mut end = r as f64 + 1.0;
            loop {
                budget.tick()?;
                let mut grown = false;
                for c in &all_cells {
                    if n(c, "ra") >= r as f64 && n(c, "ra") < end && n(c, "ra") + n(c, "rs") > end {
                        end = n(c, "ra") + n(c, "rs");
                        grown = true;
                    }
                }
                if !grown {
                    break;
                }
            }
            let group_h = rows[r..(end as usize).min(rows.len())]
                .iter()
                .fold(0.0, |a, b| a + b);
            if end > r as f64 + 1.0 && group_h <= body_h - out_t - out_b - head_h {
                need = group_h;
            }
        }
        if y + need + out_b > floor_at(floor, body_h, moved, nn, ns, &mut notes)?
            && !(moved && y == out_t + head_h)
        {
            if !moved && r == 0 {
                state.whole = true;
            }
            y = out_t + head_h;
            floor = body_h;
            moved = true;
            placed += ns;
            nn = 0.0;
            ns = 0.0;
        }
        y += h;
    }
    if mode == "CELL" {
        rebase_split_cells(d, &record, &carries, styles, ctx, budget)?;
    }
    if moved {
        state.notes_n = all_n;
        state.notes_s = max(0.0, all_s - placed);
    }
    state.moved = moved;
    Ok(Some(y + cap_bottom + out_b))
}

#[allow(clippy::too_many_arguments)]
fn cell_row_remainder(
    d: &mut ParsedXml,
    cells: &[Value],
    row: f64,
    avail: f64,
    from: &mut Map<usize, Value>,
    record: &mut Record,
    bases: &mut Map<usize, Value>,
    carries: &mut Carries,
    styles: Option<&Value>,
    cont_h: f64,
    piece: f64,
    fixed: bool,
    ctx: &mut Context,
    state: &mut SplitState,
    budget: &mut Budget,
    depth: usize,
) -> Result<Option<f64>, String> {
    let (mut rem, mut any, mut placed) = (0.0, false, false);
    let mut cuts = Vec::new();
    for c in cells {
        budget.tick()?;
        if c["ra"].as_f64() != Some(row) || c["rs"].as_f64() != Some(1.0) {
            continue;
        }
        let Some(sub) = id(&c["sub"]) else {
            continue;
        };
        let el = id(&c["el"]).unwrap();
        let off = from.get(&el).cloned().unwrap_or_else(|| 0.into());
        let base = bases.get(&el).cloned().unwrap_or_else(|| 0.into());
        let lim_abs = number(&add(base.clone(), avail.into())) - n(c, "marginB") - n(c, "marginT");
        let (mut carry, mut carry_from) = (0.0, f64::INFINITY);
        let mut split_at = None;
        for _ in 0..16 {
            budget.tick()?;
            let f = sub_float_bottoms(d, sub, ctx)?.into_iter().find(|f| {
                n(f, "v0") >= number(&off)
                    && n(f, "lineBottom") <= lim_abs
                    && n(f, "bottom") > lim_abs
                    && n(f, "v0") < carry_from
            });
            let Some(f) = f else {
                break;
            };
            let rows = list(&f, "rows");
            if !rows.is_empty() {
                let mut y = n(&f, "bottom") - n(&f, "carry") + n(&f, "outT");
                let mut k = 0;
                while k < rows.len() && y + number(&rows[k]) <= lim_abs {
                    y += number(&rows[k]);
                    k += 1;
                }
                if k == rows.len() {
                    break;
                }
                if let Some(el) = id(&f["el"]).filter(|&el| {
                    (k > 0 || d.nodes[el].attr("pageBreak") == Some("CELL")) && cont_h > 0.0
                }) {
                    let nested = split_table_rows(
                        d,
                        el,
                        n(&f, "bottom") - n(&f, "carry") - number(&base),
                        avail - n(c, "marginB") - n(c, "marginT"),
                        cont_h - n(c, "marginT") - n(c, "marginB"),
                        styles,
                        None,
                        ctx,
                        state,
                        budget,
                        depth + 1,
                    )?;
                    let rest = nested.or_else(|| {
                        if k > 0 {
                            Some(
                                n(&f, "outT")
                                    + rows[k..].iter().fold(0.0, |a, b| a + number(b))
                                    + n(&f, "outB"),
                            )
                        } else {
                            None
                        }
                    });
                    if let Some(rest) = rest {
                        split_at = Some((n(&f, "bottom"), rest));
                        carry_from = n(&f, "v0");
                        break;
                    }
                }
            }
            let delta = n(&f, "bottom") - n(&f, "hostEnd");
            if delta > 0.0 {
                for sg in segs(d, sub) {
                    let v = num(d, Some(sg), "vertpos", 0.0);
                    if v >= n(&f, "bottom") {
                        d.nodes[sg].set_attr("vertpos", string(&(v - delta).into()));
                    }
                }
            }
            carry = max(carry, n(&f, "carry"));
            carry_from = n(&f, "v0");
        }
        let segs = segs(d, sub)
            .into_iter()
            .filter(|&s| num(d, Some(s), "vertpos", 0.0) >= number(&off))
            .collect::<Vec<_>>();
        if segs.is_empty() {
            continue;
        }
        any = true;
        let lines = segs
            .iter()
            .map(|&s| {
                [
                    num(d, Some(s), "vertpos", 0.0) - number(&base),
                    num(d, Some(s), "vertpos", 0.0) + num(d, Some(s), "vertsize", 0.0)
                        - number(&base),
                ]
            })
            .collect::<Vec<_>>();
        let end = lines.iter().fold(f64::NEG_INFINITY, |a, l| max(a, l[1]));
        let Some(k) = lines
            .iter()
            .position(|l| n(c, "marginT") + l[1] > avail - n(c, "marginB"))
        else {
            placed = true;
            if carry > 0.0 {
                rem = max(rem, n(c, "marginT") + carry + n(c, "marginB"));
            }
            cuts.push((el, add(add(base.clone(), end.into()), 1.0.into()), base));
            continue;
        };
        if k > 0 {
            placed = true;
        }
        let mut nb = if k > 0 {
            end_with_space(
                d,
                segs[k - 1],
                num(d, Some(segs[k - 1]), "vertpos", 0.0),
                styles,
            )
        } else {
            base.clone()
        };
        let cut_v = num(d, Some(segs[k]), "vertpos", 0.0);
        for f in sub_float_bottoms(d, sub, ctx)? {
            if n(&f, "v0") >= number(&off) && n(&f, "v0") < cut_v && n(&f, "bottom") <= cut_v {
                nb = max(number(&nb), n(&f, "bottom")).into();
            }
        }
        if let Some((bottom, rest)) = split_at.filter(|(bottom, _)| cut_v >= *bottom) {
            nb = max(number(&nb), bottom).into();
            carry = rest - (cut_v - bottom);
        }
        if carry > 0.0 && cut_v > carry_from {
            nb = (number(&nb) - carry).into();
            if carries.get(&sub).is_none() {
                carries.insert(sub, Map::new());
            }
            carries.get_mut(&sub).unwrap().insert(key(cut_v), carry);
        }
        cuts.push((el, add(base.clone(), lines[k][0].into()), nb.clone()));
        rem = max(
            rem,
            number(&add(end.into(), base)) - number(&nb) + n(c, "marginT") + n(c, "marginB"),
        );
        if off.as_f64() == Some(0.0)
            && (fixed || matches!(d.nodes[sub].attr("vertAlign"), Some("CENTER" | "BOTTOM")))
        {
            rem = max(rem, n(c, "h") - piece);
        }
    }
    if !(any && placed) {
        return Ok(None);
    }
    for (el, v, nb) in cuts {
        from.insert(el, v);
        bases.insert(el, nb);
    }
    for c in cells {
        if c["ra"].as_f64() != Some(row) || c["rs"].as_f64() != Some(1.0) {
            continue;
        }
        let Some(sub) = id(&c["sub"]) else {
            continue;
        };
        let Some(v) = from.get(&id(&c["el"]).unwrap()) else {
            continue;
        };
        let last = segs(d, sub).iter().fold(0.0, |a, &s| {
            max(
                a,
                num(d, Some(s), "vertpos", 0.0) + num(d, Some(s), "vertsize", 0.0),
            )
        });
        if number(v) <= last
            && !record
                .get(&sub)
                .is_some_and(|l| l.iter().any(|old| old == v || matches!((old, v), (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan())))
        {
            if record.get(&sub).is_none() {
                record.insert(sub, Vec::new());
            }
            record.get_mut(&sub).unwrap().push(v.clone());
        }
    }
    Ok(Some(rem))
}
fn rebase_split_cells(
    d: &mut ParsedXml,
    record: &Record,
    carries: &Carries,
    styles: Option<&Value>,
    ctx: &mut Context,
    budget: &mut Budget,
) -> Result<(), String> {
    for (&sub, cuts) in record {
        budget.tick()?;
        let segs = segs(d, sub);
        let old = segs
            .iter()
            .map(|&s| num(d, Some(s), "vertpos", 0.0))
            .collect::<Vec<_>>();
        let floats = sub_float_bottoms(d, sub, ctx)?;
        let mut sorted = cuts.clone();
        sorted.sort_by(|a, b| {
            (number(a) - number(b))
                .partial_cmp(&0.0)
                .unwrap_or(Ordering::Equal)
        });
        let mut bases = Vec::new();
        for c in &sorted {
            budget.tick()?;
            let mut base = 0.0;
            for (i, &sg) in segs.iter().enumerate() {
                if old[i] < number(c) {
                    base = max(base, number(&end_with_space(d, sg, old[i], styles)));
                }
            }
            for f in &floats {
                if n(f, "v0") < number(c) && n(f, "bottom") <= number(c) {
                    base = max(base, n(f, "bottom"));
                }
            }
            bases.push(
                base - carries
                    .get(&sub)
                    .and_then(|m| c.as_f64().and_then(|c| m.get(&key(c))))
                    .copied()
                    .unwrap_or(0.0),
            );
        }
        for (i, &sg) in segs.iter().enumerate() {
            if let Some(j) = sorted.iter().rposition(|c| old[i] >= number(c)) {
                d.nodes[sg].set_attr("vertpos", string(&(old[i] - bases[j]).into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn select(d: &ParsedXml, input: &Value) -> Result<usize, String> {
    let mut i = d.root;
    for p in list(input, "path") {
        i = *d.nodes[i]
            .children
            .get(p.as_u64().ok_or("노드 경로 오류")? as usize)
            .ok_or("노드 경로 범위 오류")?;
    }
    Ok(i)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    if operation != "splitFloatTableRemainder" && operation != "sequence" {
        return Err("알 수 없는 표 이어 나눔 연산입니다.".into());
    }
    let input = Value::from_json(input);
    let mut d = parse_style_xml(input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?)?;
    let mut ctx = Context {
        memoize: truth(&input["memo"]),
        ..Context::default()
    };
    if truth(&input["prepare"]) {
        pm::prepare_deleted_ranges(&d, d.root, 0, &mut ctx);
    }
    let styles = input.get("styles").filter(|v| truth(v));
    if let Some(styles) = styles {
        for entry in list(styles, "paraGeom") {
            if let Some(k) = entry[0].as_str() {
                ctx.para_geom.insert(k.into(), entry[1].clone());
            }
        }
    }
    let mut state = SplitState::default();
    let steps = if operation == "sequence" {
        list(&input, "steps").to_vec()
    } else {
        vec![input.clone()]
    };
    let mut output = Vec::new();
    for step in steps {
        let p = select(&d, &step)?;
        let note = step.get("notes").filter(|v| truth(v));
        let mut calls = Vec::new();
        let mut callback = |n: Value, s: Value| -> Result<Value, String> {
            let at = calls.len();
            calls.push(json!({"n":n,"s":s}));
            let v = note.unwrap();
            if v["throwAt"].as_u64() == Some(at as u64) {
                return Err("합성 각주 계산 실패".into());
            }
            if let Some(replies) = v["responses"].as_array() {
                return Ok(replies.get(at).cloned().unwrap_or(Value::Undefined));
            }
            Ok((number(&n) * v.get("nFactor").map_or(0.0, number)
                + number(&s) * v.get("sFactor").map_or(1.0, number)
                + v.get("constant").map_or(0.0, number))
            .into())
        };
        let mut notes = note.map(|v| Notes {
            n0: v["n0"].clone(),
            s0: v["s0"].clone(),
            area: &mut callback as &mut AreaCallback<'_>,
        });
        let result = split_float_table_remainder(
            &mut d,
            p,
            n(&step, "startV"),
            n(&step, "lim"),
            n(&step, "bodyH"),
            styles,
            truth(&step["withText"]),
            notes.as_mut(),
            &mut ctx,
            &mut state,
        );
        let value = match result {
            Ok(v) => {
                json!({"defined":v.is_some(),"result":v.map_or(Value::Undefined,Value::from),"error":Value::Null})
            }
            Err(e) => json!({"defined":false,"error":e}),
        };
        let attrs=d.nodes.iter().enumerate().map(|(i,node)|json!({"id":i,"attrs":node.attributes().iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>()})).collect::<Vec<_>>();
        let mut paragraphs = Vec::new();
        for i in 0..d.nodes.len() {
            if d.nodes[i].local == "p" {
                paragraphs.push(json!({"el":i,"model":pm::build_para(&d,i,&mut ctx)?}));
            }
        }
        output.push(json!({"value":value,"state":state.to_value(),"calls":calls,"attributes":attrs,"paragraphs":paragraphs}));
    }
    Ok(Value::from(output).to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_number_keys_use_same_value_zero() {
        assert_eq!(key(0.0), key(-0.0));
        assert_eq!(key(f64::NAN), key(-f64::NAN));
    }
    #[test]
    fn budget_rejects_instead_of_returning_partial_result() {
        let mut b = Budget(MAX_SPLIT_STEPS);
        assert_eq!(b.tick().unwrap_err(), LIMIT);
    }
}
