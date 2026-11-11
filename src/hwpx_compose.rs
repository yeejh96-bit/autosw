//! New HWPX composition from a validated model or the supported Markdown subset.
use crate::{
    hwpx_document::load_hwpx,
    input::{is_space, js_string, trim},
    office_xml::{check_xml_text, xml_escape},
    office_zip::create_zip,
};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

pub const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const JS_SPACE: &str =
    r"[\x09-\x0d\x20\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]";
static DEFAULTS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/hwpx-defaults.json"))
        .expect("valid embedded HWPX defaults")
});
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(#{{1,3}}){JS_SPACE}+([^\r\n\u2028\u2029]+)\z")).unwrap()
});
static UNSUPPORTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"\*|_(?:[^_\n]+)_|__|`|!\[|\]\(|</?[A-Za-z!]|(?:\A|[\n\r\u2028\u2029]){JS_SPACE}+(?:[-+*]|[0-9]+\.){JS_SPACE}|(?:\A|[\n\r\u2028\u2029])#{{4,}}{JS_SPACE}|(?:\A|[\n\r\u2028\u2029])>{JS_SPACE}|\\\|")).unwrap()
});

pub fn namespace() -> &'static str {
    DEFAULTS["namespace"].as_str().unwrap()
}

// 전환 중 고정 원본과 메모리 결과를 대조하는 읽기 전용 진입점이다.
// 파일 공개와 승인 확인은 hwpx-create의 미리보기·저장 단계에서 처리한다.
pub fn run(operation: &str, input: &str) -> Result<Value, String> {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let model = if matches!(operation, "markdown" | "compose-markdown") {
        document_from_markdown(input)?
    } else if matches!(operation, "normalize" | "compose") {
        normalize_document(&serde_json::from_str(text).map_err(|e| e.to_string())?)?
    } else {
        return Err("normalize|markdown|compose|compose-markdown을 지정하세요.".into());
    };
    if matches!(operation, "normalize" | "markdown") {
        return Ok(model);
    }
    let composed = compose_hwpx(&model)?;
    Ok(
        json!({"model":composed.model,"bytesHex":composed.bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()}),
    )
}

fn own<'a>(value: &'a Value, keys: &[&str], label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .filter(|object| object.keys().all(|key| keys.contains(&key.as_str())))
        .ok_or_else(|| format!("{label}: 알 수 없는 필드·형식"))
}
fn nullish(value: &Value, default: Value) -> Value {
    if value.is_null() {
        default
    } else {
        value.clone()
    }
}
fn number(value: Value, minimum: f64, maximum: f64, label: &str) -> Result<Value, String> {
    if value
        .as_f64()
        .is_some_and(|n| n.is_finite() && n >= minimum && n <= maximum)
    {
        Ok(value)
    } else {
        Err(format!(
            "{label}: 허용 범위 {}~{}",
            numeric(minimum),
            numeric(maximum)
        ))
    }
}
fn text(value: &Value) -> Result<&str, String> {
    let text = value
        .as_str()
        .filter(|s| s.encode_utf16().count() <= 50000 && !s.contains(['\t', '\r']))
        .ok_or("본문은 탭·CR 없는 문자열, 항목당 50,000자 이하로 지정하세요.")?;
    check_xml_text(text)?;
    Ok(text)
}
fn take_text(value: &Value, characters: &mut usize) -> Result<Value, String> {
    let text = text(value)?;
    *characters += text.encode_utf16().count();
    Ok(json!(text))
}
fn numeric(value: f64) -> String {
    ryu_js::Buffer::new().format(value).to_string()
}
fn round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}
fn mm(value: f64) -> f64 {
    round(value * 7200.0 / 25.4)
}

