//! 평문 변환의 기존 Node/.NET 차이를 유지한다. 작성용 엄격 XML과 분리한다.
use crate::input::is_space;
use regex::Regex;
use std::{collections::HashMap, sync::LazyLock};

pub const HWP: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";
pub const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMode {
    Node,
    PowerShell,
}

pub fn net_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{0085}' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

pub fn decode_entities(text: &str) -> String {
    String::from_utf16_lossy(&decode_entities_utf16(text))
}

// JS 문자열은 UTF-16 단위를 유지한다. 서로 다른 참조·텍스트 노드의
// surrogate 쌍도 문단을 완성한 뒤에만 UTF-8로 바꿔야 글자가 손실되지 않는다.
fn decode_entities_utf16(text: &str) -> Vec<u16> {
    static ENTITY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z][A-Za-z0-9]*);").unwrap());
    let mut result = vec![];
    let mut end = 0;
    for c in ENTITY.captures_iter(text) {
        let found = c.get(0).unwrap();
        result.extend(text[end..found.start()].encode_utf16());
        let body = &c[1];
        let point = match body {
            "amp" => Some(38),
            "lt" => Some(60),
            "gt" => Some(62),
            "quot" => Some(34),
            "apos" => Some(39),
            _ => {
                if let Some(hex) = body.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    body.strip_prefix('#').and_then(|s| s.parse().ok())
                }
            }
        };
        match point {
            Some(n) if n <= 0xffff => result.push(n as u16),
            Some(n) if n <= 0x10ffff => {
                result.push(0xd800 + ((n - 0x10000) >> 10) as u16);
                result.push(0xdc00 + ((n - 0x10000) & 0x3ff) as u16);
            }
            _ => result.extend(found.as_str().encode_utf16()),
        }
        end = found.end();
    }
    result.extend(text[end..].encode_utf16());
    result
}

struct Element {
    name: String,
    namespace: String,
    namespaces: HashMap<String, String>,
}
enum Event<'a> {
    Open(&'a str, bool, &'a str),
    Close(&'a str, &'a str),
    Text(Vec<u16>),
}
fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, s)| s)
}
fn name_end(xml: &str, start: usize) -> usize {
    xml[start..]
        .char_indices()
        .find(|(_, c)| is_space(*c) || matches!(c, '/' | '>'))
        .map_or(xml.len(), |(i, _)| start + i)
}

