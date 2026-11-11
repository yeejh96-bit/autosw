//! Derived from kordoc (MIT), Copyright 2026 chrisryugj, with my-system gradient handling.
//! Original render/fill-paint.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
#[cfg(test)]
use super::value::native_json as json;
use super::{
    layout::{maximum, minimum},
    numbering::{add, truth},
    value::{Value, number, string},
};

fn n(value: f64) -> String {
    ryu_js::Buffer::new().format(value).to_owned()
}
fn rect(fill: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"/>",
        n(x / 100.0),
        n(y / 100.0),
        n(w / 100.0),
        n(h / 100.0)
    )
}
fn rgb(value: Option<&Value>) -> Result<[f64; 3], String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or("gradient color must support slice")?;
    let units = text.encode_utf16().collect::<Vec<_>>();
    Ok([1, 3, 5].map(|start| {
        let slice =
            String::from_utf16_lossy(&units[start.min(units.len())..(start + 2).min(units.len())]);
        let mut digits = crate::input::trim(&slice);
        let mut sign = 1.0;
        if let Some(rest) = digits.strip_prefix('-') {
            digits = rest;
            sign = -1.0;
        } else if let Some(rest) = digits.strip_prefix('+') {
            digits = rest;
        }
        if let Some(rest) = digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
        {
            digits = rest;
        }
        let mut out = 0.0;
        let mut used = false;
        for c in digits.chars() {
            let Some(digit) = c.to_digit(16).filter(|_| c.is_ascii()) else {
                break;
            };
            out = out * 16.0 + f64::from(digit);
            used = true;
        }
        if used { out * sign } else { f64::NAN }
    }))
}
fn hex_round(value: f64) -> String {
    // Math.round chooses the integer toward +Infinity at a half, including negative halves.
    if value.is_nan() {
        return "NaN".into();
    }
    if !value.is_finite() {
        return n(value);
    }
    let lower = value.floor();
    let rounded = if value - lower < 0.5 {
        lower
    } else {
        lower + 1.0
    };
    let text = if rounded < 0.0 {
        format!("-{:x}", -rounded as i64)
    } else {
        format!("{:x}", rounded as i64)
    };
    format!("{text:0>2}")
}
/// Returns SVG markup and appends gradient definitions exactly in encounter order.
pub fn paint_fill(
    bf: &Value,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    defs: &mut Vec<Value>,
) -> Result<String, String> {
    paint_fill_value(bf, &x.into(), &y.into(), &w.into(), &h.into(), defs)
}
/// Keep JS addition until the final SVG coordinate division.
pub fn paint_fill_value(
    bf: &Value,
    x: &Value,
    y: &Value,
    w: &Value,
    h: &Value,
    defs: &mut Vec<Value>,
) -> Result<String, String> {
    if !truth(&bf["gradient"]) {
        return Ok(if truth(&bf["fill"]) {
            rect(
                &string(&bf["fill"]),
                number(x),
                number(y),
                number(w),
                number(h),
            )
        } else {
            String::new()
        });
    }
    let g = &bf["gradient"];
    let colors = g["colors"]
        .as_array()
        .ok_or("gradient colors must be an array")?;
    let first = colors.first();
    let last = colors.last();
    if g["type"].as_str() == Some("SQUARE") {
        let a = rgb(last)?;
        let b = rgb(first)?;
        let mut out = String::new();
        let cx = maximum(0.0, minimum(100.0, number(&g["centerX"]))) / 100.0;
        let cy = maximum(0.0, minimum(100.0, number(&g["centerY"]))) / 100.0;
        for i in 0..64 {
            let t = f64::from(i) / 64.0;
            let color = format!(
                "#{}",
                (0..3)
                    .map(|k| hex_round(a[k] + (b[k] - a[k]) * t))
                    .collect::<String>()
            );
            out.push_str(&rect(
                &color,
                number(&add(x, &(number(w) * cx * t).into())),
                number(&add(y, &(number(h) * cy * t).into())),
                number(w) * (1.0 - t),
                number(h) * (1.0 - t),
            ));
        }
        return Ok(out);
    }
    let id = format!("mysystem-gradient-{}", defs.len());
    let angle = number(&g["angle"]) * std::f64::consts::PI / 180.0;
    // Preserve V8's polynomial and argument-reduction operation order, including
    // the final bit: it is visible in the exact SVG attribute strings.
    let dx = super::ieee754::sin(angle) / 2.0;
    let dy = super::ieee754::cos(angle) / 2.0;
    let center = maximum(
        0.0,
        minimum(
            100.0,
            number(
                &g[if dx.abs() > dy.abs() {
                    "centerX"
                } else {
                    "centerY"
                }],
            ),
        ),
    );
    defs.push(Value::String(format!("<linearGradient id=\"{id}\" x1=\"{}%\" y1=\"{}%\" x2=\"{}%\" y2=\"{}%\"><stop offset=\"0%\" stop-color=\"{}\"/><stop offset=\"{}%\" stop-color=\"{}\"/><stop offset=\"100%\" stop-color=\"{}\"/></linearGradient>", n((0.5-dx)*100.0), n((0.5-dy)*100.0), n((0.5+dx)*100.0), n((0.5+dy)*100.0), string(last.unwrap_or(&Value::Undefined)), n(center), string(first.unwrap_or(&Value::Undefined)), string(last.unwrap_or(&Value::Undefined)))));
    Ok(rect(
        &format!("url(#{id})"),
        number(x),
        number(y),
        number(w),
        number(h),
    ))
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    if operation != "paintFill" {
        return Err(format!("알 수 없는 fill 연산: {operation}"));
    }
    let mut defs = match input.get("defs") {
        None => Vec::new(),
        Some(value) => value.as_array().ok_or("defs must be an array")?.clone(),
    };
    let svg = paint_fill_value(
        &input["bf"],
        &input["x"],
        &input["y"],
        &input["w"],
        &input["h"],
        &mut defs,
    )?;
    Ok(json!({"svg":svg,"defs":defs}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gradient_numbers_are_not_normalized_before_drawing() {
        let mut defs = Vec::new();
        let svg = paint_fill(&json!({"gradient":{"type":"SQUARE","colors":["#ffffff","#000000"],"centerX":f64::NAN,"centerY":50}}),0.0,0.0,100.0,200.0,&mut defs).unwrap();
        assert!(svg.contains("x=\"NaN\""));
        assert_eq!(svg.matches("<rect ").count(), 64);
        assert!(defs.is_empty());
    }
    #[test]
    fn square_keeps_sixty_four_layers_and_no_defs() {
        let mut defs = vec![json!("existing")];
        let svg = paint_fill(&json!({"gradient":{"type":"SQUARE","colors":["#ffffff","#000000"],"centerX":50,"centerY":50}}), 0.0, 0.0, 100.0, 200.0, &mut defs).unwrap();
        assert_eq!(svg.matches("<rect ").count(), 64);
        assert!(
            svg.starts_with("<rect x=\"0\" y=\"0\" width=\"1\" height=\"2\" fill=\"#000000\"/>")
        );
        assert_eq!(defs, vec![json!("existing")]);
    }
}