pub fn normalize_document(input: &Value) -> Result<Value, String> {
    own(
        input,
        &["schemaVersion", "title", "page", "style", "blocks"],
        "문서",
    )?;
    if input["schemaVersion"].as_f64() != Some(1.0) {
        return Err("schemaVersion은 1이어야 합니다.".into());
    }
    let p = nullish(&input["page"], json!({}));
    let s = nullish(&input["style"], json!({}));
    own(&p, &["widthMm", "heightMm", "marginsMm"], "쪽")?;
    own(&s, &["font", "fontSize", "lineSpacing"], "기본 서식")?;
    let m = nullish(&p["marginsMm"], json!({}));
    own(&m, &["left", "right", "top", "bottom"], "여백")?;
    let page_width = number(nullish(&p["widthMm"], json!(210)), 100.0, 420.0, "쪽 너비")?;
    let page_height = number(nullish(&p["heightMm"], json!(297)), 100.0, 594.0, "쪽 높이")?;
    let mut margins = Map::new();
    for key in ["left", "right", "top", "bottom"] {
        margins.insert(
            key.into(),
            number(nullish(&m[key], json!(20)), 5.0, 80.0, "여백")?,
        );
    }
    if page_width.as_f64().unwrap()
        - margins["left"].as_f64().unwrap()
        - margins["right"].as_f64().unwrap()
        < 40.0
        || page_height.as_f64().unwrap()
            - margins["top"].as_f64().unwrap()
            - margins["bottom"].as_f64().unwrap()
            < 40.0
    {
        return Err("본문 영역이 너무 작습니다.".into());
    }
    let page = json!({"widthMm":page_width,"heightMm":page_height,"marginsMm":margins});
    let font = nullish(&s["font"], json!("굴림"));
    let font = text(&font)?;
    let font_size = number(nullish(&s["fontSize"], json!(11)), 8.0, 32.0, "글자 크기")?;
    let line_spacing = number(
        nullish(&s["lineSpacing"], json!(160)),
        100.0,
        250.0,
        "줄 간격",
    )?;
    if font.is_empty()
        || font.encode_utf16().count() > 80
        || font.contains(['\n', ';', '{', '}', '<', '>'])
    {
        return Err("잘못된 글꼴 이름".into());
    }
    let style = json!({"font":font,"fontSize":font_size,"lineSpacing":line_spacing});
    let source_blocks = input["blocks"]
        .as_array()
        .filter(|b| !b.is_empty() && b.len() <= 2000)
        .ok_or("본문 블록은 1~2,000개여야 합니다.")?;
    let mut characters = 0;
    let mut blocks = Vec::new();
    for (i, block) in source_blocks.iter().enumerate() {
        let fields = own(
            block,
            &[
                "type",
                "text",
                "level",
                "align",
                "items",
                "rows",
                "columnWidths",
                "header",
            ],
            &format!("블록 {}", i + 1),
        )?;
        // Preserve JavaScript's property-key conversion when selecting the
        // permitted fields; the subsequent type branches still use strict equality.
        let type_key = js_string(&block["type"]);
        let allowed: &[&str] = match type_key.as_str() {
            "paragraph" => &["type", "text", "align"],
            "heading" => &["type", "text", "level", "align"],
            "list" => &["type", "items"],
            "table" => &["type", "rows", "columnWidths", "header"],
            "pageBreak" => &["type"],
            _ => return Err(format!("블록 {}: 미지원 종류·속성", i + 1)),
        };
        if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(format!("블록 {}: 미지원 종류·속성", i + 1));
        }
        let kind = block["type"].as_str();
        if kind == Some("pageBreak") {
            if i == 0
                || i == source_blocks.len() - 1
                || source_blocks[i - 1]["type"].as_str() == Some("pageBreak")
            {
                return Err("처음·마지막·연속 쪽 나눔은 허용하지 않습니다.".into());
            }
            blocks.push(json!({"type":block["type"]}));
            continue;
        }
        if kind == Some("list") {
            let source = block["items"]
                .as_array()
                .filter(|items| !items.is_empty() && items.len() <= 500)
                .ok_or("목록 항목 수 오류")?;
            let items: Vec<Value> = source
                .iter()
                .map(|value| take_text(value, &mut characters))
                .collect::<Result<_, _>>()?;
            blocks.push(json!({"type":block["type"],"items":items}));
            continue;
        }
        if kind == Some("table") {
            let source = block["rows"]
                .as_array()
                .filter(|rows| !rows.is_empty() && rows.len() <= 500 && rows[0].is_array())
                .ok_or("표 행 수 오류")?;
            let cols = source[0].as_array().unwrap().len();
            if cols == 0
                || cols > 12
                || source
                    .iter()
                    .any(|row| row.as_array().is_none_or(|cells| cells.len() != cols))
            {
                return Err("표는 1~12열의 직사각형이어야 합니다.".into());
            }
            let widths = nullish(&block["columnWidths"], json!(vec![1; cols]));
            let width_values = widths
                .as_array()
                .filter(|v| v.len() == cols)
                .ok_or("열 너비 개수 오류")?;
            for width in width_values {
                number(width.clone(), 0.1, 100.0, "열 너비 비율")?;
            }
            if fields.contains_key("header") && !block["header"].is_boolean() {
                return Err("header는 true/false".into());
            }
            let rows: Vec<Vec<Value>> = source
                .iter()
                .map(|row| {
                    row.as_array()
                        .unwrap()
                        .iter()
                        .map(|value| take_text(value, &mut characters))
                        .collect()
                })
                .collect::<Result<_, _>>()?;
            blocks.push(json!({"type":block["type"],"rows":rows,"columnWidths":widths,"header":block.get("header").cloned().unwrap_or(json!(true))}));
            continue;
        }
        let align = nullish(&block["align"], json!("left"));
        if !align
            .as_str()
            .is_some_and(|a| ["left", "center", "right"].contains(&a))
        {
            return Err("정렬은 left/center/right".into());
        }
        let level = if kind == Some("heading") {
            Some(nullish(&block["level"], json!(1)))
        } else {
            None
        };
        if level
            .as_ref()
            .is_some_and(|level| !level.as_f64().is_some_and(|n| [1.0, 2.0, 3.0].contains(&n)))
        {
            return Err("제목 수준은 1~3".into());
        }
        let mut normalized = json!({"type":block["type"],"text":take_text(&block["text"], &mut characters)?,"align":align});
        if let Some(level) = level {
            normalized["level"] = level;
        }
        blocks.push(normalized);
    }
    if characters > 500000 {
        return Err("본문 500,000자 한도 초과".into());
    }
    let title = nullish(&input["title"], json!(""));
    Ok(json!({"schemaVersion":1,"title":text(&title)?,"page":page,"style":style,"blocks":blocks}))
}