fn scan(xml: &str, mut event: impl FnMut(Event<'_>)) {
    let mut i = 0;
    let mut elements: Vec<Element> = vec![];
    while i < xml.len() {
        let Some(lt) = xml[i..].find('<').map(|n| n + i) else {
            event(Event::Text(decode_entities_utf16(&xml[i..])));
            break;
        };
        if lt > i {
            event(Event::Text(decode_entities_utf16(&xml[i..lt])));
        }
        if xml[lt..].starts_with("<!--") {
            i = xml[lt + 4..]
                .find("-->")
                .map_or(xml.len(), |n| lt + 4 + n + 3);
            continue;
        }
        if xml[lt..].starts_with("<![CDATA[") {
            let end = xml[lt + 9..].find("]]>").map_or(xml.len(), |n| lt + 9 + n);
            event(Event::Text(xml[lt + 9..end].encode_utf16().collect()));
            i = (end + 3).min(xml.len());
            continue;
        }
        if xml[lt..].starts_with("<?") {
            i = xml[lt + 2..]
                .find("?>")
                .map_or(xml.len(), |n| lt + 2 + n + 2);
            continue;
        }
        if xml[lt..].starts_with("<!") {
            let mut start = lt + 2;
            if let Some(br) = xml[start..].find('[').map(|n| start + n)
                && xml[start..].find('>').is_none_or(|n| br < start + n)
            {
                start = xml[br..].find(']').map_or(xml.len(), |n| br + n + 1);
            }
            i = xml[start..].find('>').map_or(xml.len(), |n| start + n + 1);
            continue;
        }
        if xml[lt..].starts_with("</") {
            let j = name_end(xml, lt + 2);
            let name = &xml[lt + 2..j];
            if let Some(index) = elements.iter().rposition(|e| e.name == name) {
                event(Event::Close(local(name), &elements[index].namespace));
                elements.truncate(index);
            }
            i = xml[j..].find('>').map_or(xml.len(), |n| j + n + 1);
            continue;
        }
        let j = name_end(xml, lt + 1);
        let name = &xml[lt + 1..j];
        let mut quote = None;
        let mut end = xml.len();
        for (n, c) in xml[j..].char_indices() {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                if matches!(c, '\'' | '"') {
                    quote = Some(c);
                } else if c == '>' {
                    end = j + n;
                    break;
                }
            }
        }
        let self_closing = xml[j..end].trim_end_matches(is_space).ends_with('/');
        let mut namespaces = elements
            .last()
            .map(|e| e.namespaces.clone())
            .unwrap_or_default();
        // 기존 스캐너의 속성 패턴과 같이 공백 뒤 이름=따옴표 값만 읽는다.
        let attributes = &xml[j..end];
        let mut at = 0;
        while at < attributes.len() {
            let before = at;
            while at < attributes.len() && is_space(attributes[at..].chars().next().unwrap()) {
                at += attributes[at..].chars().next().unwrap().len_utf8();
            }
            if at == before {
                at += attributes[at..].chars().next().unwrap().len_utf8();
                continue;
            }
            let start = at;
            while at < attributes.len()
                && !attributes[at..]
                    .chars()
                    .next()
                    .is_some_and(|c| is_space(c) || matches!(c, '=' | '/' | '>'))
            {
                at += attributes[at..].chars().next().unwrap().len_utf8();
            }
            let key = &attributes[start..at];
            while at < attributes.len() && is_space(attributes[at..].chars().next().unwrap()) {
                at += attributes[at..].chars().next().unwrap().len_utf8();
            }
            if attributes.as_bytes().get(at) != Some(&b'=') {
                continue;
            }
            at += 1;
            while at < attributes.len() && is_space(attributes[at..].chars().next().unwrap()) {
                at += attributes[at..].chars().next().unwrap().len_utf8();
            }
            let Some(&q) = attributes
                .as_bytes()
                .get(at)
                .filter(|c| matches!(c, b'\'' | b'"'))
            else {
                continue;
            };
            at += 1;
            let Some(length) = attributes[at..].find(q as char) else {
                break;
            };
            if key == "xmlns" || key.starts_with("xmlns:") {
                namespaces.insert(
                    key.strip_prefix("xmlns:").unwrap_or("").into(),
                    decode_entities(&attributes[at..at + length]),
                );
            }
            at += length + 1;
        }
        let prefix = name.split_once(':').map_or("", |(s, _)| s);
        let namespace = namespaces.get(prefix).cloned().unwrap_or_default();
        event(Event::Open(local(name), self_closing, &namespace));
        if !self_closing {
            elements.push(Element {
                name: name.into(),
                namespace,
                namespaces,
            });
        }
        i = if end < xml.len() { end + 1 } else { end };
    }
}

fn node_paragraphs(xml: &str, namespace: &str, docx: bool) -> Vec<String> {
    let mut paragraphs: Vec<Vec<u16>> = vec![];
    let mut open: Vec<usize> = vec![];
    let mut stack: Vec<(String, String)> = vec![];
    let mut depth = 0usize;
    let mut buffer = Vec::new();
    scan(xml, |event| match event {
        Event::Open(name, self_closing, ns) => {
            if !self_closing {
                stack.push((name.into(), ns.into()));
            }
            if depth > 0 {
                if !self_closing {
                    depth += 1;
                }
                return;
            }
            if ns != namespace {
                return;
            }
            match name {
                "p" => {
                    paragraphs.push(Vec::new());
                    if !self_closing {
                        open.push(paragraphs.len() - 1);
                    }
                }
                "t" => {
                    buffer.clear();
                    if !self_closing {
                        depth = 1;
                    }
                }
                "tab" | "br" | "cr" if docx => {
                    for &index in &open {
                        paragraphs[index].push(if name == "tab" { 9 } else { 32 });
                    }
                }
                _ => {}
            }
        }
        Event::Close(name, ns) => {
            let Some(found) = stack.iter().rposition(|(a, b)| a == name && b == ns) else {
                return;
            };
            let popped = stack.len() - found;
            stack.truncate(found);
            if depth > 0 {
                depth = depth.saturating_sub(popped);
                if depth == 0 {
                    for &index in &open {
                        paragraphs[index].extend_from_slice(&buffer);
                    }
                    buffer.clear();
                }
                return;
            }
            if ns == namespace && name == "p" {
                open.pop();
            }
        }
        Event::Text(text) => {
            if depth > 0 {
                buffer.extend(text);
            }
        }
    });
    paragraphs
        .iter()
        .map(|p| String::from_utf16_lossy(p))
        .collect()
}

