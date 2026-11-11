//! SVG and preview HTML for the supported HWPX model. Canvas supplies actual
//! installed-font measurements; all document and presentation logic is Rust.
use crate::{
    browser_canvas::Canvas, hwpx_render_layout::layout_model, hwpx_render_model::read_render_model,
    input::js_string, office_xml::xml_escape,
};
use serde_json::{Value, json};
use std::collections::HashSet;

const INVALID: &str = "자체 렌더 결과 형식이 올바르지 않습니다.";

fn text(value: &Value) -> Result<&str, String> {
    value.as_str().ok_or_else(|| INVALID.into())
}
fn list(value: &Value) -> Result<&Vec<Value>, String> {
    value.as_array().ok_or_else(|| INVALID.into())
}
fn number(value: &Value) -> Result<f64, String> {
    value.as_f64().ok_or_else(|| INVALID.into())
}
fn num(value: f64) -> String {
    // JS interpolation spells negative zero as 0, and uses ECMAScript's
    // exponent thresholds and shortest round-trippable decimal representation.
    js_string(&json!(value))
}

fn model_fonts(model: &Value) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut fonts = Vec::new();
    let mut add = |paragraph: &Value| -> Result<(), String> {
        let font = text(&paragraph["style"]["font"])?;
        if seen.insert(font.to_owned()) {
            fonts.push(font.to_owned());
        }
        Ok(())
    };
    for block in list(&model["blocks"])? {
        if block["type"] == "paragraph" {
            add(block)?;
        } else {
            for cell in list(&block["cells"])? {
                for paragraph in list(&cell["paragraphs"])? {
                    add(paragraph)?;
                }
            }
        }
    }
    Ok(fonts)
}

fn shape_svg(shape: &Value) -> Result<String, String> {
    let x = number(&shape["x"])?;
    let y = number(&shape["y"])?;
    if shape["kind"] == "text" {
        let style = &shape["style"];
        return Ok(format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"{}\" fill=\"{}\" xml:space=\"preserve\">{}</text>",
            num(x),
            num(y),
            xml_escape(text(&style["font"])?)?,
            js_string(&style["size"]),
            if style["bold"].as_bool().ok_or(INVALID)? {
                700
            } else {
                400
            },
            text(&style["color"])?,
            xml_escape(text(&shape["text"])?)?
        ));
    }
    let width = number(&shape["width"])?;
    let height = number(&shape["height"])?;
    let mut out = format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        num(x),
        num(y),
        num(width),
        num(height),
        text(&shape["fill"])?
    );
    // The read model constructs the four named sides in insertion order.
    for (side, border) in shape["borders"].as_object().ok_or(INVALID)? {
        let stroke = number(&border["width"])?;
        if stroke == 0.0 || stroke.is_nan() {
            continue;
        }
        out.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
            num(x + if side == "right" { width } else { 0.0 }),
            num(y + if side == "bottom" { height } else { 0.0 }),
            num(x + if side == "left" { 0.0 } else { width }),
            num(y + if side == "top" { 0.0 } else { height }),
            text(&border["color"])?,
            num(stroke)
        ));
    }
    Ok(out)
}

fn rendered_result(model: Value, layout: &Value) -> Result<Value, String> {
    let page = &model["page"];
    let width = num(number(&page["width"])?);
    let height = num(number(&page["height"])?);
    let mut svgs = Vec::new();
    for (index, shapes) in list(&layout["pages"])?.iter().enumerate() {
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}pt\" height=\"{height}pt\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"자체 미리보기 {}쪽\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>",
            index + 1
        );
        for shape in list(shapes)? {
            svg.push_str(&shape_svg(shape)?);
        }
        svg.push_str("</svg>");
        svgs.push(svg);
    }
    let report = json!({
        "schemaVersion":1,
        "sourceSha256":model["sourceSha256"],
        "status":"approximate-preview",
        "pageCount":svgs.len(),
        "fonts":layout["fonts"],
        "warnings":layout["warnings"],
        "hangulVerified":false
    });
    Ok(json!({"report":report,"svgs":svgs,"model":model}))
}

