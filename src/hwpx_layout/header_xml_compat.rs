//! Read-only layout compatibility reader for the fixed kordoc/xmldom contract.
//! Derived behavior from kordoc (MIT), Copyright 2026 chrisryugj;
//! see data/kordoc-LICENSE, data/kordoc-NOTICE, data/kordoc-PROVENANCE.json.
//! Editing/validation XML parsers remain strict. This never fetches or expands
//! external declarations, and never modifies the original source document.
use super::style_dom::{ParsedXml, XmlPart, normalize_xml_lines, parse_style_xml};
#[cfg(test)]
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
type NamespaceMap = HashMap<Arc<str>, usize>;
struct NamespaceUris {
    ids: HashMap<Arc<str>, usize>,
}
impl NamespaceUris {
    fn new() -> Self {
        Self {
            ids: HashMap::from([
                (Arc::from(""), 0),
                (Arc::from("http://www.w3.org/XML/1998/namespace"), 1),
            ]),
        }
    }
    fn intern(&mut self, uri: String) -> usize {
        if let Some(&id) = self.ids.get(uri.as_str()) {
            return id;
        }
        let id = self.ids.len();
        self.ids.insert(Arc::from(uri), id);
        id
    }
}
const MAX_RECOVERY_WORK: usize = 16_000_000;
fn charge(work: &mut usize, amount: usize) -> Result<(), String> {
    *work = work
        .checked_sub(amount)
        .ok_or("머리말 호환 복구 작업 한도를 넘었습니다.")?;
    Ok(())
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}
fn error() -> String {
    "머리말 XML의 지원하지 않는 손상 구조입니다.".into()
}
/// Preserve undefined entity references and bare ampersands as literal text.
/// Other numeric references are still validated by the strict XML parser.
fn repair_entities(text: &str, null_marker: Option<char>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let standard = ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"]
            .iter()
            .find(|entity| rest.starts_with(**entity));
        if let Some(entity) = standard {
            out.push_str(entity);
            rest = &rest[entity.len()..];
            continue;
        }
        if rest.starts_with("&#") {
            // Scan only this reference's numeric prefix. Searching the entire
            // suffix for ';' for every '&#' makes incomplete input quadratic.
            let hex = rest.as_bytes().get(2) == Some(&b'x');
            let begin = if hex { 3 } else { 2 };
            let digits = rest.as_bytes()[begin..]
                .iter()
                .take_while(|b| {
                    if hex {
                        b.is_ascii_hexdigit()
                    } else {
                        b.is_ascii_digit()
                    }
                })
                .count();
            let end = begin + digits;
            if digits > 0 && rest.as_bytes().get(end) == Some(&b';') {
                if let Some(marker) =
                    null_marker.filter(|_| rest.as_bytes()[begin..end].iter().all(|b| *b == b'0'))
                {
                    out.push(marker);
                } else {
                    out.push_str(&rest[..=end]);
                }
                rest = &rest[end + 1..];
                continue;
            }
        }
        out.push_str("&amp;");
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}
fn quoted(value: &str, null_marker: Option<char>) -> String {
    repair_entities(value, null_marker).replace('"', "&quot;")
}
fn decoded_attribute(value: &str, null_marker: Option<char>) -> Result<String, String> {
    let xml = format!("<s a=\"{}\"/>", quoted(value, null_marker));
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("머리말 속성 오류: {e}"))?;
    Ok(doc.root_element().attribute("a").unwrap_or("").to_owned())
}