// XmlDocument는 문자 참조의 UTF-16 단위와 일부 제어문자를 허용한다.
// 엄격한 XML 파서에 전달할 때만 인코딩하고 최종 문단에서 복원한다.
// 원래 표식 문자도 이중화하므로 사용자 본문과 충돌하지 않는다.
fn encode_dotnet_references(xml: &str) -> String {
    const MARKER: char = '\u{e000}';
    static PART: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?s)<!\[CDATA\[.*?\]\]>|<!--.*?-->|<\?.*?\?>|&#(?:x[0-9a-fA-F]+|[0-9]+);|\x{e000}",
        )
        .unwrap()
    });
    PART.replace_all(xml, |c: &regex::Captures<'_>| {
        let value = &c[0];
        if let Some(reference) = value.strip_prefix("&#").and_then(|s| s.strip_suffix(';')) {
            let point = if let Some(hex) = reference.strip_prefix('x') {
                u32::from_str_radix(hex, 16).ok()
            } else {
                reference.parse::<u32>().ok()
            };
            if let Some(n) = point {
                if matches!(n, 0..=8 | 11..=12 | 14..=31 | 0xd800..=0xdfff | 0xfffe..=0xffff) {
                    return format!("{MARKER}x{n:x};");
                }
                if n == MARKER as u32 {
                    return format!("{MARKER}{MARKER}");
                }
            }
        }
        value.replace(MARKER, "\u{e000}\u{e000}")
    })
    .into_owned()
}

fn decode_dotnet_references(text: &str) -> String {
    let mut result = Vec::<u16>::new();
    let mut rest = text;
    while let Some(at) = rest.find('\u{e000}') {
        result.extend(rest[..at].encode_utf16());
        rest = &rest[at + '\u{e000}'.len_utf8()..];
        if let Some(after) = rest.strip_prefix('\u{e000}') {
            result.push(0xe000);
            rest = after;
        } else if rest.starts_with('x')
            && let Some(end) = rest.find(';')
            && let Ok(unit) = u16::from_str_radix(&rest[1..end], 16)
        {
            result.push(unit);
            rest = &rest[end + 1..];
        } else {
            // 표식은 항상 짝으로 인코딩하지만 불완전해도 문자를 버리지 않는다.
            result.push(0xe000);
        }
    }
    result.extend(rest.encode_utf16());
    String::from_utf16_lossy(&result)
}

fn powershell_paragraphs(xml: &str, namespace: &str, docx: bool) -> Result<Vec<String>, String> {
    let encoded = encode_dotnet_references(xml);
    let prepared = crate::office_dtd::prepare_powershell_xml(&encoded)?;
    let xml = prepared.as_str();
    let parsed = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|e| format!("XML을 읽을 수 없습니다: {e}"))?;
    let normalized = without_ignorable_whitespace(xml, &parsed)?;
    drop(parsed);
    let document = roxmltree::Document::parse_with_options(
        &normalized,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|e| format!("XML을 읽을 수 없습니다: {e}"))?;
    let mut paragraphs = vec![];
    for paragraph in document
        .descendants()
        .filter(|n| n.has_tag_name((namespace, "p")))
    {
        let mut text = String::new();
        for node in paragraph
            .descendants()
            .skip(1)
            .filter(|n| n.is_element() && n.tag_name().namespace() == Some(namespace))
        {
            match node.tag_name().name() {
                "t" => {
                    for child in node.descendants().filter(|n| n.is_text()) {
                        let value = child.text().unwrap_or("");
                        text.push_str(value);
                    }
                }
                "tab" if docx => text.push('\t'),
                "br" | "cr" if docx => text.push(' '),
                _ => {}
            }
        }
        paragraphs.push(decode_dotnet_references(&text));
    }
    Ok(paragraphs)
}

