//! Read-only layout loader. Strict office validation precedes the frozen kordoc
//! loader (MIT, Copyright 2026 chrisryugj; see data/kordoc-LICENSE).
//! ZIP data is decompressed once; this module reuses that owned strict archive.
use super::{
    font_env, head_styles,
    header_xml_compat::parse_header_xml,
    para_model::find_first,
    preview_fonts,
    section_render::{self, RenderOptions, SectionInput},
    style_dom::ParsedXml,
    svg_assemble,
    value::{Map, Value, native_json as json, number, string},
};
use crate::{
    hwpx_document,
    office_zip::{Archive, Entry},
};
use data_encoding::BASE64;
use icu_collator::{Collator, options::CollatorOptions};
use icu_locale::locale;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

const MIB: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 500;
const WARNING: &str = "파일에 저장된 배치와 자체 조판을 사용한 미리보기입니다. 실제 한글 출력과 다를 수 있습니다. 한글 전용 글꼴은 대응하는 Windows 글꼴로 표시하며 화면의 글자 가장자리는 한글 출력과 다를 수 있습니다.";
const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
static SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i-u:Contents/section[0-9]+\.xml)\z").unwrap());
static SECTION_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[Ss]ection([0-9]+)\.xml\z").unwrap());
static MASTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i-u:Contents/masterpage)[^/]*(?i-u:\.xml)\z").unwrap());
static HPF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u:content\.hpf)\z").unwrap());
static STRIP_DTD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i:<!DOCTYPE){JS_SPACE}[^\[>]*(\[[\s\S]*?\])?{JS_SPACE}*>"
    ))
    .unwrap()
});
static BIN_ID_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<[^>]*(?-u:\b)id="([^"]+)"[^>]*(?-u:\b)href="(BinData/[^"]+)"[^>]*>"#).unwrap()
});
static BIN_HREF_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<[^>]*(?-u:\b)href="(BinData/[^"]+)"[^>]*(?-u:\b)id="([^"]+)"[^>]*>"#).unwrap()
});
static IMAGE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"binaryItemIDRef="([^"]+)""#).unwrap());
static DISTRIBUTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?-u:\b)distribution="1""#).unwrap());

pub struct LoadedLayout {
    pub document: ParsedXml,
    pub sections: Vec<SectionInput>,
    pub styles: Value,
    pub images: Map<String, Value>,
    pub warnings: Vec<String>,
}
pub struct LayoutPreview {
    pub font_css: String,
    pub svgs: Vec<String>,
    pub report: Value,
}
const ZIP_LOAD_ERROR: &str = "HWPX(ZIP) 형식이 아닙니다 — 렌더는 HWPX만 지원";
struct NamedEntry<'a> {
    name: String,
    entry: &'a Entry,
    dir: bool,
}
impl std::ops::Deref for NamedEntry<'_> {
    type Target = Entry;
    fn deref(&self) -> &Entry {
        self.entry
    }
}
struct ArchiveView<'a> {
    entries: Vec<NamedEntry<'a>>,
}
fn resolve_name(name: &str) -> String {
    let parts = name.split('/').collect::<Vec<_>>();
    let mut result = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if *part == "." || part.is_empty() && i != 0 && i != parts.len() - 1 {
            continue;
        }
        if *part == ".." {
            result.pop();
        } else {
            result.push(*part);
        }
    }
    result.join("/")
}
fn unicode_extra(data: Option<&[u8]>, original: &[u8]) -> Result<Option<String>, String> {
    let Some(data) = data else {
        return Ok(None);
    };
    let version = *data.first().ok_or(ZIP_LOAD_ERROR)?;
    if version != 1 {
        return Ok(None);
    }
    let crc = u32::from_le_bytes(data.get(1..5).ok_or(ZIP_LOAD_ERROR)?.try_into().unwrap());
    if crc != crate::office_zip::crc32(original) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&data[5..]).into_owned()))
}
impl<'a> ArchiveView<'a> {
    fn new(archive: &'a Archive) -> Result<Self, String> {
        let mut entries = Vec::<NamedEntry<'a>>::new();
        let mut slots = HashMap::new();
        for entry in &archive.entries {
            let c = &entry.central;
            let u16_at = |at| {
                c.get(at..at + 2)
                    .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
                    .ok_or(ZIP_LOAD_ERROR)
            };
            let nl = u16_at(28)?;
            let el = u16_at(30)?;
            let cl = u16_at(32)?;
            let raw_name = c.get(46..46 + nl).ok_or(ZIP_LOAD_ERROR)?;
            let raw_comment = c
                .get(46 + nl + el..46 + nl + el + cl)
                .ok_or(ZIP_LOAD_ERROR)?;
            let mut path_extra = None;
            let mut comment_extra = None;
            let mut at = 46 + nl;
            let end = at + el;
            // Match JSZip's last-field-wins scan, including its strict < end.
            while at + 4 < end {
                let kind = u16_at(at)?;
                let size = u16_at(at + 2)?;
                let data = c.get(at + 4..at + 4 + size).ok_or(ZIP_LOAD_ERROR)?;
                if kind == 0x7075 {
                    path_extra = Some(data);
                }
                if kind == 0x6375 {
                    comment_extra = Some(data);
                }
                at += 4 + size;
            }
            let original = if entry.flags & 0x800 != 0 {
                entry.name.clone()
            } else {
                let path =
                    unicode_extra(path_extra, raw_name)?.unwrap_or_else(|| entry.name.clone());
                // Comments are unused by rendering, but malformed Unicode
                // comment metadata still rejects JSZip.loadAsync itself.
                unicode_extra(comment_extra, raw_comment)?;
                path
            };
            let dos_dir = c.get(38).is_some_and(|b| b & 0x10 != 0);
            let input_dir = dos_dir || original.ends_with('/');
            let unix = c.get(5) == Some(&3) && c.get(41).is_some_and(|b| b & 0x40 != 0);
            // JSZip's fileAdd converts Unix directories; load then tries to
            // assign unsafeOriginalName through file(name), which is null.
            if unix && !input_dir {
                return Err(ZIP_LOAD_ERROR.into());
            }
            let dir = input_dir || unix;
            let mut name = resolve_name(&original);
            if dir && !name.ends_with('/') {
                name.push('/');
            }
            let view = NamedEntry {
                name: name.clone(),
                entry,
                dir,
            };
            if let Some(&slot) = slots.get(&name) {
                entries[slot] = view;
            } else {
                slots.insert(name, entries.len());
                entries.push(view);
            }
        }
        Ok(Self { entries })
    }
}
fn is_file(entry: &NamedEntry<'_>) -> bool {
    !entry.dir
}
fn file<'a, 'b>(archive: &'b ArchiveView<'a>, name: &str) -> Option<&'b NamedEntry<'a>> {
    archive
        .entries
        .iter()
        .find(|e| e.name == name && is_file(e))
}
fn max_unzip() -> f64 {
    let value = number(
        &std::env::var("KORDOC_MAX_UNZIP_MB")
            .unwrap_or_default()
            .into(),
    );
    if value.is_finite() && value > 0.0 {
        value.min(8192.0) * MIB as f64
    } else {
        256.0 * MIB as f64
    }
}
fn text(entry: &Entry, limit: f64) -> Result<String, String> {
    if entry.data.len() as f64 > limit {
        return Err("ZIP 압축 해제 크기 초과 (ZIP bomb 의심)".into());
    }
    // The old reader's explicit Buffer.toString('utf-8') boundary replaces
    // invalid source byte sequences. No intermediate layout text is lossy.
    Ok(String::from_utf8_lossy(&entry.data).into_owned())
}
fn stripped(text: &str) -> String {
    STRIP_DTD
        .replace_all(text.strip_prefix('\u{feff}').unwrap_or(text), "")
        .into_owned()
}
fn section_number(name: &str) -> f64 {
    SECTION_NUMBER
        .captures(name)
        .map_or(9007199254740991.0, |c| {
            c[1].parse().unwrap_or(f64::INFINITY)
        })
}
fn fixed_mb(bytes: usize) -> String {
    // bytes/2^20 is exact. Round the rational *10 upward on ties, as toFixed(1).
    let tenths = (bytes as u128 * 10 + MIB as u128 / 2) / (MIB as u128);
    format!("{}.{:01}", tenths / 10, tenths % 10)
}
fn precheck(archive: &Archive, limit: f64) -> Result<(), String> {
    if archive.entries.len() > MAX_ENTRIES {
        return Err(format!(
            "ZIP 엔트리 수 초과: {} (최대 {MAX_ENTRIES})",
            archive.entries.len()
        ));
    }
    let total = archive
        .entries
        .iter()
        .try_fold(0usize, |sum, e| sum.checked_add(e.data.len()))
        .ok_or("ZIP 비압축 크기 합계 오류")?;
    if total as f64 > limit {
        return Err(format!(
            "ZIP 비압축 크기 초과: {}MB (최대 {}MB)",
            fixed_mb(total),
            string(&(limit / MIB as f64).into())
        ));
    }
    Ok(())
}
// JavaScript non-Unicode /i canonicalization: a BMP unit may uppercase to one
// BMP unit, but a non-ASCII character cannot become ASCII, and astral pairs
// are not case-folded as code points without the u flag.
fn legacy_fold(s: &str) -> String {
    s.chars()
        .map(|c| {
            if u32::from(c) > 0xffff {
                return c;
            }
            let mut upper = c.to_uppercase();
            let u = upper.next().unwrap();
            if upper.next().is_none() && !(u32::from(c) >= 128 && u32::from(u) < 128) {
                u
            } else {
                c
            }
        })
        .collect()
}
#[derive(Clone, Copy)]
struct ImageLimits {
    count: usize,
    per_image: usize,
    total: usize,
}
const IMAGE_LIMITS: ImageLimits = ImageLimits {
    count: 256,
    per_image: 40 * MIB,
    total: 128 * MIB,
};
fn load_images(
    archive: &ArchiveView<'_>,
    sec_xmls: &[String],
    master_xmls: &[String],
    limit: f64,
    limits: ImageLimits,
    warnings: &mut Vec<String>,
) -> Result<Map<String, Value>, String> {
    let mut binmap = Map::<String, String>::default();
    if let Some(hpf) = archive
        .entries
        .iter()
        .find(|e| is_file(e) && HPF.is_match(&e.name))
    {
        let manifest = text(hpf, limit)?;
        for caps in BIN_ID_FIRST.captures_iter(&manifest) {
            binmap.insert(caps[1].into(), caps[2].into());
        }
        for caps in BIN_HREF_FIRST.captures_iter(&manifest) {
            binmap.insert(caps[2].into(), caps[1].into());
        }
    }
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for xml in sec_xmls.iter().chain(master_xmls) {
        for c in IMAGE_REF.captures_iter(xml) {
            if seen.insert(c[1].to_owned()) {
                refs.push(c[1].to_owned());
            }
        }
    }
    let mut images = Map::default();
    let mut total = 0;
    for reference in &refs {
        if images.len() >= limits.count {
            warnings.push(format!(
                "이미지 {}종 중 {}종만 로딩 — 개수 한도 초과분 생략",
                refs.len(),
                limits.count
            ));
            break;
        }
        let href = if let Some(href) = binmap.get(reference).filter(|s| !s.is_empty()) {
            Some(href.clone())
        } else {
            let pattern = Regex::new(&format!(
                r"BINDATA/[^\n\r\u{{2028}}\u{{2029}}]*{}",
                regex::escape(&legacy_fold(reference))
            ))
            .map_err(|e| e.to_string())?;
            archive
                .entries
                .iter()
                .find(|e| is_file(e) && pattern.is_match(&legacy_fold(&e.name)))
                .map(|e| e.name.clone())
        };
        let Some(href) = href else {
            continue;
        };
        let Some(entry) =
            file(archive, &href).or_else(|| file(archive, &format!("Contents/{href}")))
        else {
            continue;
        };
        let size = entry.data.len();
        if size as f64 > limit {
            return Err("ZIP 압축 해제 크기 초과 (ZIP bomb 의심)".into());
        }
        if size > limits.per_image {
            warnings.push(format!(
                "이미지 {href} {}MB — 한도 초과로 생략",
                fixed_mb(size)
            ));
            continue;
        }
        if size > limits.total.saturating_sub(total) {
            warnings.push(format!(
                "이미지 누적 {}MB 한도 초과 — 이후 생략",
                string(&super::svg_para::round(limits.total as f64 / MIB as f64).into())
            ));
            break;
        }
        total += size;
        images.insert(reference.clone(),json!({"dataUri":format!("data:{};base64,{}",super::svg_images::sniff_mime(&href,&entry.data),BASE64.encode(&entry.data))}));
    }
    Ok(images)
}

