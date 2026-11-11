//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
use super::header_xml_compat::parse_header_xml;
use super::style_dom::{ParsedXml, XmlPart};
use super::value::{Map, Value, native_json as json};
use crate::input::trim;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub fn default_para_geom() -> Value {
    json!({"lineSpacingType":"PERCENT","lineSpacingValue":160,"marginLeft":0,
        "marginRight":0,"marginIntent":0,"spaceBefore":0,"spaceAfter":0})
}
pub fn default_char() -> Value {
    json!({"height":1000,"bold":false,"italic":false,"underline":false,"ratio":100,"spacing":0})
}
fn number(v: Option<&str>) -> f64 {
    super::layout::number(Some(
        &v.map(|s| serde_json::Value::String(s.into()))
            .unwrap_or(serde_json::Value::Null),
    ))
}
fn number_or(v: Option<&str>, fallback: f64) -> f64 {
    let n = number(v);
    if n == 0.0 || n.is_nan() { fallback } else { n }
}
fn parse_float(s: &str) -> f64 {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[+-]?(?:(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?|Infinity)").unwrap()
    });
    PREFIX
        .find(trim(s))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(f64::NAN)
}
pub fn hwpx_app_version(xml: Option<&str>, build: bool) -> Option<f64> {
    const SPACE: &str = r"[\t\n\x0B\x0C\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]*";
    static MAJOR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r#"appVersion="{SPACE}([0-9]+)"#)).unwrap());
    static BUILD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r#"appVersion="{SPACE}[0-9]+{SPACE},{SPACE}[0-9]+{SPACE},{SPACE}[0-9]+{SPACE},{SPACE}([0-9]+)"#)).unwrap()
    });
    let captures = (if build { &BUILD } else { &MAJOR }).captures(xml?)?;
    Some(number(Some(&captures[1])))
}
pub fn doc_color<'a>(v: Option<&'a str>, fallback: Option<&'a str>) -> Option<&'a str> {
    v.filter(|s| {
        s.len() == 7 && s.starts_with('#') && s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    })
    .or(fallback)
}
pub fn hwp_face_to_css_stack(face: Option<&str>) -> String {
    let face = trim(face.unwrap_or(""));
    if face.is_empty() {
        return String::new();
    }
    let alias = match face {
        "HY헤드라인M" => Some("'HYHeadLine-Medium','HY헤드라인M'"),
        "한양신명조" => Some("'HYSinMyeongJo-Medium','HY신명조'"),
        "함초롬바탕" | "한컴바탕" => Some("'HCR Batang','함초롬바탕','한컴바탕'"),
        "함초롬돋움" | "한컴돋움" => Some("'HCR Dotum','함초롬돋움','한컴돋움'"),
        "맑은 고딕" | "맑은고딕" => Some("'Malgun Gothic','맑은 고딕'"),
        "굴림" => Some("'Gulim','굴림'"),
        "굴림체" => Some("'GulimChe','굴림체','Gulim'"),
        "돋움" => Some("'Dotum','돋움'"),
        "돋움체" => Some("'DotumChe','돋움체','Dotum'"),
        "바탕" => Some("'Batang','바탕'"),
        "바탕체" => Some("'BatangChe','바탕체','Batang'"),
        "궁서" => Some("'Gungsuh','궁서'"),
        "궁서체" => Some("'GungsuhChe','궁서체','Gungsuh'"),
        "나눔고딕" => Some("'NanumGothic','나눔고딕'"),
        "나눔명조" => Some("'NanumMyeongjo','나눔명조'"),
        "맑은 고딕 Semilight" => Some("'Malgun Gothic Semilight','맑은 고딕'"),
        _ => None,
    };
    let inherited = match face {
        "constructor" => Some("function Object() { [native code] }".into()),
        "__proto__" => Some("[object Object]".into()),
        "__defineGetter__"
        | "__defineSetter__"
        | "hasOwnProperty"
        | "__lookupGetter__"
        | "__lookupSetter__"
        | "isPrototypeOf"
        | "propertyIsEnumerable"
        | "toLocaleString"
        | "toString"
        | "valueOf" => Some(format!("function {face}() {{ [native code] }}")),
        _ => None,
    };
    let head = alias.map(str::to_owned).or(inherited).unwrap_or_else(|| {
        if face
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            face.into()
        } else {
            format!("'{}'", face.replace(['\'', '"', '\\'], ""))
        }
    });
    let low = face.to_lowercase();
    let serif = [
        "바탕", "명조", "batang", "myeong", "mincho", "궁서", "gungsuh", "serif", "신명", "순명",
        "song", "송",
    ]
    .iter()
    .any(|p| low.contains(p));
    format!(
        "{head},{}",
        if serif {
            "'HCR Batang','Batang','Noto Serif KR',serif"
        } else {
            "'Malgun Gothic','HCR Dotum','Noto Sans KR',sans-serif"
        }
    )
}
fn child(d: &ParsedXml, i: usize, name: &str) -> Option<usize> {
    d.nodes[i]
        .children
        .iter()
        .copied()
        .find(|&c| d.nodes[c].local == name)
}
fn deep(d: &ParsedXml, i: usize, name: &str, depth: usize, max: usize) -> Option<usize> {
    if depth > max {
        return None;
    }
    for &c in &d.nodes[i].children {
        if d.nodes[c].local == name {
            return Some(c);
        }
        if let Some(found) = deep(d, c, name, depth + 1, max) {
            return Some(found);
        }
    }
    None
}
fn attr<'a>(d: &'a ParsedXml, i: Option<usize>, name: &str) -> Option<&'a str> {
    i.and_then(|i| d.nodes[i].attr(name))
}
fn text_content(d: &ParsedXml, i: usize) -> String {
    d.nodes[i]
        .parts
        .iter()
        .map(|p| match p {
            XmlPart::Text(s) => s.clone(),
            XmlPart::Node(n) => text_content(d, *n),
        })
        .collect()
}
struct Fonts {
    faces: HashMap<String, String>,
    hft: HashSet<String>,
}
fn collect_fonts(d: &ParsedXml, lang: &str, subst: bool) -> Fonts {
    let mut out = Fonts {
        faces: HashMap::new(),
        hft: HashSet::new(),
    };
    let Some(faces) = deep(d, d.root, "fontfaces", 0, 24) else {
        return out;
    };
    let groups: Vec<_> = d.nodes[faces]
        .children
        .iter()
        .copied()
        .filter(|&i| d.nodes[i].local == "fontface")
        .collect();
    let Some(group) = groups
        .iter()
        .copied()
        .find(|&i| d.nodes[i].attr("lang").unwrap_or("").to_uppercase() == lang)
        .or(groups.first().copied())
    else {
        return out;
    };
    for &i in &d.nodes[group].children {
        let e = &d.nodes[i];
        if e.local != "font" {
            continue;
        }
        let mut face = e.attr("face");
        let mut typ = e.attr("type");
        if face.is_some_and(|s| {
            !s.is_empty() && (s.starts_with("한양") || subst && s.starts_with("KoPub"))
        }) && typ == Some("TTF")
        {
            let sub = child(d, i, "substFont");
            if let Some(sf) = attr(d, sub, "face").filter(|s| !s.is_empty()) {
                face = Some(sf);
                typ = attr(d, sub, "type");
            }
        }
        if let Some(id) = e.attr("id") {
            if let Some(face) = face.filter(|s| !s.is_empty()) {
                out.faces.insert(id.into(), face.into());
            }
            if typ == Some("HFT") {
                out.hft.insert(id.into());
            }
        }
    }
    out
}
fn edge(d: &ParsedXml, i: Option<usize>) -> Option<Value> {
    let e = &d.nodes[i?];
    let typ = e.attr("type").unwrap_or("NONE");
    if typ == "NONE" {
        return None;
    }
    let width = parse_float(e.attr("width").unwrap_or(""));
    Some(
        json!({"type":typ,"widthPt":if width.is_finite(){width*2.834645}else{0.34},"color":doc_color(e.attr("color"),Some("#000000"))}),
    )
}
fn para_geom(d: &ParsedXml, i: usize, old: bool) -> Value {
    let e = &d.nodes[i];
    let mut g = default_para_geom();
    let c = number_or(e.attr("condense"), 0.0);
    if c > 0.0 {
        g["condense"] = json!(c);
    }
    if e.attr("fontLineHeight") == Some("1") {
        g["fontLineHeight"] = true.into();
    }
    if let Some(ls) = deep(d, i, "lineSpacing", 0, 32) {
        let e = &d.nodes[ls];
        g["lineSpacingType"] = e.attr("type").unwrap_or("PERCENT").into();
        let v = number(e.attr("value"));
        g["lineSpacingValue"] = json!(if v.is_finite() && e.attr("value").is_some() {
            v
        } else {
            160.0
        });
    }
    if let Some(m) = deep(d, i, "margin", 0, 32) {
        let plain = old
            && d.nodes[m]
                .parent
                .is_some_and(|p| d.nodes[p].local == "paraPr");
        for (name, key) in [
            ("left", "marginLeft"),
            ("right", "marginRight"),
            ("intent", "marginIntent"),
            ("prev", "spaceBefore"),
            ("next", "spaceAfter"),
        ] {
            let c = deep(d, m, name, 0, 32);
            let x = number_or(attr(d, c, "value"), 0.0)
                * if attr(d, c, "unit") == Some("CHAR") {
                    5.0
                } else {
                    1.0
                };
            g[key] = json!(if plain { (x / 2.0).trunc() } else { x });
        }
    }
    if let Some(va) = attr(d, deep(d, i, "align", 0, 32), "vertical")
        .filter(|s| !s.is_empty() && *s != "BASELINE")
    {
        g["vertAlign"] = va.into();
    }
    if let Some(b) = deep(d, i, "breakSetting", 0, 32) {
        let b = &d.nodes[b];
        for (name, val, key) in [
            ("breakLatinWord", "BREAK_WORD", "latinBreak"),
            ("lineWrap", "SQUEEZE", "squeeze"),
            ("widowOrphan", "1", "widowOrphan"),
            ("keepLines", "1", "keepLines"),
            ("keepWithNext", "1", "keepWithNext"),
            ("pageBreakBefore", "1", "pageBreakBefore"),
        ] {
            if b.attr(name) == Some(val) {
                g[key] = true.into();
            }
        }
        match b.attr("breakNonLatinWord") {
            Some("BREAK_WORD") => g["wrapMode"] = "keep".into(),
            Some("KEEP_WORD") => g["wrapMode"] = "charAll".into(),
            _ => {}
        }
    }
    g
}
fn optional_num(d: &ParsedXml, i: Option<usize>, a: &str, default: f64) -> f64 {
    match attr(d, i, a) {
        None | Some("") => default,
        Some(v) => number(Some(v)),
    }
}
fn lang_style(
    d: &ParsedXml,
    elements: [Option<usize>; 4],
    lang: &str,
    fonts: &Fonts,
    base: Option<&String>,
    hft: bool,
) -> Value {
    let [font_ref, ratio, spacing, rel] = elements;
    let id = attr(d, font_ref, lang).unwrap_or("");
    let mut v = json!({"ratio":optional_num(d,ratio,lang,100.0),"spacing":optional_num(d,spacing,lang,0.0),"rel":optional_num(d,rel,lang,100.0)});
    if let Some(face) = fonts.faces.get(id).or(base) {
        v["face"] = face.clone().into();
    }
    if if fonts.faces.contains_key(id) {
        fonts.hft.contains(id)
    } else {
        hft
    } {
        v["hft"] = true.into();
    }
    v
}
fn char_style(d: &ParsedXml, i: usize, fonts: &[Fonts; 5]) -> Value {
    let e = &d.nodes[i];
    let ratio = child(d, i, "ratio");
    let spacing = child(d, i, "spacing");
    let rel_el = child(d, i, "relSz");
    let fr = child(d, i, "fontRef");
    let fid = attr(d, fr, "hangul").or_else(|| attr(d, fr, "latin"));
    let face = fid.and_then(|s| fonts[0].faces.get(s));
    let hangul_hft = fonts[0].hft.contains(attr(d, fr, "hangul").unwrap_or(""));
    let hangul_ratio = number_or(attr(d, ratio, "hangul"), 100.0);
    let hangul_spacing = number_or(attr(d, spacing, "hangul"), 0.0);
    let rel = optional_num(d, rel_el, "hangul", 100.0);
    let underline = child(d, i, "underline");
    let mut v = json!({"height":number_or(e.attr("height"),1000.0),"bold":child(d,i,"bold").is_some(),"italic":child(d,i,"italic").is_some(),"underline":underline.is_some()&&attr(d,underline,"type").unwrap_or("NONE")!="NONE","ratio":hangul_ratio,"spacing":hangul_spacing});
    if e.attr("textColor") != Some("#000000")
        && let Some(c) = doc_color(e.attr("textColor"), None)
    {
        v["color"] = c.into();
    }
    if let Some(f) = face {
        v["face"] = f.clone().into();
        if !f.is_empty() {
            v["fontFamily"] = hwp_face_to_css_stack(Some(f)).into();
        }
    }
    if rel != 100.0 {
        v["rel"] = json!(rel);
    }
    if let Some(c) = doc_color(e.attr("shadeColor"), None).filter(|s| s.to_uppercase() != "#FFFFFF")
    {
        v["shade"] = c.into();
    }
    if let Some(b) = e.attr("borderFillIDRef").filter(|s| !s.is_empty()) {
        v["borderFill"] = b.into();
    }
    if hangul_hft {
        v["hft"] = true.into();
    }
    for (index, lang) in [(1, "latin"), (2, "hanja"), (3, "japanese"), (4, "symbol")] {
        let mut ls = lang_style(
            d,
            [fr, ratio, spacing, rel_el],
            lang,
            &fonts[index],
            face,
            if index == 1 { false } else { hangul_hft },
        );
        // Latin HFT is tested directly even when that id has no face.
        if index == 1 && fonts[1].hft.contains(attr(d, fr, "latin").unwrap_or("")) {
            ls["hft"] = true.into();
        }
        let differs = ls.get("face").and_then(Value::as_str) != face.map(String::as_str)
            || optional_num(d, ratio, lang, 100.0) != hangul_ratio
            || optional_num(d, spacing, lang, 0.0) != hangul_spacing
            || optional_num(d, rel_el, lang, 100.0) != rel
            || index != 1
                && (ls.get("hft").and_then(Value::as_bool).unwrap_or(false) != hangul_hft);
        if differs {
            v[lang] = ls;
        }
    }
    if e.attr("useFontSpace") == Some("1") {
        v["fontSpace"] = true.into();
    }
    if child(d, i, "supscript").is_some() {
        v["script"] = "sup".into();
    } else if child(d, i, "subscript").is_some() {
        v["script"] = "sub".into();
    }
    v
}
fn head_props(d: &ParsedXml, h: Option<usize>, bullet: bool) -> Value {
    let mut v = json!({"offset":number_or(attr(d,h,"textOffset"),0.0),"offsetPct":attr(d,h,"textOffsetType").unwrap_or("PERCENT")=="PERCENT","autoIndent":attr(d,h,"autoIndent")!=Some("0")});
    if let Some(c) = attr(d, h, "charPrIDRef").filter(|s| *s != "4294967295") {
        v["charPrId"] = c.into();
    }
    if bullet && attr(d, h, "useInstWidth").is_some() {
        v["useInstWidth"] = (attr(d, h, "useInstWidth") != Some("0")).into();
    }
    v
}
/// Maps are represented as arrays of [key,value] entries, preserving Map insertion order
/// and numeric numbering keys. JSON objects would reorder integer-like ids in JS.
pub fn parse_render_styles(xml: &str, opts: Option<&Value>) -> Result<Value, String> {
    let major_value = opts.and_then(|o| o.get("appMajor")).unwrap_or(&Value::Null);
    let major = native_number(major_value);
    let build = native_number(opts.and_then(|o| o.get("appBuild")).unwrap_or(&Value::Null));
    let before = major > 0.0 && major < 13.0;
    let half = major_value.as_f64() == Some(12.0) && build >= 4605.0
        || major_value.as_f64() == Some(13.0) && build >= 3901.0
        || major > 13.0;
    let early = major_value.as_f64() == Some(13.0) && build > 0.0 && build < 3622.0;
    let mut chars: Map<String, Value> = Map::new();
    let mut aligns: Map<String, Value> = Map::new();
    let mut geoms: Map<String, Value> = Map::new();
    let mut borders: Map<String, Value> = Map::new();
    let mut tabs = HashSet::new();
    let mut para_tabs: Map<String, Value> = Map::new();
    let mut para_bullets: Map<String, Value> = Map::new();
    let mut bullets: Map<String, Value> = Map::new();
    let mut para_nums: HashMap<String, (String, f64)> = HashMap::new();
    let mut nums: HashMap<String, Vec<(f64, Value)>> = HashMap::new();
    let mut page_num = None;
    let d = parse_header_xml(xml)?;
    let hv = parse_float(d.nodes[d.root].attr("version").unwrap_or(""));
    let old = hv.is_finite() && hv < 1.4;
    let subst = opts
        .and_then(|o| o.get("substKoPub"))
        .is_some_and(|v| match v {
            Value::Null | Value::Undefined => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            _ => true,
        });
    let fonts = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "SYMBOL"]
        .map(|lang| collect_fonts(&d, lang, subst));
    for (i, e) in d.nodes.iter().enumerate() {
        if let Some(id) = e.attr("id") {
            match e.local.as_str() {
                "charPr" => {
                    let mut v = char_style(&d, i, &fonts);
                    if before {
                        v["before2024"] = true.into();
                    }
                    if half {
                        v["halfQuote"] = true.into();
                    }
                    chars.insert(id.into(), v);
                }
                "tabPr" => {
                    if e.attr("autoTabLeft") == Some("1") {
                        tabs.insert(id.to_owned());
                    }
                }
                "paraPr" => {
                    aligns.insert(
                        id.into(),
                        attr(&d, child(&d, i, "align"), "horizontal")
                            .filter(|s| !s.is_empty())
                            .unwrap_or("JUSTIFY")
                            .into(),
                    );
                    geoms.insert(id.into(), para_geom(&d, i, old));
                    if let Some(t) = e.attr("tabPrIDRef") {
                        para_tabs.insert(id.into(), t.into());
                    }
                    let h = deep(&d, i, "heading", 0, 32);
                    match attr(&d, h, "type") {
                        Some("BULLET") => {
                            para_bullets
                                .insert(id.into(), attr(&d, h, "idRef").unwrap_or("").into());
                        }
                        Some("NUMBER") => {
                            para_nums.insert(
                                id.into(),
                                (
                                    attr(&d, h, "idRef").unwrap_or("").into(),
                                    number_or(attr(&d, h, "level"), 0.0) + 1.0,
                                ),
                            );
                        }
                        _ => {}
                    }
                }
                "bullet" if e.attr("useImage") != Some("1") => {
                    let mut v = head_props(&d, child(&d, i, "paraHead"), true);
                    v["ch"] = e.attr("char").unwrap_or("").into();
                    bullets.insert(id.into(), v);
                }
                "numbering" => {
                    let mut levels: Vec<(f64, Value)> = Vec::new();
                    for &h in &e.children {
                        if d.nodes[h].local != "paraHead" {
                            continue;
                        }
                        let he = &d.nodes[h];
                        let level = number_or(he.attr("level"), 1.0);
                        let mut v = head_props(&d, Some(h), false);
                        v["head"] = json!({"text":text_content(&d,h),"format":he.attr("numFormat").unwrap_or("DIGIT"),"start":number(he.attr("start").or(Some("1")))});
                        if let Some((_, old)) = levels.iter_mut().find(|(n, _)| *n == level) {
                            *old = v;
                        } else {
                            levels.push((level, v));
                        }
                    }
                    nums.insert(id.into(), levels);
                }
                "borderFill" => {
                    let mut v = json!({});
                    for (side, name) in [
                        ("left", "leftBorder"),
                        ("right", "rightBorder"),
                        ("top", "topBorder"),
                        ("bottom", "bottomBorder"),
                    ] {
                        if let Some(b) = edge(&d, child(&d, i, name)) {
                            v[side] = b;
                        }
                    }
                    let brush = child(&d, i, "fillBrush");
                    let win = brush.and_then(|b| child(&d, b, "winBrush"));
                    if let Some(c) = doc_color(attr(&d, win, "faceColor"), None) {
                        v["fill"] = c.into();
                    }
                    if let Some(g) = brush.and_then(|b| child(&d, b, "gradation")) {
                        let ge = &d.nodes[g];
                        let colors: Vec<_> = ge
                            .children
                            .iter()
                            .filter_map(|&c| doc_color(d.nodes[c].attr("value"), None))
                            .collect();
                        if colors.len() >= 2 && matches!(ge.attr("type"), Some("LINEAR" | "SQUARE"))
                        {
                            v["gradient"] = json!({"type":ge.attr("type"),"colors":colors,"angle":number_or(ge.attr("angle"),0.0),"centerX":number_or(ge.attr("centerX"),0.0),"centerY":number_or(ge.attr("centerY"),0.0)});
                        }
                    }
                    borders.insert(id.into(), v);
                }
                _ => {}
            }
        }
        if e.local == "style"
            && (e.attr("engName") == Some("Page Number") || e.attr("name") == Some("쪽 번호"))
            && page_num.is_none()
        {
            page_num = e.attr("charPrIDRef").map(str::to_owned);
        }
    }
    for (id, t) in para_tabs {
        if tabs.contains(t.as_str().unwrap_or(""))
            && let Some(g) = geoms.get_mut(&id)
        {
            g["autoTabLeft"] = true.into();
        }
    }
    for (id, (num_id, level)) in para_nums {
        let Some(levels) = nums.get(&num_id) else {
            continue;
        };
        let Some((_, lv)) = levels.iter().find(|(l, _)| *l == level) else {
            continue;
        };
        if let Some(g) = geoms.get_mut(&id) {
            let mut num = json!({"id":num_id,"level":level,"heads":levels.iter().map(|(l,v)|json!([l,v["head"]])).collect::<Vec<_>>(),"offset":lv["offset"],"offsetPct":lv["offsetPct"],"autoIndent":lv["autoIndent"]});
            if let Some(c) = lv.get("charPrId") {
                num["charPrId"] = c.clone();
            }
            g["number"] = num;
        }
    }
    for (id, b) in para_bullets {
        if let (Some(g), Some(bu)) = (geoms.get_mut(&id), bullets.get(b.as_str().unwrap_or("")))
            && bu["ch"].as_str().is_some_and(|s| !s.is_empty())
        {
            g["bullet"] = bu.clone();
        }
    }
    let entries = |m: Map<String, Value>| {
        m.into_iter()
            .map(|(k, v)| json!([k, v]))
            .collect::<Vec<_>>()
    };
    let mut result = json!({"charPr":entries(chars),"paraAlign":entries(aligns),"paraGeom":entries(geoms),"borderFill":entries(borders)});
    if before {
        result["before2024"] = true.into();
    }
    if early {
        result["early2024"] = true.into();
    }
    if let Some(n) = page_num {
        result["pageNumCharPr"] = n.into();
    }
    Ok(result)
}
fn native_number(value: &Value) -> f64 {
    super::value::number(value)
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|v| v.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    Ok(match operation {
        "glyphWidths" => {
            let styles = parse_render_styles(
                input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?,
                input.get("opts"),
            )?;
            let text = input["text"].as_str().ok_or("text 문자열이 필요합니다.")?;
            let mut out = Vec::new();
            for entry in styles["charPr"].as_array().unwrap() {
                for ch in text.chars() {
                    let ch = ch.to_string();
                    let (w, sp) = super::glyph::glyph_width(&ch, &entry[1], true)?;
                    out.push(json!({"id":entry[0],"ch":ch,"w":w,"sp":sp,"wIsNaN":w.is_nan(),"spIsNaN":sp.is_nan()}));
                }
            }
            Value::Array(out)
        }
        "parseRenderStyles" => parse_render_styles(
            input["xml"].as_str().ok_or("xml 문자열이 필요합니다.")?,
            input.get("opts"),
        )?,
        "hwpxAppMajor" => json!(hwpx_app_version(input["xml"].as_str(), false)),
        "hwpxAppBuild" => json!(hwpx_app_version(input["xml"].as_str(), true)),
        "hwpFaceToCssStack" => hwp_face_to_css_stack(input["face"].as_str()).into(),
        "docColor" => json!(doc_color(
            input["value"].as_str(),
            input["fallback"].as_str()
        )),
        "defaults" => json!({"char":default_char(),"para":default_para_geom()}),
        _ => return Err("알 수 없는 서식 명령입니다.".into()),
    })
}