// 일반 공백과 CDATA를 DOM이 합치기 전에 구분한다. xml:space는 상속한다.
fn without_ignorable_whitespace(
    xml: &str,
    document: &roxmltree::Document<'_>,
) -> Result<String, String> {
    const XML: &str = "http://www.w3.org/XML/1998/namespace";
    let mut spaces = HashMap::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        if node
            .attribute((XML, "space"))
            .is_some_and(|s| !matches!(s, "default" | "preserve"))
        {
            return Err("잘못된 xml:space 속성입니다.".into());
        }
        let preserve = node
            .attribute((XML, "space"))
            .map(|s| s == "preserve")
            .unwrap_or_else(|| {
                node.parent()
                    .and_then(|p| spaces.get(&p.range().start))
                    .copied()
                    .unwrap_or(false)
            });
        spaces.insert(node.range().start, preserve);
    }
    let mut output = String::with_capacity(xml.len());
    let mut stack = vec![false];
    let mut at = 0;
    while at < xml.len() {
        if xml.as_bytes()[at] != b'<' {
            let end = xml[at..].find('<').map_or(xml.len(), |n| at + n);
            let text = &xml[at..end];
            if *stack.last().unwrap()
                || !decode_entities(text)
                    .chars()
                    .all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
            {
                output.push_str(text);
            }
            at = end;
            continue;
        }
        let rest = &xml[at..];
        let skip = if rest.starts_with("<!--") {
            Some((4, "-->"))
        } else if rest.starts_with("<![CDATA[") {
            Some((9, "]]>"))
        } else if rest.starts_with("<?") {
            Some((2, "?>"))
        } else {
            None
        };
        if let Some((start, ending)) = skip {
            let end = at
                + start
                + rest[start..]
                    .find(ending)
                    .ok_or("닫히지 않은 XML 요소입니다.")?
                + ending.len();
            output.push_str(&xml[at..end]);
            at = end;
            continue;
        }
        let mut end = at + 1;
        let mut quote = None;
        let mut brackets = 0usize;
        while end < xml.len() {
            let b = xml.as_bytes()[end];
            if quote == Some(b) {
                quote = None;
            } else if quote.is_none() {
                if matches!(b, b'\'' | b'"') {
                    quote = Some(b);
                } else if rest.starts_with("<!") && b == b'[' {
                    brackets += 1;
                } else if rest.starts_with("<!") && b == b']' {
                    brackets = brackets.saturating_sub(1);
                } else if b == b'>' && brackets == 0 {
                    break;
                }
            }
            end += 1;
        }
        if end == xml.len() {
            return Err("닫히지 않은 XML 태그입니다.".into());
        }
        output.push_str(&xml[at..=end]);
        if rest.starts_with("</") {
            stack.pop();
        } else if !rest.starts_with("<!") && xml.as_bytes()[end - 1] != b'/' {
            stack.push(spaces.get(&at).copied().unwrap_or(*stack.last().unwrap()));
        }
        at = end + 1;
    }
    Ok(output)
}