/// Render with the installed Chromium engine's real font metrics, then close
/// its owned browser and temporary profile on success or failure.
pub fn render_hwpx(bytes: &[u8]) -> Result<Value, String> {
    let model = read_render_model(bytes)?;
    let mut canvas = Canvas::launch()?;
    let result = (|| {
        for font in model_fonts(&model)? {
            canvas.require_font(&font)?;
        }
        let layout = layout_model(&model, |text, style| canvas.measure(text, style))?;
        rendered_result(model, &layout)
    })();
    match (result, canvas.close()) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}\n{cleanup}")),
    }
}

/// Preserve the original standalone preview's CSP, print rules and warning
/// presentation. Optional fontCss is already-produced trusted CSS, as before.
pub fn preview_html(result: &Value) -> Result<String, String> {
    let mut out = String::from(
        "<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; font-src data:; style-src 'unsafe-inline'\"><title>한글 문서 자체 미리보기</title><style>body{margin:0;background:#e5e7eb;font-family:굴림,sans-serif}.notice{padding:16px;background:#fff3cd;line-height:1.7}main{display:flex;flex-direction:column;align-items:center;gap:24px;padding:24px}svg{background:white;box-shadow:0 2px 8px #999;max-width:100%;height:auto}@media print{.notice{display:none}main{display:block;padding:0}svg{display:block;box-shadow:none;break-after:page;max-width:none}}",
    );
    if let Some(css) = result.get("fontCss").filter(|value| !value.is_null()) {
        out.push_str(&js_string(css));
    }
    out.push_str(
        "</style><div class=\"notice\"><strong>자체 미리보기 · 실제 한글 확인 전</strong><br>",
    );
    for (index, warning) in list(&result["report"]["warnings"])?.iter().enumerate() {
        if index > 0 {
            out.push_str("<br>");
        }
        out.push_str(&xml_escape(text(warning)?)?);
    }
    out.push_str("</div><main>");
    for svg in list(&result["svgs"])? {
        // Array.join converts null/undefined to empty strings.
        if !svg.is_null() {
            out.push_str(&js_string(svg));
        }
    }
    out.push_str("</main></html>");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwpx_document::sha256;
    use crate::hwpx_form::json_stringify;
    use std::collections::HashMap;

    #[test]
    fn fixed_legacy_native_preview_with_recorded_browser_measurements_matches() {
        // Reuse the widths observed by the fixed implementation so differences
        // in installed Windows font versions do not change this layout oracle.
        let fixture = include_bytes!("../tests/fixtures/office/hwpx-render.json.gz");
        let cases: Vec<Value> =
            serde_json::from_reader(flate2::read::GzDecoder::new(fixture.as_slice())).unwrap();
        assert_eq!(cases.len(), 42);
        for case in cases {
            let label = case["label"].as_str().unwrap();
            let source = data_encoding::BASE64
                .decode(case["bytes"].as_str().unwrap().as_bytes())
                .unwrap();
            let model = read_render_model(&source);
            if let Some(message) = case["error"].as_str() {
                let error = match model {
                    Err(error) => error,
                    Ok(_) if message == "글꼴" => {
                        // The browser integration test exercises actual missing fonts.
                        continue;
                    }
                    Ok(model) => layout_model(&model, |_, _| Ok(0.0)).expect_err(label),
                };
                assert!(error.contains(message), "{label}: {error}");
                continue;
            }
            let model = model.unwrap_or_else(|error| panic!("{label}: {error}"));
            assert_eq!(
                json_stringify(&model),
                json_stringify(&case["model"]),
                "{label}: 모델"
            );
            let widths = case["widths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| (entry[0].as_str().unwrap(), entry[1].as_f64().unwrap()))
                .collect::<HashMap<_, _>>();
            let layout = layout_model(&model, |text, style| {
                let key =
                    json_stringify(&json!([text, style["font"], style["size"], style["bold"]]));
                widths
                    .get(key.as_str())
                    .copied()
                    .ok_or_else(|| format!("이전 측정에 없는 문자열과 서식: {key}"))
            })
            .unwrap_or_else(|error| panic!("{label}: {error}"));
            let actual = rendered_result(model, &layout).unwrap();
            assert_eq!(actual["report"], case["report"], "{label}: 보고서");
            let hashes = actual["svgs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|svg| sha256(svg.as_str().unwrap().as_bytes()))
                .collect::<Vec<_>>();
            assert_eq!(json!(hashes), case["svgSha256"], "{label}: 쪽별 SVG 바이트");
            assert_eq!(
                sha256(preview_html(&actual).unwrap().as_bytes()),
                case["htmlSha256"].as_str().unwrap(),
                "{label}: HTML 전체 바이트"
            );
        }
    }

    #[test]
    fn text_shapes_preserve_xml_and_js_number_spelling() {
        let shape = json!({
            "kind":"text","x":-0.0,"y":1e-7,"text":"<&>\r한😀 \"",
            "style":{"font":"A&B<>\"","size":12.0,"bold":true,"color":"#123456"}
        });
        assert_eq!(
            shape_svg(&shape).unwrap(),
            "<text x=\"0\" y=\"1e-7\" font-family=\"A&amp;B&lt;&gt;\"\" font-size=\"12\" font-weight=\"700\" fill=\"#123456\" xml:space=\"preserve\">&lt;&amp;&gt;&#13;한😀 \"</text>"
        );
        let mut invalid = shape;
        invalid["text"] = json!("\u{0001}");
        assert!(shape_svg(&invalid).is_err());
    }

    #[test]
    fn borders_keep_side_order_geometry_and_zero_omission() {
        let shape = json!({
            "kind":"rect","x":1.0,"y":2.0,"width":3.0,"height":4.0,"fill":"none",
            "borders":{
                "right":{"width":0.5,"color":"#000000"},
                "bottom":{"width":1.0,"color":"#111111"},
                "left":{"width":2.0,"color":"#222222"},
                "top":{"width":0.0,"color":"#333333"}
            }
        });
        assert_eq!(
            shape_svg(&shape).unwrap(),
            concat!(
                "<rect x=\"1\" y=\"2\" width=\"3\" height=\"4\" fill=\"none\"/>",
                "<line x1=\"4\" y1=\"2\" x2=\"4\" y2=\"6\" stroke=\"#000000\" stroke-width=\"0.5\"/>",
                "<line x1=\"1\" y1=\"6\" x2=\"4\" y2=\"6\" stroke=\"#111111\" stroke-width=\"1\"/>",
                "<line x1=\"1\" y1=\"2\" x2=\"1\" y2=\"6\" stroke=\"#222222\" stroke-width=\"2\"/>"
            )
        );
    }

    #[test]
    fn reports_and_standalone_html_preserve_contract() {
        let model = json!({"page":{"width":100.25,"height":200.0},"sourceSha256":"synthetic"});
        let layout = json!({"pages":[[],[]],"fonts":["굴림"],"warnings":["<&>\r","한😀"]});
        let mut result = rendered_result(model.clone(), &layout).unwrap();
        assert_eq!(result["model"], model);
        assert_eq!(
            result["report"],
            json!({
                "schemaVersion":1,"sourceSha256":"synthetic","status":"approximate-preview",
                "pageCount":2,"fonts":["굴림"],"warnings":["<&>\r","한😀"],"hangulVerified":false
            })
        );
        assert_eq!(
            result["svgs"][1],
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100.25pt\" height=\"200pt\" viewBox=\"0 0 100.25 200\" role=\"img\" aria-label=\"자체 미리보기 2쪽\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/></svg>"
        );
        let plain = preview_html(&result).unwrap();
        assert!(plain.contains("content=\"default-src 'none'; img-src data:; font-src data:; style-src 'unsafe-inline'\""));
        assert!(plain.contains("<br>&lt;&amp;&gt;&#13;<br>한😀</div><main><svg"));
        assert!(plain.ends_with("</svg></main></html>"));
        result["fontCss"] = Value::Null;
        assert_eq!(preview_html(&result).unwrap(), plain);
        result["fontCss"] = json!("@font-face{font-family:Test}");
        assert!(
            preview_html(&result)
                .unwrap()
                .contains("}}@font-face{font-family:Test}</style>")
        );
        result["fontCss"] = json!(false);
        assert!(preview_html(&result).unwrap().contains("}}false</style>"));
    }

    #[test]
    fn fonts_follow_first_document_and_unsorted_cell_appearance() {
        let q = |font| json!({"style":{"font":font}});
        let mut first = q("첫째");
        first["type"] = json!("paragraph");
        let model = json!({"blocks":[first,{"type":"table","cells":[
            {"c":1,"paragraphs":[q("둘째"),q("첫째")]},
            {"c":0,"paragraphs":[q("셋째")]}
        ]}]});
        assert_eq!(model_fonts(&model).unwrap(), ["첫째", "둘째", "셋째"]);
    }
}