/// A transport character absent both literally and as a numeric reference.
/// Only the tolerant header reader restores NUL after structural parsing.
/// CDATA, comments and escaped reference text are never converted to NUL.
fn numeric_null_marker(xml: &str) -> Result<Option<char>, String> {
    let mut used = HashSet::new();
    let mut has_null = false;
    for (at, _) in xml.match_indices("&#") {
        let tail = &xml[at + 2..];
        let (tail, radix) = tail.strip_prefix('x').map_or((tail, 10), |s| (s, 16));
        let digits = tail
            .bytes()
            .take_while(|b| {
                if radix == 16 {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 || tail.as_bytes().get(digits) != Some(&b';') {
            continue;
        }
        if let Ok(value) = u32::from_str_radix(&tail[..digits], radix) {
            has_null |= value == 0;
            if let Some(ch) = char::from_u32(value) {
                used.insert(ch);
            }
        }
    }
    if !has_null {
        return Ok(None);
    }
    used.extend(xml.chars());
    (0xe000..=0xf8ff)
        .chain(0xf0000..=0xffffd)
        .chain(0x100000..=0x10fffd)
        .filter_map(char::from_u32)
        .find(|ch| !used.contains(ch))
        .map(Some)
        .ok_or_else(|| "머리말 NUL 문자 전달 공간 한도를 넘었습니다.".into())
}
#[derive(Debug)]
struct Attribute {
    name: String,
    value: String,
}
struct StartTag {
    name: String,
    attrs: Vec<Attribute>,
    closed: bool,
    end: usize,
}
struct Restoration {
    names: HashMap<String, String>,
    collisions: Vec<(usize, usize)>,
}
fn start_tag(xml: &str, start: usize) -> Result<StartTag, String> {
    let bytes = xml.as_bytes();
    let mut at = start + 1;
    let begin = at;
    while bytes
        .get(at)
        .is_some_and(|b| !xml_space(*b) && !matches!(b, b'/' | b'>'))
    {
        at += 1;
    }
    if at == begin {
        return Err(error());
    }
    let name = xml[begin..at].to_owned();
    let mut attrs = Vec::new();
    let mut names = HashSet::new();
    loop {
        while bytes.get(at).is_some_and(|b| xml_space(*b)) {
            at += 1;
        }
        if bytes.get(at) == Some(&b'>') {
            return Ok(StartTag {
                name,
                attrs,
                closed: false,
                end: at + 1,
            });
        }
        if bytes.get(at) == Some(&b'/') && bytes.get(at + 1) == Some(&b'>') {
            return Ok(StartTag {
                name,
                attrs,
                closed: true,
                end: at + 2,
            });
        }
        if at >= bytes.len() {
            return Err(error());
        }
        let begin = at;
        while bytes
            .get(at)
            .is_some_and(|b| !xml_space(*b) && !matches!(b, b'=' | b'/' | b'>'))
        {
            at += 1;
        }
        if at == begin {
            return Err(error());
        }
        let key = xml[begin..at].to_owned();
        if !names.insert(key.clone()) {
            return Err(format!("중복 머리말 속성: {key}"));
        }
        while bytes.get(at).is_some_and(|b| xml_space(*b)) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            attrs.push(Attribute {
                name: key.clone(),
                value: key,
            });
            continue;
        }
        at += 1;
        while bytes.get(at).is_some_and(|b| xml_space(*b)) {
            at += 1;
        }
        let Some(&first) = bytes.get(at) else {
            return Err(error());
        };
        let value;
        if matches!(first, b'\'' | b'"') {
            at += 1;
            let begin = at;
            while bytes.get(at).is_some_and(|b| *b != first) {
                at += 1;
            }
            if at >= bytes.len() {
                return Err(error());
            }
            value = xml[begin..at].to_owned();
            at += 1;
        } else {
            let begin = at;
            while bytes
                .get(at)
                .is_some_and(|b| !xml_space(*b) && !matches!(b, b'>' | b'\'' | b'"'))
            {
                at += 1;
            }
            let mut end = at;
            if bytes.get(at) == Some(&b'>') && end > begin && bytes[end - 1] == b'/' {
                end -= 1;
                at -= 1;
            }
            if begin == end {
                return Err(error());
            }
            value = xml[begin..end].to_owned();
            if bytes.get(at).is_some_and(|b| matches!(b, b'\'' | b'"')) {
                at += 1;
            }
        }
        if value.contains('<') {
            return Err(error());
        }
        attrs.push(Attribute { name: key, value });
    }
}
fn declaration_end(xml: &str, start: usize) -> Result<usize, String> {
    let mut quote = None;
    let mut depth = 0usize;
    for (offset, ch) in xml[start..].char_indices() {
        if let Some(expected) = quote {
            if ch == expected {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '>' if depth == 0 => return Ok(start + offset + 1),
            _ => {}
        }
    }
    Err(error())
}
fn validate_declaration(declaration: &str) -> Result<(), String> {
    // Validate only the declaration header in an isolated document. The actual
    // subset and document text are never handed to a DTD-expanding parser.
    // No resolver is installed; SYSTEM/PUBLIC identifiers are only syntax.
    let mut quote = None;
    let subset = declaration.char_indices().find_map(|(i, ch)| {
        if let Some(expected) = quote {
            if ch == expected {
                quote = None;
            }
            return None;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '[' => return Some(i),
            _ => {}
        }
        None
    });
    let header = if let Some(at) = subset {
        let close = declaration.rfind(']').ok_or_else(error)?;
        if declaration[close + 1..].trim_matches([' ', '\t', '\r', '\n']) != ">" {
            return Err(error());
        }
        format!("{}>", &declaration[..at])
    } else {
        declaration.to_owned()
    };
    let shell = format!("{header}<header_validation/>");
    roxmltree::Document::parse_with_options(
        &shell,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 2,
            entity_resolver: None,
        },
    )
    .map_err(|e| format!("머리말 DOCTYPE 오류: {e}"))?;
    Ok(())
}
fn recover(xml: &str) -> Result<ParsedXml, String> {
    recover_with_work_limit(xml, MAX_RECOVERY_WORK)
}
fn recover_with_work_limit(xml: &str, mut work: usize) -> Result<ParsedXml, String> {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let null_marker = numeric_null_marker(xml)?;
    let mut normalized = String::with_capacity(xml.len());
    let mut stack: Vec<(String, Arc<NamespaceMap>)> = Vec::new();
    let mut namespace_uris = NamespaceUris::new();
    let mut renames: Vec<Restoration> = Vec::new();
    let mut roots = 0;
    let mut doctype_seen = false;
    let mut at = 0;
    while at < xml.len() {
        if !xml[at..].starts_with('<') {
            let end = xml[at..].find('<').map(|n| at + n).unwrap_or(xml.len());
            if !stack.is_empty() {
                normalized.push_str(&repair_entities(&xml[at..end], null_marker));
            } else if xml[at..end].bytes().all(xml_space) {
                normalized.push_str(&xml[at..end]);
            }
            at = end;
            continue;
        }
        if xml[at..].starts_with("<!--") {
            let end = xml[at + 4..]
                .find("-->")
                .map(|n| at + 4 + n + 3)
                .ok_or_else(error)?;
            normalized.push_str(&xml[at..end]);
            at = end;
            continue;
        }
        if xml[at..].starts_with("<![CDATA[") {
            if stack.is_empty() {
                return Err(error());
            }
            let end = xml[at + 9..]
                .find("]]>")
                .map(|n| at + 9 + n + 3)
                .ok_or_else(error)?;
            normalized.push_str(&xml[at..end]);
            at = end;
            continue;
        }
        if xml[at..].starts_with("<?") {
            let target = xml[at + 2..]
                .split(|ch: char| ch.is_ascii_whitespace() || ch == '?')
                .next()
                .unwrap_or("");
            if target == "xml" && at != 0 {
                return Err(error());
            }
            let end = xml[at + 2..]
                .find("?>")
                .map(|n| at + 2 + n + 2)
                .ok_or_else(error)?;
            normalized.push_str(&xml[at..end]);
            at = end;
            continue;
        }
        if xml[at..].starts_with("<!DOCTYPE") {
            if !stack.is_empty() || roots > 0 || doctype_seen {
                return Err(error());
            }
            let tail = &xml[at + 9..];
            if !tail.as_bytes().first().is_some_and(|b| xml_space(*b)) {
                return Err(error());
            }
            let name = tail.trim_start_matches([' ', '\t', '\n', '\r']);
            if name.is_empty() || name.starts_with(['>', '[', '\'', '"']) {
                return Err(error());
            }
            doctype_seen = true;
            let end = declaration_end(xml, at + 9)?;
            validate_declaration(&xml[at..end])?;
            at = end;
            continue;
        }
        if xml[at..].starts_with("</") {
            let end = xml[at..].find('>').map(|n| at + n + 1).ok_or_else(error)?;
            let name = xml[at + 2..end - 1].trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
            let Some((open, _)) = stack.pop() else {
                return Err(error());
            };
            if name != open {
                return Err(error());
            }
            normalized.push_str(&xml[at..end]);
            at = end;
            continue;
        }
        if xml[at..].starts_with("<!") {
            return Err(error());
        }
        let tag = start_tag(xml, at)?;
        charge(&mut work, tag.attrs.len() + 1)?;
        if stack.is_empty() {
            roots += 1;
            if roots > 1 {
                return Err(error());
            }
        }
        if stack.len() > 200 || renames.len() >= 500_000 {
            return Err("머리말 XML 구조 한도를 넘었습니다.".into());
        }
        let mut ns = stack
            .last()
            .map(|(_, ns)| ns.clone())
            .unwrap_or_else(|| Arc::new(HashMap::from([(Arc::from("xml"), 1)])));
        for attr in &tag.attrs {
            let prefix = if attr.name == "xmlns" {
                Some("")
            } else {
                attr.name.strip_prefix("xmlns:")
            };
            if let Some(prefix) = prefix {
                // Inherited scopes share the map. Copy only for a declaration;
                // shared keys and integer URI identities keep each copied
                // binding constant-size, without repeatedly hashing long URIs.
                if Arc::strong_count(&ns) > 1 {
                    charge(&mut work, ns.len())?;
                }
                let uri = namespace_uris.intern(decoded_attribute(&attr.value, null_marker)?);
                Arc::make_mut(&mut ns).insert(Arc::from(prefix), uri);
            }
        }
        let names: HashSet<_> = tag.attrs.iter().map(|attr| attr.name.as_str()).collect();
        let mut expanded = HashMap::new();
        let mut changed = HashMap::new();
        let mut collisions = Vec::new();
        let mut serial = 0usize;
        normalized.push('<');
        normalized.push_str(&tag.name);
        for (attr_index, attr) in tag.attrs.iter().enumerate() {
            let mut emitted = attr.name.clone();
            if attr.name != "xmlns" && !attr.name.starts_with("xmlns:") {
                let key = if let Some((prefix, local)) = attr.name.split_once(':') {
                    if local.contains(':') {
                        None
                    } else {
                        ns.get(prefix).map(|uri| (*uri, local.to_owned()))
                    }
                } else {
                    Some((0, attr.name.clone()))
                };
                if let Some(key) = key {
                    let old = expanded.get(&key).copied();
                    if old.is_none() {
                        expanded.insert(key, attr_index);
                    }
                    if let Some(first) = old {
                        loop {
                            emitted = format!("__my_system_header_attr_{serial}");
                            serial += 1;
                            if !names.contains(emitted.as_str()) && !changed.contains_key(&emitted)
                            {
                                break;
                            }
                        }
                        changed.insert(emitted.clone(), attr.name.clone());
                        collisions.push((first, attr_index));
                    }
                }
            }
            normalized.push(' ');
            normalized.push_str(&emitted);
            normalized.push_str("=\"");
            normalized.push_str(&quoted(&attr.value, null_marker));
            normalized.push('"');
        }
        renames.push(Restoration {
            names: changed,
            collisions,
        });
        if tag.closed {
            normalized.push_str("/>");
        } else {
            normalized.push('>');
            stack.push((tag.name, ns));
        }
        at = tag.end;
    }
    if !stack.is_empty() || roots != 1 {
        return Err(error());
    }
    let mut doc = parse_style_xml(&normalized)?;
    if doc.nodes.len() != renames.len() {
        return Err(error());
    }
    for (node, Restoration { names, collisions }) in doc.nodes.iter_mut().zip(renames) {
        if !names.is_empty() {
            let mut attrs: Vec<_> = node
                .attributes()
                .iter()
                .map(|(name, value)| Some((names.get(name).unwrap_or(name).clone(), value.clone())))
                .collect();
            // xmldom replaces an expanded-name collision in the first slot,
            // retaining the final QName/value rather than keeping two attrs.
            for (first, later) in collisions {
                attrs[first] = attrs[later].take();
            }
            node.replace_attributes(attrs.into_iter().flatten().collect());
        }
        if let Some(marker) = null_marker {
            let attrs = node
                .attributes()
                .iter()
                .map(|(name, value)| (name.clone(), value.replace(marker, "\0")))
                .collect();
            node.replace_attributes(attrs);
            for part in &mut node.parts {
                if let XmlPart::Text(text) = part {
                    *text = text.replace(marker, "\0");
                }
            }
        }
    }
    Ok(doc)
}
/// Strict-first parsing, retaining the original header entry-point name. Layout
/// loading may reuse this reader after the separate strict writing validation.
/// The fallback only changes the confirmed read-only reader
/// cases: literal entities/ampersands, unquoted values, outer text, declarations
/// ignored by the old reader, and distinct QNames sharing one expanded name.
pub fn parse_header_xml(xml: &str) -> Result<ParsedXml, String> {
    let normalized = normalize_xml_lines(xml);
    let xml = normalized.as_ref();
    match parse_style_xml(xml) {
        Ok(doc) => Ok(doc),
        Err(_) => recover(xml),
    }
}
#[cfg(test)]
fn dom_report(doc: &ParsedXml) -> Value {
    let nodes=doc.nodes.iter().map(|node|{
        let mut parts:Vec<Value>=Vec::new();
        for part in &node.parts{
            match part{
                XmlPart::Node(id)=>parts.push(json!({"node":id})),
                XmlPart::Text(text)=>{
                    if let Some(last)=parts.last().and_then(Value::as_str).map(str::to_owned){
                        *parts.last_mut().unwrap()=json!(last+text);
                    }else{parts.push(json!(text));}
                }
            }
        }
        json!({"name":node.name,"local":node.local,"parent":node.parent,"attrs":node.attributes(),"children":node.children,"parts":parts})
    }).collect::<Vec<_>>();
    json!({"root":doc.root,"nodes":nodes})
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &Value) -> Result<Value, String> {
    match operation {
        "parseHeader" => Ok(dom_report(&parse_header_xml(
            input["xml"].as_str().ok_or("XML 문자열이 없습니다.")?,
        )?)),
        "parseRenderStyles" => super::head_styles::run(operation, input),
        _ => Err("알 수 없는 머리말 호환 연산입니다.".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_null_is_preserved_only_in_header_values() {
        let xml = "<h a='&#0;&#57344;'>&#x0;<![CDATA[&#0;]]></h>";
        let doc = parse_header_xml(xml).unwrap();
        assert_eq!(doc.nodes[0].attr("a"), Some("\0\u{e000}"));
        assert!(parse_style_xml(xml).is_err());
        assert!(crate::office_xml::parse_office_xml(xml).is_err());
        assert!(parse_header_xml("<h a='\0'/>").is_err());
    }
    #[test]
    fn equal_namespace_values_reuse_one_numeric_identity() {
        let mut uris = NamespaceUris::new();
        assert_eq!(uris.intern(String::new()), 0);
        assert_eq!(
            uris.intern("http://www.w3.org/XML/1998/namespace".into()),
            1
        );
        let first = uris.intern("urn:shared".into());
        assert_eq!(first, uris.intern(["urn:", "shared"].concat()));
        assert_ne!(first, uris.intern("urn:other".into()));
        assert_eq!(uris.ids.len(), 4);
    }
    #[test]
    fn incomplete_references_remain_literal_with_linear_prefix_scanning() {
        let value = "&#".repeat(6_000);
        assert_eq!(repair_entities(&value, None), "&amp;#".repeat(6_000));
        assert_eq!(
            repair_entities("&#65; &#x41; &#xno; &#65", None),
            "&#65; &#x41; &amp;#xno; &amp;#65"
        );
    }
    #[test]
    fn declaration_headers_and_boolean_attributes_keep_old_outcomes() {
        for xml in ["<!DOCTYPE 1h><h/>", "<!DOCTYPE h junk junk><h/>"] {
            assert!(parse_header_xml(xml).is_err());
        }
        let doc = parse_header_xml("<h enabled last='ok'/>").unwrap();
        assert_eq!(
            doc.nodes[0].attributes()[0],
            ("enabled".into(), "enabled".into())
        );
        let doc =
            parse_header_xml("<!DOCTYPE h [<!ENTITY x SYSTEM 'file:///not-read'>]><h id='&x;'/>")
                .unwrap();
        assert_eq!(doc.nodes[0].attr("id"), Some("&x;"));
    }
    #[test]
    fn inherited_namespaces_share_storage_and_copy_work_is_bounded() {
        let declarations = (0..20)
            .map(|i| format!(" xmlns:p{i}='urn:{i}'"))
            .collect::<String>();
        let inherited = format!("junk<h{declarations}>{}</h>", "<c/>".repeat(20));
        assert!(recover_with_work_limit(&inherited, 100).is_ok());
        let changed = format!(
            "junk<h{declarations}>{}</h>",
            "<c xmlns:q='urn:q'/>".repeat(20)
        );
        assert!(
            recover_with_work_limit(&changed, 100)
                .err()
                .unwrap()
                .contains("작업 한도")
        );
        assert!(recover(&changed).is_ok());
    }
    #[test]
    fn duplicate_expanded_names_keep_last_qname_in_first_position() {
        let doc=parse_header_xml("<h xmlns:a='urn:u' xmlns:b='urn:u' __my_system_header_attr_0='keep' a:x='first' b:x='second'><c xmlns:b='urn:v' a:x='A' b:x='B'/></h>").unwrap();
        assert_eq!(doc.nodes[0].attr("a:x"), None);
        assert_eq!(doc.nodes[0].attr("b:x"), Some("second"));
        assert_eq!(doc.nodes[0].attributes()[3].0, "b:x");
        assert_eq!(doc.nodes[0].attr("__my_system_header_attr_0"), Some("keep"));
        assert_eq!(doc.nodes[1].attr("b:x"), Some("B"));
    }
    #[test]
    fn repairs_are_literal_and_do_not_expand_declared_entities() {
        let doc = parse_header_xml(
            "junk<!DOCTYPE h [<!ENTITY x 'expanded'>]><h id=0 a='&x; &bad &amp; &#65;'>a&b</h>tail",
        )
        .unwrap();
        assert_eq!(doc.nodes[0].attr("id"), Some("0"));
        assert_eq!(doc.nodes[0].attr("a"), Some("&x; &bad & A"));
    }
    #[test]
    fn existing_fatal_cases_stay_rejected() {
        for xml in [
            "<h><a/></h2>",
            "<h><a/>",
            "<h a='1' a='2'/>",
            "<x:h/>",
            "<h/><h/>",
            "<h a='<bad'/>",
        ] {
            assert!(parse_header_xml(xml).is_err(), "{xml}");
        }
    }
}
