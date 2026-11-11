//! Read-only HWPX model for the existing limited preview renderer.
//! Local-name selection intentionally follows the original renderer, while
//! paragraph text still uses the HWP paragraph namespace through ParsedXml.
use crate::{
    hwpx_document::{load_hwpx, sha256},
    input::trim,
    office_xml::{ParsedXml, parse_office_xml},
};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{collections::HashSet, sync::LazyLock};

static DECIMAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\z").unwrap()
});
static BORDER_WIDTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]+)? mm\z").unwrap());

fn reject(reason: &str) -> String {
    format!("자체 렌더 미지원: {reason}. 실제 한글에서 확인하세요.")
}
fn finite(value: f64, minimum: f64, maximum: f64, label: &str) -> Result<f64, String> {
    if value.is_finite() && value >= minimum && value <= maximum {
        Ok(value)
    } else {
        Err(reject(&format!("잘못된 {label} 수치")))
    }
}
fn truthy(number: f64) -> bool {
    number != 0.0 && !number.is_nan()
}
fn integer(number: f64) -> bool {
    number.is_finite() && number.fract() == 0.0
}
fn color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

/// JavaScript Number(string), including empty/whitespace, radix literals and
/// NaN. Invalid numeric attributes are sometimes intentionally only tested for
/// truthiness by the original renderer, so NaN must not become a truthy error.
fn number(value: &str) -> f64 {
    let value = trim(value);
    if value.is_empty() {
        return 0.0;
    }
    match value {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefixes, radix) in [(["0x", "0X"], 16_u32), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        if let Some(digits) = value
            .strip_prefix(prefixes[0])
            .or_else(|| value.strip_prefix(prefixes[1]))
        {
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii() && c.is_digit(radix)) {
                return f64::NAN;
            }
            // All accepted layout values are small enough for this exact path.
            if let Ok(value) = u128::from_str_radix(digits, radix) {
                return value as f64;
            }
            // Larger values exceed every finite layout bound, but retain the
            // same nonzero/Infinity truthiness as JavaScript Number.
            return digits.chars().fold(0.0, |n, c| {
                n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap())
            });
        }
    }
    if !DECIMAL.is_match(value) {
        return f64::NAN;
    }
    value.parse().unwrap_or(f64::NAN)
}
fn child(doc: &ParsedXml, node: Option<usize>, local: &str) -> Option<usize> {
    node.and_then(|node| {
        doc.nodes[node]
            .children
            .iter()
            .copied()
            .find(|i| doc.nodes[*i].local == local)
    })
}
fn descendants(doc: &ParsedXml, node: usize, local: &str) -> Vec<usize> {
    let mut pending: Vec<_> = doc.nodes[node].children.iter().rev().copied().collect();
    let mut result = Vec::new();
    while let Some(index) = pending.pop() {
        if doc.nodes[index].local == local {
            result.push(index);
        }
        pending.extend(doc.nodes[index].children.iter().rev().copied());
    }
    result
}
fn attr<'a>(doc: &'a ParsedXml, node: Option<usize>, name: &str) -> Option<&'a str> {
    node.and_then(|index| doc.nodes[index].attr(name))
}
fn val(doc: &ParsedXml, node: Option<usize>, name: &str, otherwise: f64) -> f64 {
    attr(doc, node, name).map_or(otherwise, number)
}