fn page_break(line: &str) -> bool {
    line.strip_prefix("---")
        .or_else(|| line.strip_prefix("***"))
        .is_some_and(|rest| rest.chars().all(is_space))
}
fn bullet(line: &str) -> bool {
    line.strip_prefix('-')
        .or_else(|| line.strip_prefix('+'))
        .and_then(|rest| rest.chars().next())
        .is_some_and(is_space)
}
fn paragraph_stop(line: &str) -> bool {
    let hashes = line.bytes().take_while(|c| *c == b'#').count();
    ((1..=3).contains(&hashes) && line[hashes..].chars().next().is_some_and(is_space))
        || bullet(line)
        || page_break(line)
        || line.starts_with('|')
}
fn cells(line: &str) -> Vec<String> {
    let line = trim(line);
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|part| trim(part).to_string()).collect()
}
pub fn document_from_markdown(markdown: &str) -> Result<Value, String> {
    if markdown.encode_utf16().count() > 600000 {
        return Err("Markdown 크기 초과".into());
    }
    let source = markdown
        .strip_prefix('\u{feff}')
        .unwrap_or(markdown)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<_> = source.split('\n').collect();
    let mut blocks: Vec<Value> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if trim(line).is_empty() {
            i += 1;
            continue;
        }
        if page_break(line) {
            blocks.push(json!({"type":"pageBreak"}));
            i += 1;
            continue;
        }
        if let Some(heading) = HEADING.captures(line) {
            blocks.push(json!({"type":"heading","level":heading[1].len(),"text":&heading[2]}));
            i += 1;
            continue;
        }
        if bullet(line) {
            let mut items = Vec::new();
            while i < lines.len() && bullet(lines[i]) {
                items.push(lines[i][1..].trim_start_matches(is_space));
                i += 1;
            }
            blocks.push(json!({"type":"list","items":items}));
            continue;
        }
        if trim(line).starts_with('|')
            && i + 1 < lines.len()
            && cells(lines[i + 1])
                .iter()
                .all(|s| s.len() >= 3 && s.bytes().all(|b| b == b'-'))
        {
            let mut rows = vec![cells(line)];
            if cells(lines[i + 1]).len() != rows[0].len() {
                return Err("표 구분행의 열 수가 다릅니다.".into());
            }
            i += 2;
            while i < lines.len() && trim(lines[i]).starts_with('|') {
                rows.push(cells(lines[i]));
                i += 1;
            }
            blocks.push(json!({"type":"table","rows":rows}));
            continue;
        }
        let mut paragraph = Vec::new();
        while i < lines.len() && !trim(lines[i]).is_empty() && !paragraph_stop(lines[i]) {
            paragraph.push(lines[i]);
            i += 1;
        }
        if paragraph.is_empty() {
            return Err(format!("Markdown {}행: 지원하지 않는 구문", i + 1));
        }
        blocks.push(json!({"type":"paragraph","text":paragraph.join("\n")}));
    }
    for block in &blocks {
        let values: Vec<&Value> = if let Some(text) = block.get("text") {
            vec![text]
        } else if let Some(items) = block["items"].as_array() {
            items.iter().collect()
        } else if let Some(rows) = block["rows"].as_array() {
            rows.iter()
                .flat_map(|row| row.as_array().unwrap())
                .collect()
        } else {
            vec![]
        };
        if values
            .iter()
            .any(|v| UNSUPPORTED.is_match(v.as_str().unwrap()))
        {
            return Err("강조·코드·링크·그림·HTML·중첩 목록 등은 현재 Markdown 지원 범위가 아닙니다. 명시적 JSON을 사용하세요.".into());
        }
    }
    let title = blocks
        .iter()
        .find(|block| block["type"] == "heading")
        .map(|b| b["text"].clone())
        .unwrap_or(json!(""));
    normalize_document(&json!({"schemaVersion":1,"title":title,"blocks":blocks}))
}