pub fn paragraphs(xml: &str, docx: bool, mode: TextMode) -> Result<Vec<String>, String> {
    let namespace = if docx { WORD } else { HWP };
    match mode {
        TextMode::Node => Ok(node_paragraphs(xml, namespace, docx)),
        TextMode::PowerShell => powershell_paragraphs(xml, namespace, docx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn node_keeps_utf16_pairs_across_references_and_text_events() {
        for body in [
            "<t>&#55357;&#56832;</t>",
            "<t>&#xD83D;<!-- 분리 -->&#xDE00;</t>",
            "<t>&#55357;</t><t>&#56832;</t>",
            "<t>&#x1F600;</t>",
        ] {
            let xml = format!("<p xmlns='{HWP}'>{body}</p>");
            for mode in [TextMode::Node, TextMode::PowerShell] {
                assert_eq!(paragraphs(&xml, false, mode).unwrap(), vec!["😀"]);
            }
        }
        let xml = format!("<p xmlns='{HWP}'><t>&#55357;A&#56832;</t></p>");
        assert_eq!(
            paragraphs(&xml, false, TextMode::Node).unwrap(),
            vec!["�A�"]
        );
        assert_eq!(
            paragraphs(&xml, false, TextMode::PowerShell).unwrap(),
            vec!["�A�"]
        );
    }
    #[test]
    fn dotnet_reference_encoding_keeps_literal_markers_cdata_and_controls() {
        let text = "\u{e000}xD800; &#57344;xD800; <![CDATA[&#55357;\u{e000}]]> &#0; &#11; &#65534;";
        let xml = format!("<p xmlns='{HWP}'><t>{text}</t></p>");
        assert_eq!(
            paragraphs(&xml, false, TextMode::PowerShell).unwrap(),
            vec!["\u{e000}xD800; \u{e000}xD800; &#55357;\u{e000} \0 \u{b} \u{fffe}"]
        );
    }
    #[test]
    fn dtd_and_transport_preserve_late_refs_whitespace_and_entity_line_endings() {
        for (declaration, body, expected) in [
            ("<!ENTITY s ' '>", "<t>A</t><t>&s;</t><t>B</t>", "AB"),
            (
                "<!ATTLIST t xml:space (default|preserve) 'preserve'>",
                "<t>A</t><t> </t><t>B</t>",
                "A B",
            ),
            (
                "<!ENTITY s '&#38;#xE000;x41;'>",
                "<t>&s;</t>",
                "\u{e000}x41;",
            ),
            ("<!ENTITY s '&#38;#55357;&#38;#56832;'>", "<t>&s;</t>", "😀"),
            ("<!ENTITY s '&#55357;&#56832;'>", "<t>&s;</t>", "😀"),
            ("<!ENTITY s '&#13;'>", "<t>A&s;B</t>", "A\rB"),
            ("<!ENTITY s '<![CDATA[&#13;]]>'>", "<t>A&s;B</t>", "A\rB"),
            ("<!ENTITY s '&amp;#55357;'>", "<t>&s;</t>", "&#55357;"),
        ] {
            let xml = format!("<!DOCTYPE p [{declaration}]><p xmlns='{HWP}'>{body}</p>");
            assert_eq!(
                paragraphs(&xml, false, TextMode::PowerShell).unwrap(),
                vec![expected],
                "{declaration}"
            );
        }
    }
    #[test]
    fn namespaces_and_nested_paragraphs_keep_existing_text_order() {
        let xml = format!(
            "<root xmlns:x='{HWP}' xmlns:f='foreign'><x:p><x:t>앞</x:t><f:t>제외</f:t><x:p><x:t>안</x:t></x:p><x:t>뒤</x:t></x:p></root>"
        );
        for mode in [TextMode::Node, TextMode::PowerShell] {
            assert_eq!(paragraphs(&xml, false, mode).unwrap(), vec!["앞안뒤", "안"]);
        }
    }
    #[test]
    fn modes_keep_whitespace_and_undefined_entities_contracts() {
        let xml = format!("<p xmlns='{HWP}'><t>앞</t><t> </t><t>뒤</t></p>");
        assert_eq!(
            paragraphs(&xml, false, TextMode::Node).unwrap(),
            vec!["앞 뒤"]
        );
        assert_eq!(
            paragraphs(&xml, false, TextMode::PowerShell).unwrap(),
            vec!["앞뒤"]
        );
        let unknown = xml.replace("앞", "&unknown;");
        assert!(paragraphs(&unknown, false, TextMode::PowerShell).is_err());
        assert!(paragraphs(&unknown, false, TextMode::Node).unwrap()[0].starts_with("&unknown;"));
    }
}