struct Reader<'a> {
    header: &'a ParsedXml,
    section: &'a ParsedXml,
    table_count: usize,
}
impl Reader<'_> {
    fn resolve(&self, local: &str, id: Option<&str>) -> Result<usize, String> {
        self.header
            .nodes
            .iter()
            .position(|node| node.local == local && node.attr("id") == id)
            .ok_or_else(|| reject(&format!("{local} 서식 참조")))
    }
    fn char_style(&self, id: Option<&str>) -> Result<Value, String> {
        let h = self.header;
        let c = self.resolve("charPr", id)?;
        let font_ref = child(h, Some(c), "fontRef");
        if font_ref.is_none_or(|index| h.nodes[index].attrs.is_empty()) {
            return Err(reject("글꼴 참조 없음"));
        }
        let mut fonts = Vec::new();
        for (index, face) in h
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.local == "fontface")
        {
            if child(h, Some(index), "font").is_none() {
                return Err(reject("언어별 다른 글꼴"));
            }
            let language = match face.attr("lang") {
                Some("HANGUL") => "hangul",
                Some("LATIN") => "latin",
                Some("HANJA") => "hanja",
                Some("JAPANESE") => "japanese",
                Some("OTHER") => "other",
                Some("SYMBOL") => "symbol",
                Some("USER") => "user",
                _ => "undefined",
            };
            let wanted = attr(h, font_ref, language);
            let font = face
                .children
                .iter()
                .find(|i| h.nodes[**i].local == "font" && h.nodes[**i].attr("id") == wanted)
                .and_then(|i| h.nodes[*i].attr("face"));
            let font = font
                .filter(|name| !name.is_empty())
                .ok_or_else(|| reject("언어별 다른 글꼴"))?;
            fonts.push(font);
        }
        if fonts.iter().collect::<HashSet<_>>().len() != 1 {
            return Err(reject("언어별 다른 글꼴"));
        }
        for (name, expected) in [
            ("ratio", "100"),
            ("spacing", "0"),
            ("relSz", "100"),
            ("offset", "0"),
        ] {
            if child(h, Some(c), name).is_some_and(|index| {
                h.nodes[index]
                    .attrs
                    .values()
                    .any(|v| v.as_str() != Some(expected))
            }) {
                return Err(reject("장평·자간·글자 위치 조정"));
            }
        }
        if ["italic", "supscript", "subscript", "emboss", "engrave"]
            .iter()
            .any(|name| child(h, Some(c), name).is_some())
            || attr(h, child(h, Some(c), "outline"), "type") != Some("NONE")
            || attr(h, child(h, Some(c), "shadow"), "type") != Some("NONE")
        {
            return Err(reject("기울임·첨자·장식 글자"));
        }
        if attr(h, child(h, Some(c), "underline"), "type") != Some("NONE")
            || attr(h, child(h, Some(c), "strikeout"), "shape") != Some("NONE")
        {
            return Err(reject("밑줄·취소선"));
        }
        let text_color = h.nodes[c].attr("textColor").unwrap_or("#000000");
        if !color(text_color) {
            return Err(reject("글자 색"));
        }
        if h.nodes[c]
            .attr("shadeColor")
            .is_some_and(|s| !s.is_empty() && s != "none")
        {
            return Err(reject("글자 음영"));
        }
        Ok(
            json!({"font":fonts[0],"size":finite(val(h,Some(c),"height",0.0)/100.0,1.0,200.0,"글자 크기")?,"bold":child(h,Some(c),"bold").is_some(),"color":text_color}),
        )
    }
    fn paragraph(&mut self, index: usize) -> Result<Value, String> {
        let section = self.section;
        let h = self.header;
        let node = &section.nodes[index];
        if node
            .children
            .iter()
            .any(|i| !["run", "linesegarray"].contains(&section.nodes[*i].local.as_str()))
        {
            return Err(reject("문단의 미지원 요소"));
        }
        let pp = self.resolve("paraPr", node.attr("paraPrIDRef"))?;
        let align = attr(h, child(h, Some(pp), "align"), "horizontal").unwrap_or("LEFT");
        if !["LEFT", "CENTER", "RIGHT", "JUSTIFY"].contains(&align) {
            return Err(reject("문단 정렬"));
        }
        let heading = child(h, Some(pp), "heading");
        if heading.is_some() && attr(h, heading, "type") != Some("NONE") {
            return Err(reject("자동 번호 문단"));
        }
        if truthy(val(h, Some(pp), "condense", 0.0)) {
            return Err(reject("문단 압축"));
        }
        let margin = descendants(h, pp, "margin").first().copied();
        let spacing = descendants(h, pp, "lineSpacing").first().copied();
        if spacing.is_none() || attr(h, spacing, "type") != Some("PERCENT") {
            return Err(reject("고정 줄 간격"));
        }
        let ma = |key: &str| {
            finite(
                val(h, child(h, margin, key), "value", 0.0) / 100.0,
                0.0,
                2000.0,
                "문단 여백",
            )
        };
        if ma("intent")? != 0.0 {
            return Err(reject("첫 줄 들여쓰기"));
        }
        let mut styles = Vec::new();
        let mut tables = Vec::new();
        for run in node
            .children
            .iter()
            .filter(|i| section.nodes[**i].local == "run")
        {
            for element in &section.nodes[*run].children {
                match section.nodes[*element].local.as_str() {
                    "t" => {
                        if !descendants(section, *element, "tab").is_empty()
                            || section.nodes[*element]
                                .children
                                .iter()
                                .any(|i| section.nodes[*i].local != "lineBreak")
                        {
                            return Err(reject("탭·본문 제어 문자"));
                        }
                        styles.push(self.char_style(section.nodes[*run].attr("charPrIDRef"))?);
                    }
                    "tbl" => tables.push(self.table(*element)?),
                    "ctrl" => {
                        if section.nodes[*element]
                            .children
                            .iter()
                            .any(|i| section.nodes[*i].local != "colPr")
                        {
                            return Err(reject("머리말·꼬리말·쪽번호·각주 등 제어 요소"));
                        }
                    }
                    "secPr" => {}
                    local => return Err(reject(&format!("문단 {local} 요소"))),
                }
            }
        }
        if styles.iter().any(|style| Some(style) != styles.first()) {
            return Err(reject("한 문단 안의 혼합 글자 서식"));
        }
        let content = section.paragraph_text(index);
        if !tables.is_empty() {
            if tables.len() != 1 || !trim(&content).is_empty() {
                return Err(reject("표와 글이 섞인 문단"));
            }
            let mut table = tables.remove(0);
            table["pageBreak"] = json!(node.attr("pageBreak") == Some("1"));
            return Ok(table);
        }
        if truthy(val(section, Some(index), "columnBreak", 0.0)) {
            return Err(reject("단 나눔"));
        }
        let bs = child(h, Some(pp), "breakSetting");
        if ["keepWithNext", "keepLines", "widowOrphan"]
            .iter()
            .any(|key| attr(h, bs, key) == Some("1"))
        {
            return Err(reject("문단 묶음·외톨이줄 보호"));
        }
        if let Some(pb) = child(h, Some(pp), "border") {
            let bf = self.resolve("borderFill", h.nodes[pb].attr("borderFillIDRef"))?;
            if h.nodes[bf].children.iter().any(|i| {
                h.nodes[*i].local.ends_with("Border")
                    && h.nodes[*i].attr("type") != Some("NONE")
                    && h.nodes[*i].local != "diagonal"
            }) || descendants(h, bf, "winBrush")
                .iter()
                .any(|i| h.nodes[*i].attr("faceColor") != Some("none"))
            {
                return Err(reject("문단 테두리·음영"));
            }
        }
        let style = if let Some(style) = styles.first() {
            style.clone()
        } else {
            let first_run = node
                .children
                .iter()
                .find(|i| section.nodes[**i].local == "run")
                .copied();
            self.char_style(Some(attr(section, first_run, "charPrIDRef").unwrap_or("0")))?
        };
        Ok(
            json!({"type":"paragraph","text":content,"style":style,"align":align.to_lowercase(),"lineSpacing":finite(val(h,spacing,"value",160.0)/100.0,1.0,5.0,"줄 간격")?,"left":ma("left")?,"right":ma("right")?,"before":ma("prev")?,"after":ma("next")?,"pageBreak":node.attr("pageBreak")==Some("1") || attr(h,bs,"pageBreakBefore")==Some("1")}),
        )
    }
    fn table(&mut self, index: usize) -> Result<Value, String> {
        self.table_count += 1;
        let table_number = self.table_count;
        let section = self.section;
        let h = self.header;
        let node = &section.nodes[index];
        if node.children.iter().any(|i| {
            !["sz", "pos", "outMargin", "inMargin", "tr"]
                .contains(&section.nodes[*i].local.as_str())
        }) {
            return Err(reject("표 캡션 등 미지원 표 요소"));
        }
        if !descendants(section, index, "tbl").is_empty() {
            return Err(reject("중첩표"));
        }
        let pos = child(section, Some(index), "pos");
        if pos.is_none()
            || [
                ("vertRelTo", "PARA"),
                ("horzRelTo", "COLUMN"),
                ("horzAlign", "LEFT"),
                ("vertAlign", "TOP"),
                ("treatAsChar", "0"),
                ("flowWithText", "1"),
            ]
            .iter()
            .any(|(k, v)| attr(section, pos, k) != Some(*v))
            || truthy(val(section, pos, "vertOffset", 0.0))
            || truthy(val(section, pos, "horzOffset", 0.0))
            || attr(section, pos, "allowOverlap") == Some("1")
            || node.attr("textWrap") != Some("TOP_AND_BOTTOM")
        {
            return Err(reject("떠 있는 표의 위치·글 감싸기"));
        }
        if node.attr("pageBreak") == Some("NONE") {
            return Err(reject("쪽 나눔 금지 표"));
        }
        let rows = val(section, Some(index), "rowCnt", 0.0);
        let cols = val(section, Some(index), "colCnt", 0.0);
        if !integer(rows)
            || !integer(cols)
            || !(1.0..=1000.0).contains(&rows)
            || !(1.0..=32.0).contains(&cols)
        {
            return Err(reject("표 크기"));
        }
        if val(section, Some(index), "cellSpacing", 0.0) != 0.0 {
            return Err(reject("셀 간격"));
        }
        let mut cells = Vec::new();
        let mut widths: Vec<Option<f64>> = vec![None; cols as usize];
        let mut positions = HashSet::new();
        let mut row = 0_usize;
        for tr in node
            .children
            .iter()
            .filter(|i| section.nodes[**i].local == "tr")
        {
            if section.nodes[*tr]
                .children
                .iter()
                .any(|i| section.nodes[*i].local != "tc")
            {
                return Err(reject("미지원 표 행 요소"));
            }
            for tc in section.nodes[*tr]
                .children
                .iter()
                .filter(|i| section.nodes[**i].local == "tc")
            {
                let cell = &section.nodes[*tc];
                if cell.children.iter().any(|i| {
                    !["subList", "cellAddr", "cellSpan", "cellSz", "cellMargin"]
                        .contains(&section.nodes[*i].local.as_str())
                }) {
                    return Err(reject("미지원 표 셀 요소"));
                }
                let address = child(section, Some(*tc), "cellAddr");
                let span = child(section, Some(*tc), "cellSpan");
                let sz = child(section, Some(*tc), "cellSz");
                let sub = child(section, Some(*tc), "subList");
                let r = val(section, address, "rowAddr", 0.0);
                let c = val(section, address, "colAddr", 0.0);
                let rs = val(section, span, "rowSpan", 1.0);
                let cs = val(section, span, "colSpan", 1.0);
                if ![r, c, rs, cs].into_iter().all(integer)
                    || r < 0.0
                    || c < 0.0
                    || r != row as f64
                    || r + rs > rows
                    || c + cs > cols
                    || rs < 1.0
                    || cs < 1.0
                {
                    return Err(reject("잘못된 병합 셀"));
                }
                if rs != 1.0 || cs != 1.0 {
                    return Err(reject("병합 표 — 첫 버전은 직사각형 셀만 렌더"));
                }
                if sub.is_none()
                    || attr(section, sub, "textDirection") != Some("HORIZONTAL")
                    || !attr(section, sub, "vertAlign")
                        .is_some_and(|v| ["TOP", "CENTER", "BOTTOM"].contains(&v))
                {
                    return Err(reject("셀 글 방향·정렬"));
                }
                let location = format!(
                    "표 {table_number}, {}행 {}열",
                    r as usize + 1,
                    c as usize + 1
                );
                let line_wrap = attr(section, sub, "lineWrap").ok_or_else(|| {
                    reject(&format!(
                        "{location}의 lineWrap 생략 — 줄바꿈 설정을 확인할 수 없음"
                    ))
                })?;
                if line_wrap != "BREAK" {
                    return Err(reject(&format!(
                        "{location}의 셀 줄바꿈(lineWrap={}) — BREAK만 지원",
                        serde_json::to_string(line_wrap).unwrap()
                    )));
                }
                let sub = sub.unwrap();
                if section.nodes[sub]
                    .children
                    .iter()
                    .any(|i| section.nodes[*i].local != "p")
                {
                    return Err(reject("미지원 셀 본문 요소"));
                }
                let cm = if cell.attr("hasMargin") == Some("1") {
                    child(section, Some(*tc), "cellMargin")
                } else {
                    child(section, Some(index), "inMargin")
                };
                let fill = self.resolve("borderFill", cell.attr("borderFillIDRef"))?;
                let brush = descendants(h, fill, "winBrush").first().copied();
                let fill_color = attr(h, brush, "faceColor").unwrap_or("none");
                if fill_color != "none" && !color(fill_color) {
                    return Err(reject("셀 배경"));
                }
                if !descendants(h, fill, "imgBrush").is_empty()
                    || !descendants(h, fill, "gradation").is_empty()
                {
                    return Err(reject("그림·그라데이션 배경"));
                }
                let mut borders = Map::new();
                for side in ["left", "right", "top", "bottom"] {
                    let border = child(h, Some(fill), &format!("{side}Border"));
                    let kind = attr(h, border, "type");
                    if border.is_some()
                        && !kind.is_some_and(|kind| ["NONE", "SOLID"].contains(&kind))
                    {
                        return Err(reject("표의 특수 테두리"));
                    }
                    let border_color = attr(h, border, "color").unwrap_or("#000000");
                    if !color(border_color) {
                        return Err(reject("테두리 색"));
                    }
                    let border_width = attr(h, border, "width");
                    if kind == Some("SOLID")
                        && !border_width.is_some_and(|s| BORDER_WIDTH.is_match(s))
                    {
                        return Err(reject("테두리 두께 단위"));
                    }
                    let width = if kind == Some("SOLID") {
                        finite(
                            number(border_width.unwrap().strip_suffix(" mm").unwrap()) * 72.0
                                / 25.4,
                            0.0,
                            10.0,
                            "테두리 두께",
                        )?
                    } else {
                        0.0
                    };
                    borders.insert(side.into(), json!({"color":border_color,"width":width}));
                }
                let width = finite(
                    val(section, sz, "width", 0.0) / 100.0,
                    5.0,
                    2000.0,
                    "셀 너비",
                )?;
                let column = c as usize;
                if widths[column].is_some_and(|previous| (previous - width).abs() > 0.02) {
                    return Err(reject("행마다 다른 열 너비"));
                }
                widths[column] = Some(width);
                let paragraphs: Vec<Value> = section.nodes[sub]
                    .children
                    .iter()
                    .filter(|i| section.nodes[**i].local == "p")
                    .map(|i| self.paragraph(*i))
                    .collect::<Result<_, _>>()?;
                if paragraphs.iter().any(|p| p["pageBreak"] == true) {
                    return Err(reject("셀 내부 쪽 나눔"));
                }
                let min_height = finite(
                    val(section, sz, "height", 0.0) / 100.0,
                    0.0,
                    10000.0,
                    "셀 높이",
                )?;
                let mut padding = Map::new();
                for side in ["left", "right", "top", "bottom"] {
                    padding.insert(
                        side.into(),
                        json!(finite(
                            val(section, cm, side, 0.0) / 100.0,
                            0.0,
                            1000.0,
                            "셀 여백"
                        )?),
                    );
                }
                positions.insert((r as usize, c as usize));
                cells.push(json!({"r":r,"c":c,"width":width,"minHeight":min_height,"padding":padding,"fill":if fill_color=="none" {"#ffffff"} else {fill_color},"borders":borders,"header":cell.attr("header")==Some("1"),"vertical":attr(section,Some(sub),"vertAlign").unwrap().to_lowercase(),"paragraphs":paragraphs}));
            }
            row += 1;
        }
        if row as f64 != rows
            || cells.len() as f64 != rows * cols
            || positions.len() != cells.len()
            || widths.iter().any(|w| w.is_none_or(|w| w < 5.0))
        {
            return Err(reject("빠진 표 셀"));
        }
        let out = child(section, Some(index), "outMargin");
        Ok(
            json!({"type":"table","rows":rows,"cols":cols,"cells":cells,"widths":widths,"repeatHeader":node.attr("repeatHeader")==Some("1"),"before":finite(val(section,out,"top",0.0)/100.0,0.0,1000.0,"표 위 여백")?,"after":finite(val(section,out,"bottom",0.0)/100.0,0.0,1000.0,"표 아래 여백")?,"left":finite(val(section,out,"left",0.0)/100.0,0.0,1000.0,"표 왼쪽 여백")?}),
        )
    }
}

