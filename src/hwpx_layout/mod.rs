//! Native implementation of the existing complex HWPX layout engine.
//! Derived sources retain their MIT notices in data/kordoc-*.
pub mod equation;
pub mod fill_paint;
pub mod font_env;
pub mod glyph;
pub mod head_styles;
pub mod header_xml_compat;
pub mod hwpx_loader;
pub mod ieee754;
pub mod layout;
pub mod layout_text;
pub mod line_plan;
pub mod metrics;
pub mod notes;
pub mod numbering;
pub mod object_anchor;
pub mod page_stories;
pub mod para_model;
pub mod para_number;
pub mod preview_fonts;
pub mod pua;
#[cfg(test)]
mod reference_tests;
pub mod reflow_block;
pub mod reflow_children;
pub mod reflow_helpers;
pub mod reflow_para;
pub mod scene;
pub mod section_render;
pub mod shape_geometry;
mod style_dom;
pub mod svg_assemble;
pub mod svg_images;
pub mod svg_objects;
pub mod svg_para;
pub mod svg_renderer;
pub mod svg_safety;
pub mod svg_table;
pub mod table_flow;
pub mod table_pages;
pub mod table_split;
pub mod value;

/// Read-only comparison bridge; batch execution keeps each error independent.
#[cfg(test)]
fn probe(command: &str, request: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    if let Some(cases) = request.get("cases").and_then(serde_json::Value::as_array) {
        return Ok(cases
            .iter()
            .map(|case| match probe(command, case) {
                Ok(value) => json!({"ok":true,"value":value}),
                Err(error) => json!({"ok":false,"error":error}),
            })
            .collect());
    }
    let op = request["operation"]
        .as_str()
        .ok_or("operation이 필요합니다.")?;
    let input = &request["input"];
    match command {
        "layout-font-env" => font_env::run(op, input),
        "layout-hwpx-loader" => hwpx_loader::run(op, input),
        "layout-section-render" => section_render::run(op, input),
        "layout-svg-assemble" => svg_assemble::run(op, input),
        "layout-reflow-block" => reflow_block::run(op, input),
        "layout-svg-images" => svg_images::run(op, input),
        "layout-svg-objects" => svg_objects::run(op, input),
        "layout-svg-table" => svg_table::run(op, input),
        "layout-reflow-para" => reflow_para::run(op, input),
        "layout-svg-para" => svg_para::run(op, input),
        "layout-pua" => pua::run(op, input),
        "layout-text" => layout_text::run(op, input),
        "layout-object-anchor" => object_anchor::run(op, input),
        "layout-page-stories" => page_stories::run(op, input),
        "layout-reflow-helpers" => reflow_helpers::run(op, input),
        "layout-reflow-children" => reflow_children::run(op, input),
        "layout-table-flow" => table_flow::run(op, input),
        "layout-table-pages" => table_pages::run(op, input),
        "layout-table-split" => table_split::run(op, input),
        "layout-line-plan" => line_plan::run(op, input),
        "layout-header-compat" => header_xml_compat::run(op, input),
        "layout-equation" => equation::run(op, input),
        "layout-shape-geometry" => shape_geometry::run(op, input),
        "layout-para-model" => para_model::run(op, input),
        "layout-numbering" => match request["module"].as_str() {
            Some("numbering") => numbering::run(op, input),
            Some("para_number") => para_number::run(op, input),
            Some("notes") => notes::run(op, input),
            _ => Err("알 수 없는 번호 모듈입니다.".into()),
        },
        "layout-head-styles" => head_styles::run(op, input),
        "layout-metrics" => metrics::run(request),
        "layout-preview-fonts" if op == "validateSvg" => svg_safety::run(op, input),
        "layout-preview-fonts" => preview_fonts::run(op, input),
        "layout-primitives" => match request["module"].as_str() {
            Some("layout") => layout::run(op, input),
            Some("scene") => scene::run(op, input),
            Some("fill") => fill_paint::run(op, input),
            _ => Err("알 수 없는 layout 모듈입니다.".into()),
        },
        _ => Err("알 수 없는 layout 명령입니다.".into()),
    }
}