fn border(id: usize, line: &str, fill: Option<&str>) -> String {
    let edges: String = ["left", "right", "top", "bottom"]
        .iter()
        .map(|d| format!(r##"<hh:{d}Border type="{line}" width="0.12 mm" color="#333333"/>"##))
        .collect();
    let fill = fill.map(|color| format!(r##"<hc:fillBrush><hc:winBrush faceColor="{color}" hatchColor="#999999" alpha="0"/></hc:fillBrush>"##)).unwrap_or_default();
    format!(
        r##"<hh:borderFill id="{id}" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0"><hh:slash type="NONE" Crooked="0" isCounter="0"/><hh:backSlash type="NONE" Crooked="0" isCounter="0"/>{edges}<hh:diagonal type="SOLID" width="0.1 mm" color="#000000"/>{fill}</hh:borderFill>"##
    )
}
fn header(model: &Value) -> Result<String, String> {
    let face = xml_escape(model["style"]["font"].as_str().unwrap())?;
    let fonts: String = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"].iter().map(|lang| format!(r#"<hh:fontface lang="{lang}" fontCnt="1"><hh:font id="0" face="{face}" type="TTF" isEmbedded="0"><hh:typeInfo familyType="FCAT_GOTHIC" weight="6" proportion="4" contrast="0" strokeVariation="1" armStyle="1" letterform="1" midline="1" xHeight="1"/></hh:font></hh:fontface>"#)).collect();
    let size = model["style"]["fontSize"].as_f64().unwrap();
    let chars: String = [size, 20.0, 16.0, 13.0, size]
        .iter()
        .enumerate()
        .map(|(i, size)| {
            DEFAULTS["charPr"]
                .as_str()
                .unwrap()
                .replacen("id=\"0\"", &format!("id=\"{i}\""), 1)
                .replacen(
                    "height=\"1000\"",
                    &format!("height=\"{}\"", numeric(round(size * 100.0))),
                    1,
                )
                .replacen(
                    "<hh:underline",
                    if i == 0 {
                        "<hh:underline"
                    } else {
                        "<hh:bold/><hh:underline"
                    },
                    1,
                )
        })
        .collect();
    let spacing = js_string(&model["style"]["lineSpacing"]);
    let paras: String = ["LEFT", "CENTER", "RIGHT"]
        .iter()
        .enumerate()
        .map(|(i, align)| {
            DEFAULTS["paraPr"]
                .as_str()
                .unwrap()
                .replacen("id=\"0\"", &format!("id=\"{i}\""), 1)
                .replacen(
                    "horizontal=\"JUSTIFY\"",
                    &format!("horizontal=\"{align}\""),
                    1,
                )
                .replace("value=\"160\"", &format!("value=\"{spacing}\""))
                .replacen("snapToGrid=\"1\"", "snapToGrid=\"0\"", 1)
        })
        .collect();
    let borders = format!(
        "{}{}{}{}",
        border(1, "NONE", None),
        border(2, "NONE", None),
        border(3, "SOLID", None),
        border(4, "SOLID", Some("#EDEDED"))
    );
    let numbering = DEFAULTS["numberings"].as_str().unwrap();
    let ns = namespace();
    Ok(format!(
        r#"{XML}<hh:head {ns} version="1.4" secCnt="1"><hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/><hh:refList><hh:fontfaces itemCnt="7">{fonts}</hh:fontfaces><hh:borderFills itemCnt="4">{borders}</hh:borderFills><hh:charProperties itemCnt="5">{chars}</hh:charProperties><hh:tabProperties itemCnt="1"><hh:tabPr id="0" autoTabLeft="0" autoTabRight="0"/></hh:tabProperties>{numbering}<hh:paraProperties itemCnt="3">{paras}</hh:paraProperties><hh:styles itemCnt="1"><hh:style id="0" type="PARA" name="바탕글" engName="Normal" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/></hh:styles></hh:refList><hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument><hh:docOption><hh:linkinfo path="" pageInherit="0" footnoteInherit="0"/></hh:docOption></hh:head>"#
    ))
}

struct Paragraphs {
    id: usize,
    first: bool,
    page_break: bool,
    section_property: String,
}
impl Paragraphs {
    fn paragraph(
        &mut self,
        value: &str,
        character: usize,
        align: &str,
        control: &str,
        cell: bool,
    ) -> Result<String, String> {
        let leading = if !cell && self.first {
            self.section_property.as_str()
        } else {
            ""
        };
        if !cell {
            self.first = false;
        }
        let pb = !cell && self.page_break;
        if !cell {
            self.page_break = false;
        }
        let id = self.id;
        self.id += 1;
        let align = ["left", "center", "right"]
            .iter()
            .position(|a| *a == align)
            .unwrap();
        let text = xml_escape(value)?.replace('\n', "<hp:lineBreak/>");
        Ok(format!(
            r#"<hp:p id="{id}" paraPrIDRef="{align}" styleIDRef="0" pageBreak="{}" columnBreak="0" merged="0"><hp:run charPrIDRef="{character}">{leading}{control}<hp:t>{text}</hp:t></hp:run></hp:p>"#,
            u8::from(pb)
        ))
    }
}

#[derive(Debug)]
pub struct Composed {
    pub bytes: Vec<u8>,
    pub model: Value,
}

pub fn compose_hwpx(input: &Value) -> Result<Composed, String> {
    let model = normalize_document(input)?;
    let page = &model["page"];
    let margin = &page["marginsMm"];
    let n = |value: &Value| value.as_f64().unwrap();
    let width = mm(n(&page["widthMm"]) - n(&margin["left"]) - n(&margin["right"]));
    let page_property = format!(
        r#"<hp:pagePr landscape="WIDELY" width="{}" height="{}" gutterType="LEFT_ONLY"><hp:margin header="0" footer="0" gutter="0" left="{}" right="{}" top="{}" bottom="{}"/></hp:pagePr>"#,
        numeric(mm(n(&page["widthMm"]))),
        numeric(mm(n(&page["heightMm"]))),
        numeric(mm(n(&margin["left"]))),
        numeric(mm(n(&margin["right"]))),
        numeric(mm(n(&margin["top"]))),
        numeric(mm(n(&margin["bottom"])))
    );
    let template = DEFAULTS["secPr"].as_str().unwrap();
    let start = template.find("<hp:pagePr").unwrap();
    let end = start + template[start..].find("</hp:pagePr>").unwrap() + "</hp:pagePr>".len();
    let section_property = format!(
        r#"{}{page_property}{}<hp:ctrl><hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/></hp:ctrl>"#,
        &template[..start],
        &template[end..]
    );
    let mut writer = Paragraphs {
        id: 0,
        first: true,
        page_break: false,
        section_property,
    };
    let mut paragraphs = String::new();
    let blocks = model["blocks"].as_array().unwrap();
    for block in blocks {
        let kind = block["type"].as_str();
        if kind == Some("pageBreak") {
            writer.page_break = true;
            continue;
        }
        if kind == Some("list") {
            for text in block["items"].as_array().unwrap() {
                paragraphs.push_str(&writer.paragraph(
                    &format!("• {}", text.as_str().unwrap()),
                    0,
                    "left",
                    "",
                    false,
                )?);
            }
            continue;
        }
        if kind != Some("table") {
            let character = if kind == Some("heading") {
                n(&block["level"]) as usize
            } else {
                0
            };
            paragraphs.push_str(&writer.paragraph(
                block["text"].as_str().unwrap(),
                character,
                block["align"].as_str().unwrap(),
                "",
                false,
            )?);
            continue;
        }
        let source_widths = block["columnWidths"].as_array().unwrap();
        let sum = source_widths.iter().fold(0.0, |total, w| total + n(w));
        let mut remaining = width;
        let widths: Vec<f64> = source_widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let size = if i == source_widths.len() - 1 {
                    remaining
                } else {
                    round(width * n(w) / sum)
                };
                remaining -= size;
                size
            })
            .collect();
        if widths.iter().any(|w| *w < 1500.0) {
            return Err("표 열 너비가 너무 좁습니다. 열 수·비율·쪽 너비를 조정하세요.".into());
        }
        let row_height =
            (n(&model["style"]["fontSize"]) * 100.0 * n(&model["style"]["lineSpacing"]) / 100.0)
                .ceil()
                + 800.0;
        let source_rows = block["rows"].as_array().unwrap();
        let has_header = block["header"].as_bool().unwrap();
        let mut rows = String::new();
        for (r, row) in source_rows.iter().enumerate() {
            rows.push_str("<hp:tr>");
            for (c, text) in row.as_array().unwrap().iter().enumerate() {
                let is_header = r == 0 && has_header;
                let paragraph = writer.paragraph(
                    text.as_str().unwrap(),
                    if is_header { 4 } else { 0 },
                    "left",
                    "",
                    true,
                )?;
                rows.push_str(&format!(r#"<hp:tc name="" header="{}" hasMargin="1" protect="0" editable="0" dirty="0" borderFillIDRef="{}"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0">{paragraph}</hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="{}" height="{}"/><hp:cellMargin left="400" right="400" top="400" bottom="400"/></hp:tc>"#, u8::from(is_header), if is_header { 4 } else { 3 }, numeric(widths[c]), numeric(row_height)));
            }
            rows.push_str("</hp:tr>");
        }
        let table = format!(
            r#"<hp:tbl id="{}" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="{}" rowCnt="{}" colCnt="{}" cellSpacing="0" borderFillIDRef="3" noAdjust="0"><hp:sz width="{}" widthRelTo="ABSOLUTE" height="{}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="600"/><hp:inMargin left="400" right="400" top="400" bottom="400"/>{rows}</hp:tbl>"#,
            100000 + writer.id,
            u8::from(has_header),
            source_rows.len(),
            widths.len(),
            numeric(width),
            numeric(row_height * source_rows.len() as f64)
        );
        paragraphs.push_str(&writer.paragraph("", 0, "left", &table, false)?);
    }
    let ns = namespace();
    let section = format!(r#"{XML}<hs:sec {ns}>{paragraphs}</hs:sec>"#);
    let mut preview = Vec::new();
    for block in blocks {
        if let Some(text) = block.get("text") {
            preview.push(text.as_str().unwrap().to_string());
        } else if let Some(items) = block["items"].as_array() {
            preview.extend(items.iter().map(|t| t.as_str().unwrap().to_string()));
        } else if let Some(rows) = block["rows"].as_array() {
            preview.extend(rows.iter().map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("\t")
            }));
        }
    }
    let title = xml_escape(model["title"].as_str().unwrap())?;
    let entries: Vec<(String, Vec<u8>)> = vec![
        ("mimetype", "application/hwp+zip".into()),
        ("version.xml", format!(r#"{XML}<hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" targetApplication="WORDPROCESSOR" major="5" minor="1" micro="0" buildNumber="1" os="1" xmlVersion="1.4" application="my-system" appVersion="1.0"/>"#)),
        ("Contents/header.xml", header(&model)?), ("Contents/section0.xml", section),
        ("Preview/PrvText.txt", preview.join("\r\n")),
        ("settings.xml", format!(r#"{XML}<ha:HWPApplicationSetting xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app"><ha:CaretPosition listIDRef="0" paraIDRef="0" pos="0"/></ha:HWPApplicationSetting>"#)),
        ("META-INF/container.xml", format!(r#"{XML}<ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#)),
        ("META-INF/manifest.xml", format!(r#"{XML}<odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#)),
        ("Contents/content.hpf", format!(r#"{XML}<opf:package {ns} version="" unique-identifier="" id=""><opf:metadata><opf:title>{title}</opf:title><opf:language>ko</opf:language><opf:meta name="creator" content="text">my-system</opf:meta></opf:metadata><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="settings" href="settings.xml" media-type="application/xml"/></opf:manifest><opf:spine><opf:itemref idref="header" linear="no"/><opf:itemref idref="section0" linear="yes"/></opf:spine></opf:package>"#)),
    ].into_iter().map(|(name, content)| (name.into(), content.into_bytes())).collect();
    let bytes = create_zip(&entries)?;
    load_hwpx(&bytes)?;
    Ok(Composed { bytes, model })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office_zip::read_zip_strict;
    #[test]
    fn null_defaults_header_types_and_utf16_limits_are_preserved() {
        let model = normalize_document(&json!({"schemaVersion":1.0,"page":null,"style":{"fontSize":null},"blocks":[{"type":"heading","text":"가상 😀","level":null,"align":null}]})).unwrap();
        assert_eq!(model["style"]["font"], "굴림");
        assert_eq!(model["blocks"][0]["level"], 1);
        assert!(
            normalize_document(
                &json!({"schemaVersion":1,"blocks":[{"type":"table","rows":[[""]],"header":null}]})
            )
            .is_err()
        );
        let allowed = "😀".repeat(25000);
        assert!(
            normalize_document(
                &json!({"schemaVersion":1,"blocks":[{"type":"paragraph","text":allowed}]})
            )
            .is_ok()
        );
        assert!(normalize_document(&json!({"schemaVersion":1,"blocks":[{"type":"paragraph","text":"😀".repeat(25001)}]})).is_err());
        assert!(normalize_document(&json!({"schemaVersion":1,"style":{"font":"😀".repeat(41)},"blocks":[{"type":"paragraph","text":""}]})).is_err());
        for invalid in [
            json!({"schemaVersion":true,"blocks":[]}),
            json!({"schemaVersion":1,"page":[],"blocks":[]}),
            json!({"schemaVersion":1,"blocks":[{"type":"paragraph","text":"x","items":[]}]}),
            json!({"schemaVersion":1,"blocks":[{"type":"paragraph","text":"x\t"}]}),
        ] {
            assert!(normalize_document(&invalid).is_err());
        }
    }
    #[test]
    fn markdown_supported_blocks_and_rejections_remain_explicit() {
        let model = document_from_markdown("\u{feff}# 제목\r\n\r\n첫 줄\r둘째 줄\n\n- 항목\n+ 둘\n\n| 제목 | 값 |\n| --- | --- |\n| 빈칸 | |\n\n---\n\n끝").unwrap();
        assert_eq!(model["title"], "제목");
        assert_eq!(model["blocks"].as_array().unwrap().len(), 6);
        assert_eq!(model["blocks"][1]["text"], "첫 줄\n둘째 줄");
        assert_eq!(model["blocks"][3]["rows"][1][1], "");
        for unsupported in [
            "**강조**",
            "_강조_",
            "`code`",
            "[링크](x)",
            "<b>html</b>",
            "  - nested",
            "#### 제목",
            "> 인용",
            "| 잘못된 표 |",
            "---\n본문",
            "본문\n---",
            "# ",
        ] {
            assert!(
                document_from_markdown(unsupported).is_err(),
                "{unsupported}"
            );
        }
        assert!(document_from_markdown("#\u{feff}제목").is_ok());
        assert!(document_from_markdown("#\u{85}제목").is_ok()); // NEL is body text, not JS whitespace.
    }
    #[test]
    fn first_table_keeps_control_placement_ids_styles_and_page_breaks() {
        let input = json!({"schemaVersion":1,"title":"가상 & <제목>","style":{"fontSize":11.25,"lineSpacing":160.5},"blocks":[{"type":"table","rows":[["제목","값"],["내용\n다음",""]],"columnWidths":[1,2]},{"type":"pageBreak"},{"type":"list","items":["다음 쪽"]},{"type":"heading","text":"끝","level":2,"align":"right"}]});
        let composed = compose_hwpx(&input).unwrap();
        let archive = read_zip_strict(&composed.bytes).unwrap();
        assert_eq!(archive.entries.len(), 9);
        assert_eq!(archive.entries[0].name, "mimetype");
        let section = std::str::from_utf8(archive.read("Contents/section0.xml").unwrap()).unwrap();
        assert_eq!(section.matches("<hp:secPr ").count(), 1);
        assert!(section.contains("<hp:p id=\"4\" paraPrIDRef=\"0\""));
        assert!(section.contains("<hp:tbl id=\"100004\""));
        assert!(
            section.contains("<hp:p id=\"5\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"1\"")
        );
        assert!(section.contains("내용<hp:lineBreak/>다음"));
        assert_eq!(
            archive.read("Preview/PrvText.txt").unwrap(),
            "제목\t값\r\n내용\n다음\t\r\n다음 쪽\r\n끝".as_bytes()
        );
        let header = std::str::from_utf8(archive.read("Contents/header.xml").unwrap()).unwrap();
        assert!(header.contains("height=\"1125\""));
        assert!(header.contains("value=\"160.5\""));
        assert_eq!(compose_hwpx(&input).unwrap().bytes, composed.bytes);
    }
    #[test]
    fn positive_js_rounding_and_narrow_columns() {
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(2.5), 3.0);
        let error = compose_hwpx(&json!({"schemaVersion":1,"blocks":[{"type":"table","rows":[["A","B"]],"columnWidths":[0.1,100]}]})).unwrap_err();
        assert!(error.contains("표 열 너비"));
        for blocks in [
            json!([{"type":"pageBreak"},{"type":"paragraph","text":"x"}]),
            json!([{"type":"paragraph","text":"x"},{"type":"pageBreak"}]),
            json!([{"type":"paragraph","text":"x"},{"type":"pageBreak"},{"type":"pageBreak"},{"type":"paragraph","text":"y"}]),
        ] {
            assert!(normalize_document(&json!({"schemaVersion":1,"blocks":blocks})).is_err());
        }
    }
}