pub fn read_render_model(bytes: &[u8]) -> Result<Value, String> {
    let doc = load_hwpx(bytes)?;
    let sections = doc.structure["sections"].as_array().unwrap();
    if sections.len() != 1 {
        return Err(reject("여러 구역"));
    }
    let header_bytes = doc
        .archive
        .read("Contents/header.xml")
        .ok_or("Cannot read properties of null (reading 'toString')")?;
    let header = parse_office_xml(std::str::from_utf8(header_bytes).map_err(|e| e.to_string())?)?;
    let name = sections[0]["name"].as_str().unwrap();
    let section_bytes = doc
        .archive
        .read(name)
        .ok_or("Cannot read properties of null (reading 'toString')")?;
    let section = parse_office_xml(std::str::from_utf8(section_bytes).map_err(|e| e.to_string())?)?;
    let sec = section.nodes.iter().position(|node| node.local == "secPr");
    let page = child(&section, sec, "pagePr");
    let margin = child(&section, page, "margin");
    if sec.is_none() || page.is_none() || margin.is_none() {
        return Err(reject("쪽 설정 없음"));
    }
    if attr(&section, sec, "textDirection") != Some("HORIZONTAL")
        || truthy(val(&section, sec, "masterPageCnt", 0.0))
        || truthy(val(&section, margin, "gutter", 0.0))
    {
        return Err(reject("세로쓰기·바탕쪽·제본 여백"));
    }
    if section
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.local == "colPr")
        .any(|(i, _)| val(&section, Some(i), "colCount", 1.0) != 1.0)
    {
        return Err(reject("다단"));
    }
    let width = val(&section, page, "width", 0.0) / 100.0;
    let height = val(&section, page, "height", 0.0) / 100.0;
    let left = val(&section, margin, "left", 0.0) / 100.0;
    let right = val(&section, margin, "right", 0.0) / 100.0;
    let top = (val(&section, margin, "top", 0.0) + val(&section, margin, "header", 0.0)) / 100.0;
    let bottom =
        (val(&section, margin, "bottom", 0.0) + val(&section, margin, "footer", 0.0)) / 100.0;
    for (name, value) in [
        ("width", width),
        ("height", height),
        ("left", left),
        ("right", right),
        ("top", top),
        ("bottom", bottom),
    ] {
        finite(value, 0.0, 2000.0, name)?;
    }
    if width - left - right < 40.0 || height - top - bottom < 40.0 {
        return Err(reject("잘못된 쪽 크기"));
    }
    let page =
        json!({"width":width,"height":height,"left":left,"right":right,"top":top,"bottom":bottom});
    if section.nodes[section.root]
        .children
        .iter()
        .any(|i| section.nodes[*i].local != "p")
    {
        return Err(reject("구역의 미지원 요소"));
    }
    let mut reader = Reader {
        header: &header,
        section: &section,
        table_count: 0,
    };
    let blocks: Vec<Value> = section.nodes[section.root]
        .children
        .iter()
        .filter(|i| section.nodes[**i].local == "p")
        .map(|i| reader.paragraph(*i))
        .collect::<Result<_, _>>()?;
    if blocks.is_empty() {
        return Err(reject("본문 없음"));
    }
    Ok(json!({"page":page,"blocks":blocks,"sourceSha256":sha256(bytes)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hwpx_compose::compose_hwpx,
        office_zip::{patch_zip, read_zip_strict},
    };
    fn sample() -> Vec<u8> {
        compose_hwpx(&json!({"schemaVersion":1,"blocks":[{"type":"heading","text":"가상 제목","level":2},{"type":"paragraph","text":"본문\n다음"},{"type":"pageBreak"},{"type":"table","rows":[["제목","값"],["빈칸",""]]}]})).unwrap().bytes
    }
    fn replace(bytes: &[u8], entry: &str, from: &str, to: &str) -> Vec<u8> {
        let archive = read_zip_strict(bytes).unwrap();
        let text = std::str::from_utf8(archive.read(entry).unwrap()).unwrap();
        assert!(text.contains(from));
        patch_zip(
            &archive,
            &[(entry.into(), text.replacen(from, to, 1).into_bytes())],
        )
        .unwrap()
    }
    #[test]
    fn generated_document_retains_page_styles_tables_and_hash() {
        let bytes = sample();
        let model = read_render_model(&bytes).unwrap();
        assert_eq!(model["sourceSha256"], sha256(&bytes));
        assert_eq!(model["blocks"].as_array().unwrap().len(), 3);
        assert_eq!(model["blocks"][0]["style"]["size"].as_f64(), Some(16.0));
        assert_eq!(model["blocks"][0]["style"]["bold"], true);
        assert_eq!(model["blocks"][1]["text"], "본문\n다음");
        let table = &model["blocks"][2];
        assert_eq!(table["pageBreak"], true);
        assert_eq!(table["cells"].as_array().unwrap().len(), 4);
        assert_eq!(table["cells"][0]["fill"], "#EDEDED");
        assert_eq!(table["cells"][3]["fill"], "#ffffff");
        assert_eq!(table["cells"][3]["paragraphs"][0]["text"], "");
    }
    #[test]
    fn unsupported_layout_and_styles_fail_with_original_reason() {
        let bytes = sample();
        for (entry, from, to, reason) in [
            (
                "Contents/section0.xml",
                "colCount=\"1\"",
                "colCount=\"2\"",
                "다단",
            ),
            (
                "Contents/section0.xml",
                "lineWrap=\"BREAK\"",
                "lineWrap=\"SQUEEZE\"",
                "표 1, 1행 1열의 셀 줄바꿈(lineWrap=\"SQUEEZE\")",
            ),
            (
                "Contents/section0.xml",
                "lineWrap=\"BREAK\" ",
                "",
                "lineWrap 생략",
            ),
            (
                "Contents/header.xml",
                "<hh:underline type=\"NONE\"",
                "<hh:underline type=\"BOTTOM\"",
                "밑줄·취소선",
            ),
            (
                "Contents/header.xml",
                "horizontal=\"LEFT\"",
                "horizontal=\"DISTRIBUTE\"",
                "문단 정렬",
            ),
            (
                "Contents/section0.xml",
                "vertRelTo=\"PARA\"",
                "vertRelTo=\"PAPER\"",
                "떠 있는 표의 위치·글 감싸기",
            ),
        ] {
            let error = read_render_model(&replace(&bytes, entry, from, to)).unwrap_err();
            assert!(error.contains(reason), "{error}");
            assert!(error.ends_with("실제 한글에서 확인하세요."));
        }
    }
    #[test]
    fn javascript_numbers_and_nan_truthiness_are_preserved() {
        for (value, expected) in [
            ("", 0.0),
            (" \u{feff} ", 0.0),
            ("0x10", 16.0),
            ("0o10", 8.0),
            ("0b10", 2.0),
            ("+1.5e2", 150.0),
            (".5", 0.5),
            ("1.", 1.0),
        ] {
            assert_eq!(number(value), expected);
        }
        for value in ["inf", "-0x1", "0x", "1 mm", "NaN", "\u{85}"] {
            assert!(number(value).is_nan());
        }
        assert!(!truthy(number("invalid")));
        assert!(truthy(number("Infinity")));
        let bytes = sample();
        let unchanged = read_render_model(&bytes).unwrap();
        let changed = replace(
            &bytes,
            "Contents/section0.xml",
            "masterPageCnt=\"0\"",
            "masterPageCnt=\"not-a-number\"",
        );
        let mut result = read_render_model(&changed).unwrap();
        result["sourceSha256"] = unchanged["sourceSha256"].clone();
        assert_eq!(result, unchanged);
    }
    #[test]
    fn local_style_selection_and_namespace_text_remain_distinct() {
        let bytes = sample();
        let changed = replace(
            &bytes,
            "Contents/header.xml",
            "<hh:align horizontal=\"LEFT\"",
            "<hp:align horizontal=\"LEFT\"",
        );
        assert!(read_render_model(&changed).is_ok()); // local name selection, not the hh namespace.
        let changed = replace(
            &bytes,
            "Contents/section0.xml",
            "<hp:t>본문<hp:lineBreak/>다음</hp:t>",
            "<hh:t>본문<hp:lineBreak/>다음</hh:t>",
        );
        let result = read_render_model(&changed).unwrap();
        assert_eq!(result["blocks"][1]["text"], "");
    }
}
