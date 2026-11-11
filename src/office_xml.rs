//! Strict XML reader for structure inspection and future byte-preserving edits.
use crate::input::{is_space, trim};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub const HP: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Debug, Clone, PartialEq)]
pub enum XmlPart {
    Text(String),
    Node(usize),
}
#[derive(Debug, Clone)]
pub struct XmlNode {
    pub name: String,
    pub local: String,
    pub ns: String,
    pub attrs: Map<String, Value>,
    pub namespaces: HashMap<String, String>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub parts: Vec<XmlPart>,
    // Byte offsets are suitable for slicing the original UTF-8 XML. UTF-16
    // offsets separately preserve the positions exposed by the former JS API.
    pub start: usize,
    pub open_end: usize,
    pub close_start: usize,
    pub end: usize,
    pub start_utf16: usize,
    pub open_end_utf16: usize,
    pub close_start_utf16: usize,
    pub end_utf16: usize,
    pub self_closing: bool,
}
impl XmlNode {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).and_then(Value::as_str)
    }
    pub fn is_hp(&self, name: &str) -> bool {
        self.ns == HP && self.local == name
    }
}
#[derive(Debug, Clone)]
pub struct ParsedXml {
    pub xml: String,
    pub root: usize,
    pub nodes: Vec<XmlNode>,
}
impl ParsedXml {
    pub fn children_hp(&self, node: usize, name: &str) -> Vec<usize> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .filter(|&i| self.nodes[i].is_hp(name))
            .collect()
    }
    pub fn ancestor_hp(&self, node: usize, name: &str) -> Option<usize> {
        let mut parent = self.nodes[node].parent;
        while let Some(i) = parent {
            if self.nodes[i].is_hp(name) {
                return Some(i);
            }
            parent = self.nodes[i].parent;
        }
        None
    }
    pub fn text_of(&self, node: usize) -> String {
        if self.nodes[node].is_hp("lineBreak") {
            return "\n".into();
        }
        if self.nodes[node].is_hp("tab") {
            return "\t".into();
        }
        self.nodes[node]
            .parts
            .iter()
            .map(|p| match p {
                XmlPart::Text(s) => s.clone(),
                XmlPart::Node(i) => self.text_of(*i),
            })
            .collect()
    }
    pub fn paragraph_text(&self, node: usize) -> String {
        self.children_hp(node, "run")
            .into_iter()
            .flat_map(|i| self.children_hp(i, "t"))
            .map(|i| self.text_of(i))
            .collect()
    }
}
fn valid_char(cp: u32) -> bool {
    matches!(cp,9|10|13|32..=0xd7ff|0xe000..=0xfffd|0x10000..=0x10ffff)
}
pub fn check_xml_text(text: &str) -> Result<(), String> {
    if text.chars().all(|c| valid_char(c as u32)) {
        Ok(())
    } else {
        Err("XML에 쓸 수 없는 문자입니다.".into())
    }
}
pub fn xml_decode(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest
            .find(';')
            .ok_or("알 수 없거나 잘못된 XML 문자 참조입니다.")?;
        let body = &rest[..end];
        let ch = match body {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let (digits, radix) = if let Some(s) = body.strip_prefix("#x") {
                    (s, 16)
                } else if let Some(s) = body.strip_prefix('#') {
                    (s, 10)
                } else {
                    ("", 10)
                };
                if digits.is_empty()
                    || !digits.chars().all(|c| {
                        if radix == 16 {
                            c.is_ascii_hexdigit()
                        } else {
                            c.is_ascii_digit()
                        }
                    })
                {
                    return Err("알 수 없거나 잘못된 XML 문자 참조입니다.".into());
                }
                let significant = digits.trim_start_matches('0');
                let cp = u32::from_str_radix(
                    if significant.is_empty() {
                        "0"
                    } else {
                        significant
                    },
                    radix,
                )
                .ok();
                cp.filter(|&c| valid_char(c))
                    .and_then(char::from_u32)
                    .ok_or("알 수 없거나 잘못된 XML 문자 참조입니다.")?
            }
        };
        out.push(ch);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    check_xml_text(&out)?;
    Ok(out)
}
pub fn xml_escape(text: &str) -> Result<String, String> {
    check_xml_text(text)?;
    Ok(text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\r', "&#13;"))
}
fn qname(s: &str) -> bool {
    let parts: Vec<_> = s.split(':').collect();
    parts.len() <= 2
        && parts.iter().all(|part| {
            let mut c = part.chars();
            c.next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        })
}
fn split_name(s: &str) -> (&str, &str) {
    s.split_once(':').unwrap_or(("", s))
}
fn normalized(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}
fn attributes(mut rest: &str) -> Result<Vec<(String, String)>, String> {
    let mut attrs = Vec::new();
    let mut seen = HashSet::new();
    while !trim(rest).is_empty() {
        let fail = "잘못되거나 중복된 XML 속성입니다.";
        let next = rest.trim_start_matches(is_space);
        if next.len() == rest.len() {
            return Err(fail.into());
        }
        rest = next;
        let end = rest
            .find(|c: char| is_space(c) || c == '=')
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if !qname(name) || !seen.insert(name.to_owned()) {
            return Err(fail.into());
        }
        rest = rest[end..].trim_start_matches(is_space);
        rest = rest
            .strip_prefix('=')
            .ok_or(fail)?
            .trim_start_matches(is_space);
        let quote = rest
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'))
            .ok_or(fail)?;
        rest = &rest[1..];
        let end = rest.find(quote).ok_or(fail)?;
        let value = &rest[..end];
        if value.contains('<') {
            return Err(fail.into());
        }
        attrs.push((
            name.to_owned(),
            xml_decode(&normalized(value).replace(['\t', '\n'], " "))?,
        ));
        rest = &rest[end + 1..];
    }
    Ok(attrs)
}
fn declaration(body: &str) -> bool {
    let Some(rest) = body.strip_prefix("xml") else {
        return false;
    };
    let Ok(attrs) = attributes(rest) else {
        return false;
    };
    if attrs.first() != Some(&("version".into(), "1.0".into())) {
        return false;
    }
    let mut index = 1;
    if attrs.get(index).is_some_and(|(k, _)| k == "encoding") {
        if !attrs[index].1.eq_ignore_ascii_case("utf-8") {
            return false;
        }
        index += 1;
    }
    if attrs.get(index).is_some_and(|(k, _)| k == "standalone") {
        if !matches!(attrs[index].1.as_str(), "yes" | "no") {
            return false;
        }
        index += 1;
    }
    // Declarations do not allow entity references or normalized line breaks in values.
    index == attrs.len() && !body.contains('&')
}
pub fn parse_office_xml(xml: &str) -> Result<ParsedXml, String> {
    check_xml_text(xml)?;
    let mut nodes: Vec<XmlNode> = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let start = if xml.starts_with('\u{feff}') { 3 } else { 0 };
    let mut pos = start;
    let mut pos16 = usize::from(start > 0);
    let mut declared = false;
    while pos < xml.len() {
        let rest = &xml[pos..];
        let advance;
        if !rest.starts_with('<') {
            advance = rest.find('<').unwrap_or(rest.len());
            let text = &rest[..advance];
            if text.contains("]]>") {
                return Err("잘못된 XML 본문입니다.".into());
            }
            let decoded = xml_decode(&normalized(text))?;
            if let Some(&i) = stack.last() {
                nodes[i].parts.push(XmlPart::Text(decoded));
            } else if !trim(&decoded).is_empty() {
                return Err("XML 루트 밖 글자입니다.".into());
            }
        } else if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").ok_or("잘못된 XML 주석입니다.")?;
            if body[..end].contains("--") || body[..end].ends_with('-') {
                return Err("잘못된 XML 주석입니다.".into());
            }
            advance = end + 7;
        } else if let Some(body) = rest.strip_prefix("<?") {
            let end = body.find("?>").ok_or("잘못된 XML 선언입니다.")?;
            if pos != start || declared || !declaration(&body[..end]) {
                return Err("지원하지 않거나 잘못된 XML 선언·처리 지시입니다.".into());
            }
            declared = true;
            advance = end + 4;
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").ok_or("잘못된 CDATA입니다.")?;
            let i = *stack.last().ok_or("잘못된 CDATA입니다.")?;
            nodes[i].parts.push(XmlPart::Text(normalized(&body[..end])));
            advance = end + 12;
        } else if rest.starts_with("<!") {
            return Err("DTD·외부 엔티티는 지원하지 않습니다.".into());
        } else {
            let mut quote = None;
            let mut end = None;
            for (i, c) in rest[1..].char_indices() {
                if let Some(q) = quote {
                    if c == q {
                        quote = None;
                    }
                } else if matches!(c, '\'' | '"') {
                    quote = Some(c);
                } else if c == '>' {
                    end = Some(i + 1);
                    break;
                }
            }
            let end = end.ok_or("닫히지 않은 XML 태그입니다.")?;
            advance = end + 1;
            let tag = &rest[1..end];
            let next16 = pos16 + rest[..advance].encode_utf16().count();
            if let Some(body) = tag.strip_prefix('/') {
                let name = body.trim_end_matches(is_space);
                if !qname(name) {
                    return Err("잘못된 XML 닫는 태그입니다.".into());
                }
                let i = stack.pop().ok_or("XML 태그 짝이 맞지 않습니다.")?;
                if nodes[i].name != name {
                    return Err("XML 태그 짝이 맞지 않습니다.".into());
                }
                nodes[i].close_start = pos;
                nodes[i].end = pos + advance;
                nodes[i].close_start_utf16 = pos16;
                nodes[i].end_utf16 = next16;
            } else {
                let self_closing = tag.ends_with('/');
                let body = if self_closing {
                    &tag[..tag.len() - 1]
                } else {
                    tag
                };
                let name_end = body.find(is_space).unwrap_or(body.len());
                let name = &body[..name_end];
                if !qname(name) {
                    return Err("잘못된 XML 이름입니다.".into());
                }
                let mut namespaces = stack
                    .last()
                    .map(|&i| nodes[i].namespaces.clone())
                    .unwrap_or_else(|| HashMap::from([("xml".into(), XML.into())]));
                let mut attrs = Map::new();
                for (key, value) in attributes(&body[name_end..])? {
                    if key == "xmlns" || key.starts_with("xmlns:") {
                        let prefix = if key == "xmlns" { "" } else { &key[6..] };
                        if prefix == "xmlns"
                            || (prefix == "xml" && value != XML)
                            || (prefix != "xml" && value == XML)
                            || value == XMLNS
                            || (!prefix.is_empty() && value.is_empty())
                        {
                            return Err("잘못된 XML namespace입니다.".into());
                        }
                        namespaces.insert(prefix.into(), value.clone());
                    }
                    attrs.insert(key, Value::String(value));
                }
                let (prefix, local) = split_name(name);
                if !prefix.is_empty() && namespaces.get(prefix).is_none_or(|s| s.is_empty()) {
                    return Err("선언되지 않은 XML 접두어입니다.".into());
                }
                let mut expanded = HashSet::new();
                for key in attrs.keys() {
                    let (ap, al) = split_name(key);
                    if !ap.is_empty()
                        && ap != "xmlns"
                        && namespaces.get(ap).is_none_or(|s| s.is_empty())
                    {
                        return Err("선언되지 않은 속성 접두어입니다.".into());
                    }
                    let ns = if ap == "xmlns" || key == "xmlns" {
                        XMLNS
                    } else if ap.is_empty() {
                        ""
                    } else {
                        namespaces.get(ap).unwrap()
                    };
                    if !expanded.insert(format!("{ns}|{al}")) {
                        return Err("중복된 XML 속성입니다.".into());
                    }
                }
                let index = nodes.len();
                let parent = stack.last().copied();
                let node = XmlNode {
                    name: name.into(),
                    local: local.into(),
                    ns: namespaces.get(prefix).cloned().unwrap_or_default(),
                    attrs,
                    namespaces,
                    parent,
                    children: Vec::new(),
                    parts: Vec::new(),
                    start: pos,
                    open_end: pos + advance,
                    close_start: pos + advance,
                    end: pos + advance,
                    start_utf16: pos16,
                    open_end_utf16: next16,
                    close_start_utf16: next16,
                    end_utf16: next16,
                    self_closing,
                };
                if let Some(i) = parent {
                    nodes[i].children.push(index);
                    nodes[i].parts.push(XmlPart::Node(index));
                } else {
                    roots.push(index);
                }
                nodes.push(node);
                if !self_closing {
                    stack.push(index);
                }
                if stack.len() > 256 || nodes.len() > 500000 {
                    return Err("XML 구조가 너무 큽니다.".into());
                }
            }
        }
        pos16 += rest[..advance].encode_utf16().count();
        pos += advance;
    }
    if !stack.is_empty() || roots.len() != 1 {
        return Err("완전한 XML 문서가 아닙니다.".into());
    }
    Ok(ParsedXml {
        xml: xml.into(),
        root: roots[0],
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_spans_and_utf16_spans_keep_original_unicode_and_mixed_parts() {
        let xml = format!(
            "\u{feff}<?xml version='1.0' encoding='UTF-8'?><s xmlns:h='{HP}'>한😀<h:p><h:run a='&#9;\t'><h:t>A&amp;<![CDATA[한\r\n😀]]><h:lineBreak/>끝<h:tab/></h:t></h:run></h:p></s>"
        );
        let doc = parse_office_xml(&xml).unwrap();
        let p = doc.nodes.iter().position(|n| n.is_hp("p")).unwrap();
        assert_eq!(doc.paragraph_text(p), "A&한\n😀\n끝\t");
        for n in &doc.nodes {
            assert!(xml[n.start..n.open_end].starts_with('<'));
            assert_eq!(n.start_utf16, xml[..n.start].encode_utf16().count());
            assert_eq!(n.open_end_utf16, xml[..n.open_end].encode_utf16().count());
            assert_eq!(
                n.close_start_utf16,
                xml[..n.close_start].encode_utf16().count()
            );
            assert_eq!(n.end_utf16, xml[..n.end].encode_utf16().count());
        }
        assert_eq!(
            doc.nodes[doc.children_hp(p, "run")[0]].attr("a"),
            Some("\t ")
        );
        assert!(matches!(&doc.nodes[doc.root].parts[0],XmlPart::Text(s) if s=="한😀"));
    }
    #[test]
    fn strict_xml_rejects_unsafe_or_ambiguous_structure() {
        for xml in [
            "<!DOCTYPE a><a/>",
            "<a>&unknown;</a>",
            "<a>&#0;</a>",
            "<a>&#xD800;</a>",
            "<a>&#1114112;</a>",
            "<a>]]></a>",
            "<a/><b/>",
            "<a><b></a>",
            "<x:a/>",
            "<a x:b='1'/>",
            "<a xmlns:xml='wrong'/>",
            "<a xmlns:a='urn:x' xmlns:b='urn:x' a:c='1' b:c='2'/>",
            "<a x='1' x='2'/>",
            "<a x='<'/>",
            "<a><!--bad--comment--></a>",
            "<?target?><a/>",
            " <?xml version='1.0'?><a/>",
            "<?xml version='1.1'?><a/>",
            "<?xml version='1.0' encoding='UTF-16'?><a/>",
        ] {
            assert!(parse_office_xml(xml).is_err(), "{xml}");
        }
        let deep = format!("{}{}", "<a>".repeat(257), "</a>".repeat(257));
        assert!(parse_office_xml(&deep).is_err());
        assert!(parse_office_xml("<?xml version=\"1.0\" standalone='no'?><a xmlns=''/> ").is_ok());
        assert_eq!(
            xml_decode("&#00000000000000000000000000065;&#x1f600;").unwrap(),
            "A😀"
        );
        assert_eq!(xml_escape("&<>\r😀").unwrap(), "&amp;&lt;&gt;&#13;😀");
    }
}