/// `archive` must come from load_hwpx, preserving its stronger ZIP/XML checks.
/// All parts are read from memory; only the later preview-font stage reads its
/// two named Windows font files. No ZIP extraction occurs.
pub fn load_layout_archive(archive: &Archive) -> Result<LoadedLayout, String> {
    let limit = max_unzip();
    precheck(archive, limit)?;
    let view = ArchiveView::new(archive)?;
    let archive = &view;
    if let Some(manifest) = file(archive, "META-INF/manifest.xml")
        && text(manifest, limit)?.contains("encryption-data")
    {
        let distribution = file(archive, "Contents/content.hpf")
            .and_then(|f| text(f, limit).ok())
            .is_some_and(|s| DISTRIBUTION.is_match(&s));
        return Err(if distribution{"배포용으로 암호화된 HWPX 파일입니다 (배포용 복호는 지원하지 않음). 한컴오피스에서 일반 문서로 저장한 뒤 여세요."}else{"암호로 보호된 HWPX 파일입니다. password 옵션에 열기 암호를 지정하세요."}.into());
    }
    let mut sec_files = archive
        .entries
        .iter()
        .filter(|e| is_file(e) && SECTION.is_match(&e.name))
        .collect::<Vec<_>>();
    let collator = Collator::try_new(locale!("ko").into(), CollatorOptions::default())
        .map_err(|e| e.to_string())?;
    sec_files.sort_by(|a, b| {
        let ai = section_number(&a.name);
        let bi = section_number(&b.name);
        if ai == bi {
            collator.compare(&a.name, &b.name)
        } else {
            (ai - bi)
                .partial_cmp(&0.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    if sec_files.is_empty() {
        return Err("Contents/section0.xml 없음 — HWPX가 아니거나 손상됨".into());
    }
    let mut warnings = Vec::new();
    let head_file =
        file(archive, "Contents/header.xml").or_else(|| file(archive, "Contents/head.xml"));
    let version = file(archive, "version.xml")
        .map(|f| text(f, limit))
        .transpose()?;
    let major = head_styles::hwpx_app_version(version.as_deref(), false);
    let build = head_styles::hwpx_app_version(version.as_deref(), true);
    let options = json!({"appMajor":major.map(Value::Number).unwrap_or(Value::Undefined),"appBuild":build.map(Value::Number).unwrap_or(Value::Undefined)});
    let head_xml = head_file
        .map(|f| text(f, limit).map(|s| stripped(&s)))
        .transpose()?;
    let mut styles = if let Some(xml) = head_xml.as_ref().filter(|s| !s.is_empty()) {
        head_styles::parse_render_styles(xml, Some(&options))?
    } else {
        json!({"charPr":[],"paraAlign":[],"paraGeom":[],"borderFill":[]})
    };
    if head_file.is_none() {
        warnings.push("header.xml 없음 — 기본 스타일로 렌더".into());
    }
    let sec_xmls = sec_files
        .into_iter()
        .map(|f| text(f, limit).map(|s| stripped(&s)))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(head) = head_xml.as_ref().filter(|s| !s.is_empty())
        && font_env::has_kopub_subst(head)
    {
        let mut opts = options.clone();
        opts["substKoPub"] = true.into();
        let alt = head_styles::parse_render_styles(head, Some(&opts))?;
        if font_env::kopub_substituted(&sec_xmls, &styles, &alt)? {
            styles = alt;
        }
    }
    let mut document = ParsedXml {
        root: 0,
        nodes: Vec::new(),
    };
    let mut masters = HashMap::<String, usize>::new();
    let mut master_xmls = Vec::new();
    for entry in archive
        .entries
        .iter()
        .filter(|e| is_file(e) && MASTER.is_match(&e.name))
    {
        let xml = stripped(&text(entry, limit)?);
        let doc = parse_header_xml(&xml)?;
        if doc.nodes[doc.root].local == "masterPage" {
            let id = doc.nodes[doc.root]
                .attr("id")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    let file = entry.name.rsplit('/').next().unwrap();
                    file.strip_suffix(".xml").unwrap_or(file).into()
                });
            let root = document.import_document(doc)?;
            masters.insert(id, root);
            master_xmls.push(xml);
        }
    }
    let images = load_images(
        archive,
        &sec_xmls,
        &master_xmls,
        limit,
        IMAGE_LIMITS,
        &mut warnings,
    )?;
    let mut sections = Vec::new();
    let mut last_parity = HashMap::<String, usize>::new();
    for (index, xml) in sec_xmls.iter().enumerate() {
        let doc = parse_header_xml(xml)?;
        let root = document.import_document(doc)?;
        let sec_pr = find_first(&document, root, "secPr", 0);
        let mut referenced = sec_pr
            .map(|sec| {
                document.nodes[sec]
                    .children
                    .iter()
                    .filter(|&&ch| document.nodes[ch].local == "masterPage")
                    .filter_map(|&ch| {
                        masters
                            .get(document.nodes[ch].attr("idRef").unwrap_or(""))
                            .copied()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let kind = |id: usize| {
            document.nodes[id]
                .attr("type")
                .filter(|s| !s.is_empty())
                .unwrap_or("BOTH")
        };
        if !referenced.iter().any(|&id| kind(id) == "BOTH") {
            for (t, other) in [("EVEN", "ODD"), ("ODD", "EVEN")] {
                if let Some(&prev) = last_parity.get(t)
                    && !referenced.iter().any(|&id| kind(id) == t)
                    && referenced.iter().any(|&id| kind(id) == other)
                {
                    referenced.insert(0, prev);
                }
            }
        }
        for &id in &referenced {
            if matches!(kind(id), "EVEN" | "ODD") {
                last_parity.insert(kind(id).into(), id);
            }
        }
        if referenced.iter().any(|&id| kind(id) == "BOTH") {
            last_parity.clear();
        }
        sections.push(SectionInput {
            root,
            index: index.into(),
            masters: referenced,
            has_deletion_markers: xml.contains("deleteBegin") || xml.contains("deleteEnd"),
        });
    }
    if sections.is_empty() {
        return Err("렌더할 구역이 없습니다 — HWPX가 손상되었을 수 있습니다".into());
    }
    Ok(LoadedLayout {
        document,
        sections,
        styles,
        images,
        warnings,
    })
}

pub fn render_hwpx_layout(bytes: &[u8]) -> Result<LayoutPreview, String> {
    let loaded = hwpx_document::load_hwpx(bytes)?;
    let mut layout = load_layout_archive(&loaded.archive)?;
    let rendered = section_render::render_section_roots(
        &mut layout.document,
        &layout.sections,
        layout.styles,
        layout.images,
        layout.warnings,
        &RenderOptions {
            reflow: true,
            reflow_mode: "keep".into(),
            highlights: Vec::new(),
        },
    )?;
    let assembled = svg_assemble::assemble_svg(&rendered);
    if assembled.page_count == 0 || assembled.svg.0.is_empty() {
        return Err("배치 미리보기 본문 없음".into());
    }
    let svg = svg_assemble::validated_svg(&assembled.svg)?;
    let fonts = preview_fonts::preview_fonts(&svg)?;
    let mut warnings = vec![WARNING.to_owned()];
    warnings.extend(assembled.warnings);
    warnings.extend(
        fonts["warnings"]
            .as_array()
            .ok_or("글꼴 경고 배열 없음")?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned),
    );
    let report = json!({"schemaVersion":1,"sourceSha256":hwpx_document::sha256(bytes),"status":"approximate-preview","engine":"layout","engineVersion":"local-2","embeddedFonts":Value::from_json(&fonts["fonts"]),"pageCount":assembled.page_count,"stats":assembled.stats,"warnings":warnings,"hangulVerified":false});
    Ok(LayoutPreview {
        font_css: fonts["css"].as_str().ok_or("글꼴 CSS 없음")?.into(),
        svgs: vec![svg],
        report,
    })
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    if operation != "render" {
        return Err("알 수 없는 HWPX layout 로더 연산".into());
    }
    let hex = input["bytesHex"].as_str().ok_or("bytesHex 필요")?;
    let bytes = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.as_bytes())
        .map_err(|_| "바이트 16진수 오류")?;
    let preview = render_hwpx_layout(&bytes)?;
    Ok(json!({"fontCss":preview.font_css,"svgs":preview.svgs,"report":preview.report}).to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(entries: &[(&str, &[u8])]) -> Archive {
        let entries = entries
            .iter()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect::<Vec<_>>();
        crate::office_zip::read_zip_strict(&crate::office_zip::create_zip(&entries).unwrap())
            .unwrap()
    }
    #[test]
    fn image_limits_keep_exact_boundary_and_skip_oversized_without_spending_total() {
        let archive = archive(&[
            ("BinData/a.png", b"1234"),
            ("BinData/b.png", b"12345"),
            ("BinData/c.png", b"1234"),
            ("BinData/d.png", b"1"),
        ]);
        let xml=vec![r#"binaryItemIDRef="missing" binaryItemIDRef="a" binaryItemIDRef="b" binaryItemIDRef="c" binaryItemIDRef="d""#.into()];
        let mut warnings = Vec::new();
        let images = load_images(
            &ArchiveView::new(&archive).unwrap(),
            &xml,
            &[],
            f64::INFINITY,
            ImageLimits {
                count: 256,
                per_image: 4,
                total: 8,
            },
            &mut warnings,
        )
        .unwrap();
        assert_eq!(
            images.keys().map(String::as_str).collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("b.png"));
        assert!(warnings[1].contains("누적"));
    }
    #[test]
    fn reference_count_uses_loaded_images_and_preserves_manifest_second_pass() {
        let archive = archive(&[
            (
                "content.hpf",
                br#"<i id="a" href="BinData/first.png"/><i href="BinData/second.png" id="a"/>"#,
            ),
            ("BinData/first.png", b"1"),
            ("BinData/second.png", b"2"),
            ("BinData/b.png", b"3"),
        ]);
        let mut warnings = Vec::new();
        let images = load_images(
            &ArchiveView::new(&archive).unwrap(),
            &[r#"binaryItemIDRef="a" binaryItemIDRef="b" binaryItemIDRef="a""#.into()],
            &[],
            f64::INFINITY,
            ImageLimits {
                count: 1,
                per_image: 10,
                total: 10,
            },
            &mut warnings,
        )
        .unwrap();
        assert!(images["a"]["dataUri"].as_str().unwrap().ends_with("Mg=="));
        assert_eq!(
            warnings,
            ["이미지 2종 중 1종만 로딩 — 개수 한도 초과분 생략"]
        );
    }
}
