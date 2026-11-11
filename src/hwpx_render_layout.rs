//! Pure pagination for an already-read HWPX render model. The callback supplies
//! real measured widths in points; synthetic measurements are unit tests only.
use regex::Regex;
use serde_json::{Value, json};
use std::{collections::HashSet, sync::LazyLock};

static TOKENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9]+(?:[._/@-][A-Za-z0-9]+)*|[^A-Za-z0-9]").unwrap());
const INVALID: &str = "자체 렌더 모델 형식이 올바르지 않습니다.";
fn number(value: &Value, key: &str) -> Result<f64, String> {
    value[key].as_f64().ok_or_else(|| INVALID.into())
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().ok_or_else(|| INVALID.into())
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| INVALID.into())
}

struct ParagraphLayout<'a> {
    paragraph: &'a Value,
    lines: Vec<String>,
    at: f64,
}
struct CellLayout<'a> {
    cell: &'a Value,
    paragraphs: Vec<ParagraphLayout<'a>>,
    content_height: f64,
}
struct RowLayout<'a> {
    cells: Vec<CellLayout<'a>>,
    height: f64,
}
struct Layout<F> {
    measure: F,
    pages: Vec<Vec<Value>>,
    y: f64,
    top: f64,
    bottom: f64,
}

impl<F: FnMut(&str, &Value) -> Result<f64, String>> Layout<F> {
    fn lines(&mut self, paragraph: &Value, width: f64) -> Result<Vec<String>, String> {
        let available = width - number(paragraph, "left")? - number(paragraph, "right")?;
        let style = &paragraph["style"];
        if available < number(style, "size")? {
            return Err("본문·셀 너비가 글자보다 작습니다.".into());
        }
        let mut result = Vec::new();
        for explicit in string(paragraph, "text")?.split('\n') {
            if explicit.is_empty() {
                result.push(String::new());
                continue;
            }
            let mut line = String::new();
            for token in TOKENS.find_iter(explicit).map(|m| m.as_str()) {
                let combined = format!("{line}{token}");
                if (self.measure)(&combined, style)? <= available {
                    line.push_str(token);
                    continue;
                }
                if !line.is_empty() {
                    result.push(std::mem::take(&mut line));
                }
                // JavaScript /u and for-of use Unicode scalars, not UTF-16 units.
                for character in token.chars() {
                    let combined = format!("{line}{character}");
                    if (self.measure)(&combined, style)? > available && !line.is_empty() {
                        result.push(std::mem::take(&mut line));
                    }
                    line.push(character);
                }
            }
            result.push(line);
        }
        Ok(result)
    }
    fn next(&mut self) -> Result<(), String> {
        if self.pages.len() >= 500 {
            return Err("미리보기 500쪽 한도 초과".into());
        }
        self.pages.push(Vec::new());
        self.y = self.top;
        Ok(())
    }
    fn add(&mut self, shape: Value) {
        self.pages.last_mut().unwrap().push(shape);
    }
    fn draw_line(
        &mut self,
        paragraph: &Value,
        line: &str,
        x: f64,
        top: f64,
        width: f64,
    ) -> Result<(), String> {
        let style = &paragraph["style"];
        let measured = (self.measure)(line, style)?;
        let available = width - number(paragraph, "left")? - number(paragraph, "right")?;
        let mut left = x + number(paragraph, "left")?;
        let align = string(paragraph, "align")?;
        if align == "center" {
            left += (available - measured) / 2.0;
        }
        if align == "right" {
            left += available - measured;
        }
        self.add(json!({"kind":"text","text":line,"x":left,"y":top + number(style,"size")? * 0.85,"style":style}));
        Ok(())
    }
    fn paragraph(&mut self, paragraph: &Value, x: f64, width: f64) -> Result<(), String> {
        let lines = self.lines(paragraph, width)?;
        let line_height = number(&paragraph["style"], "size")? * number(paragraph, "lineSpacing")?;
        let before = number(paragraph, "before")?;
        if before + line_height > self.bottom - self.top {
            return Err("문단 한 줄과 여백이 본문 높이를 넘습니다.".into());
        }
        if self.y + before + line_height > self.bottom {
            self.next()?;
        }
        self.y += before;
        for line in &lines {
            if self.y + line_height > self.bottom {
                self.next()?;
            }
            self.draw_line(paragraph, line, x, self.y, width)?;
            self.y += line_height;
        }
        self.y += number(paragraph, "after")?;
        Ok(())
    }
    fn rows<'a>(&mut self, table: &'a Value) -> Result<Vec<RowLayout<'a>>, String> {
        let count = number(table, "rows")?;
        if count.fract() != 0.0 || !(1.0..=1000.0).contains(&count) {
            return Err(INVALID.into());
        }
        let source_cells = array(table, "cells")?;
        let mut rows = Vec::new();
        for row in 0..count as usize {
            let mut source = Vec::new();
            for cell in source_cells {
                if number(cell, "r")? == row as f64 {
                    source.push((number(cell, "c")?, cell));
                }
            }
            source.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut cells = Vec::new();
            let mut height = f64::NEG_INFINITY;
            for (_, cell) in source {
                let padding = &cell["padding"];
                let mut cy = number(padding, "top")?;
                let mut paragraphs = Vec::new();
                for paragraph in array(cell, "paragraphs")? {
                    let lines = self.lines(
                        paragraph,
                        number(cell, "width")?
                            - number(padding, "left")?
                            - number(padding, "right")?,
                    )?;
                    let at = cy + number(paragraph, "before")?;
                    // Preserve the source's grouped multiplication and addition.
                    cy = at
                        + lines.len() as f64
                            * number(&paragraph["style"], "size")?
                            * number(paragraph, "lineSpacing")?
                        + number(paragraph, "after")?;
                    paragraphs.push(ParagraphLayout {
                        paragraph,
                        lines,
                        at,
                    });
                }
                let content_height = cy + number(padding, "bottom")?;
                height = height.max(number(cell, "minHeight")?.max(content_height));
                cells.push(CellLayout {
                    cell,
                    paragraphs,
                    content_height,
                });
            }
            rows.push(RowLayout { cells, height });
        }
        Ok(rows)
    }
    fn draw_row(
        &mut self,
        row: &RowLayout<'_>,
        page_left: f64,
        table_left: f64,
    ) -> Result<(), String> {
        let mut x = page_left + table_left;
        for entry in &row.cells {
            let cell = entry.cell;
            let width = number(cell, "width")?;
            self.add(json!({"kind":"rect","x":x,"y":self.y,"width":width,"height":row.height,"fill":cell["fill"],"borders":cell["borders"]}));
            let extra = match string(cell, "vertical")? {
                "center" => (row.height - entry.content_height) / 2.0,
                "bottom" => row.height - entry.content_height,
                _ => 0.0,
            };
            let padding = &cell["padding"];
            for paragraph in &entry.paragraphs {
                let mut yy = self.y + paragraph.at + extra;
                for line in &paragraph.lines {
                    self.draw_line(
                        paragraph.paragraph,
                        line,
                        x + number(padding, "left")?,
                        yy,
                        width - number(padding, "left")? - number(padding, "right")?,
                    )?;
                    yy += number(&paragraph.paragraph["style"], "size")?
                        * number(paragraph.paragraph, "lineSpacing")?;
                }
            }
            x += width;
        }
        self.y += row.height;
        Ok(())
    }
}

