//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/layout.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
//! Keep operation order.
use crate::input::{js_string, trim};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::collections::HashMap;

// Number("0x...") rounds the complete integer once, rather than rounding after
// every digit. All JS-prefixed radices are powers of two, so retain the leading
// 53 bits plus guard/sticky bits and apply IEEE ties-to-even explicitly.
fn radix_number(digits: &str, radix: u32) -> f64 {
    let width = radix.trailing_zeros();
    let mut started = false;
    let mut length = 0_usize;
    let mut mantissa = 0_u64;
    let mut guard = false;
    let mut sticky = false;
    for digit in digits
        .chars()
        .map(|c| c.to_digit(radix).expect("validated radix"))
    {
        for shift in (0..width).rev() {
            let bit = (digit >> shift) & 1;
            if !started && bit == 0 {
                continue;
            }
            started = true;
            length += 1;
            if length <= 53 {
                mantissa = mantissa * 2 + u64::from(bit);
            } else if length == 54 {
                guard = bit != 0;
            } else {
                sticky |= bit != 0;
            }
        }
    }
    if length <= 53 {
        return mantissa as f64;
    }
    if length > 1024 {
        return f64::INFINITY;
    }
    if guard && (sticky || mantissa & 1 != 0) {
        mantissa += 1;
    }
    mantissa as f64 * 2.0_f64.powi(length as i32 - 53)
}
pub fn number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(value) => {
            let text = js_string(value);
            let text = trim(&text);
            if text.is_empty() {
                return 0.0;
            }
            if matches!(text, "Infinity" | "+Infinity") {
                return f64::INFINITY;
            }
            if text == "-Infinity" {
                return f64::NEG_INFINITY;
            }
            for (prefix, radix) in [
                ("0x", 16),
                ("0X", 16),
                ("0o", 8),
                ("0O", 8),
                ("0b", 2),
                ("0B", 2),
            ] {
                if let Some(digits) = text.strip_prefix(prefix) {
                    if digits.is_empty()
                        || !digits.chars().all(|c| c.is_ascii() && c.is_digit(radix))
                    {
                        return f64::NAN;
                    }
                    return radix_number(digits, radix);
                }
            }
            if !text
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
            {
                return f64::NAN;
            }
            text.parse().unwrap_or(f64::NAN)
        }
    }
}
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}
pub fn string(value: Option<&Value>) -> String {
    value.map(js_string).unwrap_or_else(|| "undefined".into())
}
pub fn number_value(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}
pub fn maximum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() || b.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else {
        a.max(b)
    }
}
pub fn minimum(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() || b.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else {
        a.min(b)
    }
}
pub fn to_int32(value: f64, fallback: f64) -> f64 {
    if !value.is_finite() {
        fallback
    } else if value > 2147483647.0 {
        value - 4294967296.0
    } else {
        value
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub a: f64,
    pub b: f64,
    pub size: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct CellRow {
    pub row_addr: f64,
    pub row_span: f64,
    pub height: f64,
    pub content_h: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct LongestPath {
    pub x: Vec<f64>,
    pub tight: Vec<bool>,
}
fn index(value: f64, length: usize) -> Option<usize> {
    if value >= 0.0 && value.fract() == 0.0 && value < (length as f64) {
        Some(value as usize)
    } else {
        None
    }
}
fn key(value: f64) -> String {
    ryu_js::Buffer::new().format(value).to_owned()
}
pub fn solve_boundaries(constraints: &[Constraint], count: usize, total: Option<f64>) -> Vec<f64> {
    let mut x = vec![None; count + 1];
    let mut extra = HashMap::new();
    x[0] = Some(0.0);
    if let Some(total) = total.filter(|v| *v > 0.0) {
        x[count] = Some(total);
    }
    let get = |x: &[Option<f64>], extra: &HashMap<String, f64>, i: f64| {
        index(i, x.len())
            .and_then(|i| x[i])
            .or_else(|| extra.get(&key(i)).copied())
    };
    let mut changed = true;
    let mut guard = 0;
    while changed && guard < count + 8 {
        guard += 1;
        changed = false;
        for c in constraints {
            if c.a < 0.0 || c.b > count as f64 || c.a >= c.b {
                continue;
            }
            let a = get(&x, &extra, c.a);
            let b = get(&x, &extra, c.b);
            let value = match (a, b) {
                (Some(a), None) => Some((c.b, a + c.size)),
                (None, Some(b)) => Some((c.a, b - c.size)),
                _ => None,
            };
            if let Some((i, v)) = value {
                if let Some(i) = index(i, x.len()) {
                    x[i] = Some(v);
                } else {
                    extra.insert(key(i), v);
                }
                changed = true;
            }
        }
    }
    let mut i = 0;
    while i <= count {
        if x[i].is_some() {
            i += 1;
            continue;
        }
        let lo = i - 1;
        let mut hi = i;
        while hi <= count && x[hi].is_none() {
            hi += 1;
        }
        let lo_v = x[lo].unwrap();
        let hi_v = if hi <= count {
            x[hi].unwrap()
        } else {
            lo_v + (hi - lo) as f64 * 1000.0
        };
        let n = hi - lo;
        for k in 1..n {
            x[lo + k] = Some(lo_v + (hi_v - lo_v) * k as f64 / n as f64);
        }
        if hi > count {
            x[count] = Some(hi_v);
        }
        i = hi;
    }
    let mut out: Vec<f64> = x.into_iter().map(Option::unwrap).collect();
    for k in 1..=count {
        if out[k] < out[k - 1] {
            out[k] = out[k - 1];
        }
    }
    out
}
pub fn longest_path_boundaries(constraints: &[Constraint], count: usize) -> Option<LongestPath> {
    let inner: Vec<_> = constraints
        .iter()
        .filter(|c| {
            c.a >= 0.0 && c.b <= count as f64 && c.a < c.b && !(c.a == 0.0 && c.b == count as f64)
        })
        .collect();
    let mut x = vec![f64::NEG_INFINITY; count + 1];
    x[0] = 0.0;
    for k in 1..=count {
        for c in &inner {
            if c.b == k as f64
                && let Some(a) = index(c.a, x.len())
                && x[a] > f64::NEG_INFINITY
            {
                x[k] = maximum(x[k], x[a] + c.size);
            }
        }
        if x[k] == f64::NEG_INFINITY {
            return None;
        }
        x[k] = maximum(x[k], x[k - 1]);
    }
    let mut y = vec![f64::NEG_INFINITY; count + 1];
    y[count] = 0.0;
    for k in (0..count).rev() {
        y[k] = y[k + 1];
        for c in &inner {
            if c.a == k as f64
                && let Some(b) = index(c.b, y.len())
                && y[b] > f64::NEG_INFINITY
            {
                y[k] = maximum(y[k], c.size + y[b]);
            }
        }
    }
    let tight = x
        .iter()
        .enumerate()
        .map(|(k, v)| (v + y[k] - x[count]).abs() <= 2.0)
        .collect();
    Some(LongestPath { x, tight })
}
pub fn table_column_boundaries(
    constraints: &[Constraint],
    count: usize,
    total: Option<f64>,
    no_exact_row: bool,
) -> Vec<f64> {
    let prop = solve_boundaries(constraints, count, total);
    let Some(total) = total.filter(|v| *v != 0.0 && !v.is_nan() && *v > 0.0) else {
        return prop;
    };
    if let Some(lp) = longest_path_boundaries(constraints, count)
        && (lp.x[count] - total).abs() <= 2.0
        && (no_exact_row
            || lp
                .x
                .iter()
                .enumerate()
                .all(|(k, v)| (v - prop[k]).abs() <= 2.0 || lp.tight[k]))
    {
        lp.x
    } else {
        prop
    }
}
struct Span {
    cell: CellRow,
    rows: Vec<f64>,
}
fn row_get(h: &[f64], r: f64) -> f64 {
    index(r, h.len()).map_or(f64::NAN, |i| h[i])
}
fn row_sum(h: &[f64], rows: &[f64]) -> f64 {
    rows.iter().fold(0.0, |sum, &r| sum + row_get(h, r))
}
fn solve_span(
    spans: &[Span],
    k: usize,
    h: &mut [f64],
    done: &mut [bool],
    even_only: bool,
    max_total: f64,
) -> bool {
    let Span { cell, rows } = &spans[k];
    let known = row_sum(h, rows);
    let missing: Vec<_> = rows
        .iter()
        .copied()
        .filter(|&r| row_get(h, r) == 0.0)
        .collect();
    if missing.is_empty() {
        done[k] = true;
        return false;
    }
    if even_only && missing.len() != 1 {
        return false;
    }
    if even_only && cell.height > max_total {
        return false;
    }
    done[k] = true;
    if cell.height <= known {
        return false;
    }
    let each = (cell.height - known) / missing.len() as f64;
    for r in missing {
        if let Some(i) = index(r, h.len()) {
            h[i] = each;
        }
    }
    true
}
fn slice_index(n: f64, len: usize) -> usize {
    let n = if n.is_nan() { 0.0 } else { n.trunc() };
    if n < 0.0 {
        maximum(len as f64 + n, 0.0) as usize
    } else {
        minimum(n, len as f64) as usize
    }
}
pub fn solve_row_heights(
    cells: &[CellRow],
    row_count: usize,
    max_total: f64,
    shrink_last: bool,
) -> Vec<f64> {
    let mut h = vec![0.0; row_count];
    for c in cells {
        if c.row_span == 1.0
            && let Some(r) = index(c.row_addr, row_count)
        {
            h[r] = maximum(h[r], c.height);
        }
    }
    for c in cells {
        if c.row_span == 1.0
            && let Some(content) = c.content_h
            && let Some(r) = index(c.row_addr, row_count)
            && content > h[r]
        {
            h[r] = content;
        }
    }
    let spans: Vec<_> = cells
        .iter()
        .filter(|c| c.row_span > 1.0)
        .map(|&cell| {
            let mut rows = Vec::new();
            let mut r = cell.row_addr;
            let end = minimum(cell.row_addr + cell.row_span, row_count as f64);
            while r < end {
                rows.push(r);
                r += 1.0;
            }
            Span { cell, rows }
        })
        .collect();
    let mut done = vec![false; spans.len()];
    for r in 0..row_count {
        if h[r] != 0.0 {
            continue;
        }
        let mut need = 0.0;
        for Span { cell, rows } in &spans {
            if rows.last() == Some(&(r as f64))
                && rows[..rows.len() - 1].iter().all(|&q| row_get(&h, q) > 0.0)
                && cell.height <= max_total
            {
                need = maximum(need, cell.height - row_sum(&h, &rows[..rows.len() - 1]));
            }
        }
        if need > 0.0 {
            h[r] = need;
        }
    }
    let mut again = true;
    while again {
        again = false;
        for k in 0..spans.len() {
            if !done[k] && solve_span(&spans, k, &mut h, &mut done, true, max_total) {
                again = true;
            }
        }
    }
    for k in 0..spans.len() {
        if !done[k] {
            solve_span(&spans, k, &mut h, &mut done, false, max_total);
        }
    }
    let mut total = h.iter().fold(0.0, |sum, v| sum + v);
    let over_size = total > max_total;
    let mut order: Vec<_> = (0..spans.len()).collect();
    order.sort_by(|&a, &b| {
        let a = &spans[a].rows;
        let b = &spans[b].rows;
        let primary = a.last().copied().unwrap_or(f64::NAN) - b.last().copied().unwrap_or(f64::NAN);
        let cmp = if primary != 0.0 && !primary.is_nan() {
            primary
        } else {
            b.first().copied().unwrap_or(f64::NAN) - a.first().copied().unwrap_or(f64::NAN)
        };
        cmp.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    for k in order {
        let Span { cell, rows } = &spans[k];
        let known = row_sum(&h, rows);
        let limit = if over_size
            && (cell.content_h.is_some_and(|v| v >= cell.height) || cell.height <= max_total / 2.0)
        {
            f64::INFINITY
        } else {
            max_total - total
        };
        let add = minimum(cell.height - known, limit);
        if !rows.is_empty() && add > 0.0 {
            if let Some(r) = index(*rows.last().unwrap(), h.len()) {
                h[r] += add;
            }
            total += add;
        }
    }
    let last = row_count as f64 - 1.0;
    let over = total - max_total;
    let last_cells: Vec<_> = cells
        .iter()
        .filter(|c| c.row_addr + c.row_span - 1.0 == last)
        .collect();
    if shrink_last
        && over > 0.0
        && row_count > 0
        && !last_cells.is_empty()
        && last_cells.iter().all(|c| {
            c.content_h.is_some_and(|content| {
                let span_h = if c.row_span == 1.0 {
                    h[row_count - 1]
                } else {
                    let start = slice_index(c.row_addr, h.len());
                    let end = slice_index(c.row_addr + c.row_span, h.len());
                    if start >= end {
                        0.0
                    } else {
                        h[start..end].iter().fold(0.0, |sum, v| sum + v)
                    }
                };
                content <= span_h - over
            })
        })
    {
        h[row_count - 1] -= over;
    }
    h
}
#[cfg(test)]
fn count(input: &Value, key: &str) -> Result<usize, String> {
    let n = number(input.get(key));
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > 4294967294.0 {
        return Err("Invalid array length".into());
    }
    if n > 1_000_000.0 {
        return Err("표 경계 계산 한도를 넘었습니다.".into());
    }
    Ok(n as usize)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &Value) -> Result<Value, String> {
    if operation == "toInt32" {
        let fallback = input.get("fallback").cloned().unwrap_or(json!(0));
        let value = input.get("v");
        if value.is_none_or(|v| v.is_null() || v == "") {
            return Ok(fallback);
        }
        let n = number(value);
        return Ok(if n.is_finite() {
            number_value(to_int32(n, 0.0))
        } else {
            fallback
        });
    }
    if operation == "solveRowHeights" {
        let cells = input["cells"]
            .as_array()
            .ok_or("cells must be an array")?
            .iter()
            .map(|c| CellRow {
                row_addr: number(c.get("rowAddr")),
                row_span: number(c.get("rowSpan")),
                height: number(c.get("height")),
                content_h: c
                    .get("contentH")
                    .filter(|v| !v.is_null())
                    .map(|v| number(Some(v))),
            })
            .collect::<Vec<_>>();
        let max_total = input
            .get("maxTotal")
            .map_or(f64::INFINITY, |v| number(Some(v)));
        return Ok(Value::Array(
            solve_row_heights(
                &cells,
                count(input, "rowCount")?,
                max_total,
                truthy(input.get("shrinkLast")),
            )
            .into_iter()
            .map(number_value)
            .collect(),
        ));
    }
    // The original new Array(count + 1) accepts -1 as an empty array; x[0] then
    // extends it to one item, while x[-1]/y[-1] are non-index properties.
    if input.get("count").is_some_and(|v| v.as_f64() == Some(-1.0))
        && input["constraints"].is_array()
    {
        return match operation {
            "solveBoundaries" | "tableColumnBoundaries" => Ok(json!([0])),
            "longestPathBoundaries" => Ok(json!({"x":[0],"tight":[false]})),
            _ => Err(format!("알 수 없는 layout 연산: {operation}")),
        };
    }
    let constraints = input["constraints"]
        .as_array()
        .ok_or("constraints must be an array")?
        .iter()
        .map(|c| Constraint {
            a: number(c.get("a")),
            b: number(c.get("b")),
            size: number(c.get("size")),
        })
        .collect::<Vec<_>>();
    let count = count(input, "count")?;
    let total = input
        .get("total")
        .filter(|v| !v.is_null())
        .map(|v| number(Some(v)));
    match operation{
        "solveBoundaries"=>Ok(Value::Array(solve_boundaries(&constraints,count,total).into_iter().map(number_value).collect())),
        "longestPathBoundaries"=>Ok(longest_path_boundaries(&constraints,count).map_or(Value::Null,|lp|json!({"x":lp.x.into_iter().map(number_value).collect::<Vec<_>>(),"tight":lp.tight}))),
        "tableColumnBoundaries"=>Ok(Value::Array(table_column_boundaries(&constraints,count,total,truthy(input.get("noExactRow"))).into_iter().map(number_value).collect())),
        _=>Err(format!("알 수 없는 layout 연산: {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_input_rejects_excessive_allocation_before_solving() {
        assert!(
            run("solveRowHeights", &json!({"cells":[],"rowCount":1000001}))
                .unwrap_err()
                .contains("한도")
        );
        assert!(
            run(
                "solveBoundaries",
                &json!({"constraints":[],"count":4294967294u64})
            )
            .unwrap_err()
            .contains("한도")
        );
    }
    #[test]
    fn conversion_preserves_fractions_and_subtracts_only_once() {
        assert_eq!(to_int32(1.75, 0.0), 1.75);
        assert_eq!(to_int32(2147483647.5, 0.0), -2147483648.5);
        assert_eq!(to_int32(8589934592.0, 0.0), 4294967296.0);
        assert_eq!(
            number(Some(&json!("0x10000000000000801"))),
            18446744073709556000.0
        );
    }
    #[test]
    fn open_end_retains_original_interpolation_order() {
        assert_eq!(solve_boundaries(&[], 2, None), vec![0.0, 1000.0, 3000.0]);
        let constraints = [Constraint {
            a: 1.0,
            b: 2.0,
            size: 40.0,
        }];
        assert_eq!(
            solve_boundaries(&constraints, 2, Some(100.0)),
            vec![0.0, 60.0, 100.0]
        );
    }
    #[test]
    fn last_row_shrink_requires_every_cell_content_to_fit() {
        let cells = [
            CellRow {
                row_addr: 0.0,
                row_span: 1.0,
                height: 50.0,
                content_h: Some(10.0),
            },
            CellRow {
                row_addr: 1.0,
                row_span: 1.0,
                height: 50.0,
                content_h: Some(15.0),
            },
        ];
        assert_eq!(solve_row_heights(&cells, 2, 75.0, true), vec![50.0, 25.0]);
        let mut cells = cells;
        cells[1].content_h = None;
        assert_eq!(solve_row_heights(&cells, 2, 75.0, true), vec![50.0, 50.0]);
    }
}
