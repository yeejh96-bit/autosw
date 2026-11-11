//! In-memory XML tree for layout calculations, including Unicode names.
//! Layout mutations change only this tree; never use its offsets to edit inputs.
use std::borrow::Cow;
use std::collections::HashMap;

/// The frozen xmldom parser applies these line endings before XML tokenization,
/// even for XML 1.0. Numeric character references remain untouched.
pub fn normalize_xml_lines(xml: &str) -> Cow<'_, str> {
    if !xml
        .chars()
        .any(|c| matches!(c, '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
    {
        return Cow::Borrowed(xml);
    }
    let mut out = String::with_capacity(xml.len());
    let mut chars = xml.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if matches!(chars.peek(), Some('\n' | '\u{85}')) {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{85}' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

#[derive(Clone)]
pub enum XmlPart {
    Text(String),
    Node(usize),
}
#[derive(Clone)]
pub struct Node {
    owner_root: usize,
    pub name: String,
    pub local: String,
    attrs: HashMap<String, String>,
    ordered_attrs: Vec<(String, String)>,
    pub children: Vec<usize>,
    pub parts: Vec<XmlPart>,
    pub parent: Option<usize>,
}
impl Node {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    pub fn set_attr(&mut self, key: &str, value: String) {
        self.attrs.insert(key.to_owned(), value.clone());
        if let Some(entry) = self.ordered_attrs.iter_mut().find(|(name, _)| name == key) {
            entry.1 = value;
        } else {
            self.ordered_attrs.push((key.to_owned(), value));
        }
    }
    pub fn attributes(&self) -> &[(String, String)] {
        &self.ordered_attrs
    }
    pub fn replace_attributes(&mut self, attributes: Vec<(String, String)>) {
        self.attrs = attributes.iter().cloned().collect();
        self.ordered_attrs = attributes;
    }
}
/// Preserve the declared attribute order, including namespace declarations.
/// Only names/order are scanned; decoded values come from the XML parser.
pub fn ordered_attributes(node: roxmltree::Node<'_, '_>, xml: &str) -> Vec<(String, String)> {
    let source = &xml[node.range().start + 1..];
    let bytes = source.as_bytes();
    let mut at = bytes
        .iter()
        .position(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))
        .unwrap_or(bytes.len());
    let mut result = Vec::new();
    loop {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at).is_none_or(|b| matches!(b, b'/' | b'>')) {
            break;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'=')
        {
            at += 1;
        }
        let name = &source[start..at];
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'=')
        {
            at += 1;
        }
        let Some(&quote) = bytes.get(at) else { break };
        at += 1;
        while bytes.get(at).is_some_and(|b| *b != quote) {
            at += 1;
        }
        if at < bytes.len() {
            at += 1;
        }
        let value = if name == "xmlns" {
            node.lookup_namespace_uri(None).unwrap_or("")
        } else if name == "xmlns:xml" {
            // This reserved declaration is validated but omitted by roxmltree.
            "http://www.w3.org/XML/1998/namespace"
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            node.lookup_namespace_uri(Some(prefix)).unwrap_or("")
        } else {
            node.attributes()
                .find(|attr| &xml[attr.range_qname()] == name)
                .map(|attr| attr.value())
                .unwrap_or("")
        };
        result.push((name.to_owned(), value.to_owned()));
    }
    result
}
pub struct ParsedXml {
    pub root: usize,
    pub nodes: Vec<Node>,
}
impl ParsedXml {
    /// Independent documents share storage, never the section's child list.
    pub fn document_root(&self, node: usize) -> Result<usize, String> {
        self.nodes
            .get(node)
            .map(|n| n.owner_root)
            .ok_or_else(|| "XML 노드가 없습니다.".into())
    }
    pub fn import_document(&mut self, mut document: ParsedXml) -> Result<usize, String> {
        let offset = self.nodes.len();
        if document.nodes.len() > 500_000usize.saturating_sub(offset) {
            return Err("XML 구조 처리 한도를 넘었습니다.".into());
        }
        let root = document.root + offset;
        for node in &mut document.nodes {
            node.owner_root += offset;
            node.parent = node.parent.map(|n| n + offset);
            for child in &mut node.children {
                *child += offset;
            }
            for part in &mut node.parts {
                if let XmlPart::Node(child) = part {
                    *child += offset;
                }
            }
        }
        self.nodes.append(&mut document.nodes);
        Ok(root)
    }
    fn allocate(&mut self, node: Node) -> Result<usize, String> {
        if self.nodes.len() >= 500_000 {
            return Err("XML 구조 처리 한도를 넘었습니다.".into());
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        Ok(index)
    }
    fn create_layout_element(&mut self, local: &str, owner_node: usize) -> Result<usize, String> {
        let owner_root = self.document_root(owner_node)?;
        self.allocate(Node {
            owner_root,
            name: format!("hp:{local}"),
            local: local.into(),
            attrs: HashMap::new(),
            ordered_attrs: Vec::new(),
            children: Vec::new(),
            parts: Vec::new(),
            parent: None,
        })
    }
    pub fn create_line_array(&mut self) -> Result<usize, String> {
        self.create_line_array_for(self.root)
    }
    pub fn create_line_segment(&mut self) -> Result<usize, String> {
        self.create_line_segment_for(self.root)
    }
    pub fn create_line_array_for(&mut self, owner_node: usize) -> Result<usize, String> {
        self.create_layout_element("linesegarray", owner_node)
    }
    pub fn create_line_segment_for(&mut self, owner_node: usize) -> Result<usize, String> {
        self.create_layout_element("lineseg", owner_node)
    }
    pub fn clone_shallow(&mut self, index: usize) -> Result<usize, String> {
        let mut node = self
            .nodes
            .get(index)
            .ok_or("복제할 XML 노드가 없습니다.")?
            .clone();
        node.children.clear();
        node.parts.clear();
        node.parent = None;
        self.allocate(node)
    }
    pub fn remove_child(&mut self, parent: usize, child: usize) -> Result<(), String> {
        if self
            .nodes
            .get(child)
            .is_none_or(|n| n.parent != Some(parent))
            || self
                .nodes
                .get(parent)
                .is_none_or(|n| !n.children.contains(&child))
        {
            return Err("XML 자식 관계가 일치하지 않습니다.".into());
        }
        self.nodes[parent].children.retain(|&i| i != child);
        self.nodes[parent]
            .parts
            .retain(|p| !matches!(p, XmlPart::Node(i) if *i == child));
        self.nodes[child].parent = None;
        Ok(())
    }
    pub fn append_child(&mut self, parent: usize, child: usize) -> Result<(), String> {
        if self.nodes.get(parent).is_none()
            || self.nodes.get(child).is_none()
            || child == self.nodes[child].owner_root
            || self.nodes[parent].owner_root != self.nodes[child].owner_root
        {
            return Err("XML에 연결할 노드가 올바르지 않습니다.".into());
        }
        let mut ancestor = Some(parent);
        let mut depth = 0;
        while let Some(index) = ancestor {
            if index == child {
                return Err("XML 노드를 순환 연결할 수 없습니다.".into());
            }
            if depth >= 200 {
                return Err("XML 구조 처리 한도를 넘었습니다.".into());
            }
            ancestor = self.nodes[index].parent;
            depth += 1;
        }
        let mut pending = vec![(child, 0)];
        while let Some((index, child_depth)) = pending.pop() {
            if depth + child_depth > 200 {
                return Err("XML 구조 처리 한도를 넘었습니다.".into());
            }
            pending.extend(
                self.nodes[index]
                    .children
                    .iter()
                    .map(|&i| (i, child_depth + 1)),
            );
        }
        if let Some(old_parent) = self.nodes[child].parent {
            self.remove_child(old_parent, child)?;
        }
        self.nodes[parent].children.push(child);
        self.nodes[parent].parts.push(XmlPart::Node(child));
        self.nodes[child].parent = Some(parent);
        Ok(())
    }
}
pub fn parse_style_xml(xml: &str) -> Result<ParsedXml, String> {
    let normalized = normalize_xml_lines(xml);
    let xml = normalized.as_ref();
    let document = roxmltree::Document::parse(xml).map_err(|e| format!("XML 파싱 실패: {e}"))?;
    let mut result = ParsedXml {
        root: 0,
        nodes: Vec::new(),
    };
    fn append(
        source: roxmltree::Node<'_, '_>,
        xml: &str,
        parent: Option<usize>,
        out: &mut ParsedXml,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > 200 || out.nodes.len() >= 500_000 {
            return Err("XML 구조 처리 한도를 넘었습니다.".into());
        }
        let id = out.nodes.len();
        let ordered_attrs = ordered_attributes(source, xml);
        let name = xml[source.range().start + 1..]
            .split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or_default();
        for qualified in
            std::iter::once(name).chain(ordered_attrs.iter().map(|(name, _)| name.as_str()))
        {
            if let Some((prefix, _)) = qualified.split_once(':')
                && prefix != "xmlns"
                && prefix != "xml"
                && source
                    .lookup_namespace_uri(Some(prefix))
                    .is_none_or(str::is_empty)
            {
                return Err(format!("정의되지 않은 XML 접두사입니다: {prefix}"));
            }
        }
        let attrs = ordered_attrs.iter().cloned().collect();
        out.nodes.push(Node {
            owner_root: 0,
            name: name.to_owned(),
            local: source.tag_name().name().into(),
            attrs,
            ordered_attrs,
            children: Vec::new(),
            parts: Vec::new(),
            parent,
        });
        for c in source.children() {
            if c.is_element() {
                let child = append(c, xml, Some(id), out, depth + 1)?;
                out.nodes[id].children.push(child);
                out.nodes[id].parts.push(XmlPart::Node(child));
            } else if c.is_text() {
                out.nodes[id]
                    .parts
                    .push(XmlPart::Text(c.text().unwrap_or("").into()));
            }
        }
        Ok(id)
    }
    append(document.root_element(), xml, None, &mut result, 0)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_documents_keep_roots_owners_and_remapped_parts() {
        let mut doc = parse_style_xml("<section><p>body</p></section>").unwrap();
        let original_children = doc.nodes[doc.root].children.clone();
        let master = parse_style_xml(
            "<m:root xmlns:m=\"urn:master\" z=\"1\" a=\"2\">before<m:p>master</m:p>after</m:root>",
        )
        .unwrap();
        let expected_attributes = master.nodes[master.root].attributes().to_vec();
        let root = doc.import_document(master).unwrap();
        assert_eq!(doc.nodes[doc.root].children, original_children);
        assert_eq!(doc.nodes[root].parent, None);
        assert_eq!(doc.nodes[root].name, "m:root");
        assert_eq!(doc.nodes[root].attributes(), expected_attributes);
        let p = doc.nodes[root].children[0];
        assert_eq!(doc.document_root(p).unwrap(), root);
        assert!(matches!(doc.nodes[root].parts[1], XmlPart::Node(n) if n == p));
        let lines = doc.create_line_array_for(p).unwrap();
        let seg = doc.create_line_segment_for(p).unwrap();
        doc.append_child(lines, seg).unwrap();
        doc.append_child(p, lines).unwrap();
        let cloned = doc.clone_shallow(seg).unwrap();
        assert_eq!(doc.document_root(cloned).unwrap(), root);
        assert!(doc.append_child(doc.root, root).is_err());
        assert!(doc.append_child(original_children[0], cloned).is_err());
        assert_eq!(doc.nodes[cloned].parent, None);
        doc.append_child(lines, cloned).unwrap();
        assert_eq!(doc.document_root(cloned).unwrap(), root);
    }
    #[test]
    fn xmldom_line_endings_preserve_numeric_references_and_reject_empty_prefix_use() {
        let d = parse_style_xml("<p a='\u{85}\u{2028}\u{2029}\r\u{85}'>\u{85}\u{2028}\u{2029}\r\u{85}&#x85;&#x2028;&#x2029;<![CDATA[\u{2029}]]></p>").unwrap();
        assert_eq!(d.nodes[0].attr("a"), Some("    "));
        let body: String = d.nodes[0]
            .parts
            .iter()
            .filter_map(|part| {
                if let XmlPart::Text(t) = part {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(body, "\n\n\n\n\u{85}\u{2028}\u{2029}\n");
        assert!(parse_style_xml("<h xmlns:p=''><p:c/></h>").is_err());
        assert!(parse_style_xml("<h xmlns:p='' p:x='v'/>").is_err());
        assert!(parse_style_xml("<h xmlns:p=''/>").is_ok());
    }
    #[test]
    fn generated_lines_keep_attribute_order_parent_links_and_original_nodes() {
        let mut doc = parse_style_xml("<hp:p xmlns:hp='u'>앞<hp:linesegarray><hp:lineseg textpos='0' vertpos='10' keep='x'><hp:child/></hp:lineseg></hp:linesegarray>뒤</hp:p>").unwrap();
        let new_array = doc.create_line_array().unwrap();
        let copied = doc.clone_shallow(2).unwrap();
        doc.nodes[copied].set_attr("vertpos", "30".into());
        assert!(doc.nodes[copied].children.is_empty());
        assert_eq!(doc.nodes[2].attr("vertpos"), Some("10"));
        assert_eq!(
            doc.nodes[copied].attributes()[1],
            ("vertpos".into(), "30".into())
        );
        doc.append_child(new_array, copied).unwrap();
        let new_segment = doc.create_line_segment().unwrap();
        doc.nodes[new_segment].set_attr("textpos", "8".into());
        doc.append_child(new_array, new_segment).unwrap();
        doc.append_child(doc.root, new_array).unwrap();
        doc.remove_child(doc.root, 1).unwrap();
        assert!(doc.remove_child(doc.root, 1).is_err());
        assert_eq!(doc.nodes[1].parent, None);
        assert_eq!(doc.nodes[doc.root].children, vec![new_array]);
        assert!(
            matches!(&doc.nodes[doc.root].parts[..], [XmlPart::Text(a),XmlPart::Text(b),XmlPart::Node(i)] if a=="앞" && b=="뒤" && *i==new_array)
        );
        assert!(doc.append_child(copied, new_array).is_err());
        assert_eq!(doc.nodes[new_array].parent, Some(doc.root));
        doc.append_child(new_array, copied).unwrap();
        assert_eq!(doc.nodes[new_array].children, vec![new_segment, copied]);
        assert_eq!(doc.nodes[2].children, vec![3]);
    }
    #[test]
    fn namespace_order_and_attribute_updates_preserve_existing_positions() {
        let mut doc = parse_style_xml("<한글 xmlns:p='urn:p' first='&amp;' xmlns='urn:default' p:x='1'><p:자식 xmlns='' last='2'/></한글>").unwrap();
        assert_eq!(
            doc.nodes[0].attributes(),
            &[
                ("xmlns:p".into(), "urn:p".into()),
                ("first".into(), "&".into()),
                ("xmlns".into(), "urn:default".into()),
                ("p:x".into(), "1".into())
            ]
        );
        doc.nodes[0].set_attr("first", "new".into());
        doc.nodes[0].set_attr("added", "3".into());
        assert_eq!(doc.nodes[0].attributes()[1], ("first".into(), "new".into()));
        assert_eq!(doc.nodes[0].attributes()[4], ("added".into(), "3".into()));
        assert_eq!(doc.nodes[1].name, "p:자식");
        assert_eq!(doc.nodes[1].attr("xmlns"), Some(""));
        let reserved =
            parse_style_xml("<s xmlns:xml='http://www.w3.org/XML/1998/namespace' xml:lang='ko'/>")
                .unwrap();
        assert_eq!(
            reserved.nodes[0].attr("xmlns:xml"),
            Some("http://www.w3.org/XML/1998/namespace")
        );
    }
}