/// The caller verifies fonts and returns actual glyph widths in points, matching
/// browser measureText(text).width * 3 / 4. No approximate fallback is supplied.
pub fn layout_model(
    model: &Value,
    measure: impl FnMut(&str, &Value) -> Result<f64, String>,
) -> Result<Value, String> {
    let page = &model["page"];
    let page_left = number(page, "left")?;
    let width = number(page, "width")? - page_left - number(page, "right")?;
    let top = number(page, "top")?;
    let bottom = number(page, "height")? - number(page, "bottom")?;
    let mut layout = Layout {
        measure,
        pages: vec![Vec::new()],
        y: top,
        top,
        bottom,
    };
    let mut warnings = vec![
        "자체 조판 미리보기입니다. 실제 한글의 줄바꿈·쪽 나눔과 다를 수 있습니다.".to_string(),
    ];
    let mut fonts = Vec::new();
    let mut seen_fonts = HashSet::new();
    let blocks = array(model, "blocks")?;
    for block in blocks {
        let paragraphs: Vec<&Value> = if block["type"] == "paragraph" {
            vec![block]
        } else {
            let mut paragraphs = Vec::new();
            for cell in array(block, "cells")? {
                paragraphs.extend(array(cell, "paragraphs")?);
            }
            paragraphs
        };
        for paragraph in paragraphs {
            let font = string(&paragraph["style"], "font")?;
            if seen_fonts.insert(font.to_owned()) {
                fonts.push(font.to_owned());
            }
            let notice = "양쪽 정렬은 왼쪽 정렬로 미리보기합니다.";
            if paragraph["align"] == "justify" && !warnings.iter().any(|s| s == notice) {
                warnings.push(notice.into());
            }
        }
    }
    for block in blocks {
        if block["pageBreak"] == true && !layout.pages.last().unwrap().is_empty() {
            layout.next()?;
        }
        if block["type"] == "paragraph" {
            layout.paragraph(block, page_left, width)?;
            continue;
        }
        let mut table_width = 0.0;
        for value in array(block, "widths")? {
            table_width += value.as_f64().ok_or(INVALID)?;
        }
        let table_left = number(block, "left")?;
        if table_width + table_left > width + 0.1 {
            return Err("표가 본문 너비를 넘습니다.".into());
        }
        layout.y += number(block, "before")?;
        let rows = layout.rows(block)?;
        let header = block["repeatHeader"] == true
            && rows[0]
                .cells
                .iter()
                .all(|entry| entry.cell["header"] == true);
        for (r, row) in rows.iter().enumerate() {
            if row.height > bottom - top {
                return Err("한 쪽보다 높은 표 행은 자체 렌더에서 지원하지 않습니다.".into());
            }
            if r == 0
                && header
                && rows
                    .get(1)
                    .is_some_and(|next| layout.y + row.height + next.height > bottom)
                && layout.y > top
            {
                layout.next()?;
            }
            if layout.y + row.height > bottom {
                layout.next()?;
                if r > 0 && header {
                    if rows[0].height + row.height > bottom - top {
                        return Err("반복 머리행과 본문 행이 한 쪽에 들어가지 않습니다.".into());
                    }
                    layout.draw_row(&rows[0], page_left, table_left)?;
                }
            }
            layout.draw_row(row, page_left, table_left)?;
        }
        layout.y += number(block, "after")?;
    }
    Ok(json!({"pages":layout.pages,"warnings":warnings,"fonts":fonts}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn paragraph(text: &str) -> Value {
        json!({"type":"paragraph","text":text,"style":{"font":"가상 글꼴","size":10,"bold":false,"color":"#000000"},"align":"left","lineSpacing":1,"left":0,"right":0,"before":0,"after":0,"pageBreak":false})
    }
    fn model(blocks: Vec<Value>, width: f64, height: f64) -> Value {
        json!({"page":{"width":width,"height":height,"left":0,"right":0,"top":0,"bottom":0},"blocks":blocks})
    }
    fn measure(text: &str, style: &Value) -> Result<f64, String> {
        Ok(text.chars().count() as f64 * style["size"].as_f64().unwrap())
    }
    fn texts(value: &Value, page: usize) -> Vec<&str> {
        value["pages"][page]
            .as_array()
            .unwrap()
            .iter()
            .filter(|shape| shape["kind"] == "text")
            .map(|shape| shape["text"].as_str().unwrap())
            .collect()
    }
    fn table(rows: usize) -> Value {
        let mut cells = Vec::new();
        for r in 0..rows {
            for c in 0..2 {
                cells.push(json!({"r":r,"c":c,"width":50,"minHeight":20,"padding":{"left":2,"right":2,"top":2,"bottom":2},"fill":"#ffffff","borders":{},"header":r==0,"vertical":if c==0 {"center"} else {"bottom"},"paragraphs":[paragraph(if r==0 {"머리"} else {"본문"})]}));
            }
        }
        json!({"type":"table","rows":rows,"cols":2,"cells":cells,"widths":[50,50],"repeatHeader":true,"before":0,"after":0,"left":0,"pageBreak":false})
    }
    #[test]
    fn word_scalar_and_explicit_breaks_keep_original_measure_order() {
        let mut calls = Vec::new();
        let result = layout_model(
            &model(
                vec![paragraph("ABCDEF GHIJKL\nABCDEFGHIJKL\n")],
                100.0,
                100.0,
            ),
            |text, style| {
                calls.push(text.to_string());
                measure(text, style)
            },
        )
        .unwrap();
        assert_eq!(
            texts(&result, 0),
            ["ABCDEF ", "GHIJKL", "ABCDEFGHIJ", "KL", ""]
        );
        assert_eq!(&calls[..4], ["ABCDEF", "ABCDEF ", "ABCDEF GHIJKL", "G"]);
        assert_eq!(result["pages"][0][0]["y"].as_f64(), Some(8.5));
        let result =
            layout_model(&model(vec![paragraph("가😀나다")], 30.0, 100.0), measure).unwrap();
        assert_eq!(texts(&result, 0), ["가😀나", "다"]);
    }
    #[test]
    fn alignment_warnings_fonts_and_explicit_pages_are_preserved() {
        let mut a = paragraph("A");
        a["align"] = json!("center");
        a["pageBreak"] = json!(true);
        let mut b = paragraph("B");
        b["align"] = json!("right");
        b["pageBreak"] = json!(true);
        b["style"]["font"] = json!("다른 가상 글꼴");
        let mut c = paragraph("C");
        c["align"] = json!("justify");
        let result = layout_model(&model(vec![a, b, c.clone(), c], 100.0, 100.0), measure).unwrap();
        assert_eq!(result["pages"].as_array().unwrap().len(), 2);
        assert_eq!(result["pages"][0][0]["x"].as_f64(), Some(45.0));
        assert_eq!(result["pages"][1][0]["x"].as_f64(), Some(90.0));
        assert_eq!(result["warnings"].as_array().unwrap().len(), 2);
        assert_eq!(result["fonts"], json!(["가상 글꼴", "다른 가상 글꼴"]));
    }
    #[test]
    fn table_headers_repeat_and_vertical_padding_uses_full_row_height() {
        let result = layout_model(
            &model(vec![paragraph("앞"), table(3)], 100.0, 50.0),
            measure,
        )
        .unwrap();
        assert_eq!(result["pages"].as_array().unwrap().len(), 2);
        assert_eq!(texts(&result, 0), ["앞", "머리", "머리", "본문", "본문"]);
        assert_eq!(texts(&result, 1), ["머리", "머리", "본문", "본문"]);
        let first = &result["pages"][0];
        assert_eq!(first[1]["y"].as_f64(), Some(10.0));
        assert_eq!(first[2]["y"].as_f64(), Some(23.5));
        assert_eq!(first[4]["y"].as_f64(), Some(26.5));
        let mut lead = paragraph("앞");
        lead["after"] = json!(10);
        let result = layout_model(&model(vec![lead, table(2)], 100.0, 50.0), measure).unwrap();
        assert_eq!(texts(&result, 0), ["앞"]);
        assert_eq!(texts(&result, 1), ["머리", "머리", "본문", "본문"]);
    }
    #[test]
    fn limits_and_measurement_failures_are_not_hidden() {
        assert!(
            layout_model(&model(vec![paragraph("x")], 9.0, 100.0), measure)
                .unwrap_err()
                .contains("글자보다")
        );
        let mut tall = paragraph("x");
        tall["before"] = json!(91);
        assert!(
            layout_model(&model(vec![tall], 100.0, 100.0), measure)
                .unwrap_err()
                .contains("문단 한 줄")
        );
        assert_eq!(
            layout_model(&model(vec![paragraph("x")], 100.0, 100.0), |_, _| Err(
                "가상 측정 실패".into()
            ))
            .unwrap_err(),
            "가상 측정 실패"
        );
        let mut wide = table(1);
        wide["left"] = json!(1);
        assert!(
            layout_model(&model(vec![wide], 100.0, 100.0), measure)
                .unwrap_err()
                .contains("표가 본문 너비")
        );
        let mut tall = table(1);
        for cell in tall["cells"].as_array_mut().unwrap() {
            cell["minHeight"] = json!(51);
        }
        assert!(
            layout_model(&model(vec![tall], 100.0, 50.0), measure)
                .unwrap_err()
                .contains("한 쪽보다 높은")
        );
        let mut oversized = table(2);
        for cell in oversized["cells"].as_array_mut().unwrap() {
            cell["minHeight"] = json!(30);
        }
        assert!(
            layout_model(&model(vec![oversized], 100.0, 50.0), measure)
                .unwrap_err()
                .contains("반복 머리행")
        );
        let mut q = paragraph("x");
        q["pageBreak"] = json!(true);
        assert!(
            layout_model(&model(vec![q; 501], 100.0, 100.0), measure)
                .unwrap_err()
                .contains("500쪽")
        );
    }
}
