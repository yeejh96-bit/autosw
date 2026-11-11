//! 한글에서 만든 PDF와 원본을 해시로 묶고, 조회 시 변경 여부를 확인한다.
//! 이 모듈은 한글을 실행하거나 화면 확인을 대신하지 않는다.
use crate::hwpx_document::sha256;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{ffi::OsString, fs, io::Read, path::Path};

pub(crate) fn canonical_path(path: &Path) -> Result<String, String> {
    let normalized: std::path::PathBuf = path.components().collect();
    let path = normalized.as_path();
    // Validate the target, but retain the spelling of ordinary components as
    // Node's non-native realpath does. Only links replace input components.
    fs::canonicalize(path).map_err(|e| e.to_string())?;
    let mut resolved = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(resolved.clone()) {
            return Err("파일 경로 연결이 순환합니다.".into());
        }
        let mut partial = std::path::PathBuf::new();
        let mut replacement = None;
        let mut components = resolved.components();
        while let Some(component) = components.next() {
            partial.push(component.as_os_str());
            if !matches!(component, std::path::Component::Normal(_)) {
                continue;
            }
            if fs::symlink_metadata(&partial)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                let target = fs::read_link(&partial).map_err(|e| e.to_string())?;
                let target = ordinary_path(&target)?;
                let next = partial
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(target)
                    .join(components.as_path());
                replacement = Some(std::path::absolute(next).map_err(|e| e.to_string())?);
                break;
            }
        }
        match replacement {
            Some(next) => resolved = next,
            None => {
                return resolved
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "파일 경로가 올바른 Unicode가 아닙니다.".into());
            }
        }
    }
}

fn ordinary_path(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("파일 경로가 올바른 Unicode가 아닙니다.")?;
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        Ok(format!(r"\\{unc}"))
    } else {
        Ok(text.strip_prefix(r"\\?\").unwrap_or(text).to_owned())
    }
}

pub(crate) fn hash_file(path: &Path) -> Result<String, String> {
    let normalized: std::path::PathBuf = path.components().collect();
    let mut file = fs::File::open(normalized).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn bind_preview(
    source: &Path,
    pdf: &Path,
    expected_source_hash: &str,
    page_count: f64,
) -> Result<Value, String> {
    let source_hash = hash_file(source)?;
    if !source_hash.eq_ignore_ascii_case(expected_source_hash) {
        return Err("원본 변경: 미리보기를 다시 생성해야 합니다.".into());
    }
    let normalized: std::path::PathBuf = pdf.components().collect();
    let pdf_bytes = fs::read(normalized).map_err(|e| e.to_string())?;
    if !pdf_bytes.starts_with(b"%PDF-") {
        return Err("PDF 형식 오류".into());
    }
    if !page_count.is_finite()
        || page_count.fract() != 0.0
        || !(1.0..=9_007_199_254_740_991.0).contains(&page_count)
    {
        return Err("쪽 수 오류".into());
    }
    Ok(json!({
        "schemaVersion": 1,
        "status": "hancom-output-preview",
        "sourcePath": canonical_path(source)?,
        "sourceSha256": source_hash,
        "pdfPath": canonical_path(pdf)?,
        "pdfSha256": sha256(&pdf_bytes),
        "pageCount": page_count as u64,
        "visualStatus": "awaiting-review"
    }))
}

pub fn verify_preview(report: &Value) -> Result<Value, String> {
    if report.get("schemaVersion").and_then(Value::as_f64) != Some(1.0)
        || report.get("status").and_then(Value::as_str) != Some("hancom-output-preview")
    {
        return Err("미리보기 기록 형식 오류".into());
    }
    let path = |key: &str| {
        report
            .get(key)
            .and_then(Value::as_str)
            .map(Path::new)
            .ok_or_else(|| "미리보기 파일 경로 오류".to_owned())
    };
    if report.get("sourceSha256").and_then(Value::as_str)
        != Some(hash_file(path("sourcePath")?)?.as_str())
    {
        return Err("원본 변경: 미리보기를 다시 생성해야 합니다.".into());
    }
    if report.get("pdfSha256").and_then(Value::as_str)
        != Some(hash_file(path("pdfPath")?)?.as_str())
    {
        return Err("PDF 변경: 미리보기를 다시 생성해야 합니다.".into());
    }
    let mut result = json!({"status":"current-hancom-output", "pdfPath":report["pdfPath"]});
    for key in ["pageCount", "visualStatus"] {
        if let Some(value) = report.get(key) {
            result[key] = value.clone();
        }
    }
    Ok(result)
}

fn page_number(text: &str) -> f64 {
    let text = crate::input::trim(text);
    let number = [(["0x", "0X"], 16), (["0b", "0B"], 2), (["0o", "0O"], 8)]
        .into_iter()
        .find_map(|(prefixes, radix)| {
            prefixes
                .iter()
                .find_map(|prefix| text.strip_prefix(prefix).map(|digits| (digits, radix)))
        });
    if let Some((digits, radix)) = number {
        let significant = digits.trim_start_matches('0');
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii() && c.is_digit(radix)) {
            return f64::NAN;
        }
        u64::from_str_radix(
            if significant.is_empty() {
                "0"
            } else {
                significant
            },
            radix,
        )
        .map_or(f64::NAN, |n| n as f64)
    } else {
        text.parse().unwrap_or(f64::NAN)
    }
}

pub fn run(args: &[OsString]) -> Result<String, String> {
    let args = args
        .iter()
        .map(|s| s.to_str().ok_or("인자가 올바른 Unicode가 아닙니다."))
        .collect::<Result<Vec<_>, _>>()?;
    let result = if args.len() == 6 && args[0] == "bind" {
        let result = bind_preview(
            Path::new(args[1]),
            Path::new(args[2]),
            args[3],
            page_number(args[4]),
        )?;
        let report_path = crate::hwpx_form::absolute(Path::new(args[5]))?;
        crate::atomic_file::create_new(
            Path::new(&report_path),
            crate::hwpx_form::pretty(&result)?.as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        result
    } else if args.len() == 2 && args[0] == "verify" {
        verify_preview(&crate::hwpx_form::read_json(Path::new(args[1]))?)?
    } else {
        return Err("bind SOURCE PDF SOURCE_HASH PAGES REPORT 또는 verify REPORT".into());
    };
    crate::hwpx_form::pretty(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_arguments_keep_javascript_radix_syntax() {
        for value in ["0x+2", "0b+1", "0o+2", "0x-1", "0x", "0b2", "0o8"] {
            assert!(page_number(value).is_nan(), "{value}");
        }
        for value in ["0x2", "0X02", "0b10", "0o2", " 2 ", "2e0", "+2"] {
            assert_eq!(page_number(value), 2.0, "{value}");
        }
    }
    #[test]
    fn preview_detects_source_and_pdf_changes_without_modifying_either_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("합성 # 문서.hwpx");
        let pdf = directory.path().join("합성 미리보기.pdf");
        fs::write(&source, "원본 합성 자료").unwrap();
        fs::write(&pdf, "%PDF-1.7\n합성 검증 자료").unwrap();
        let expected = hash_file(&source).unwrap();
        let report = bind_preview(&source, &pdf, &expected.to_uppercase(), 2.0).unwrap();
        let report_before = report.clone();
        assert_eq!(
            verify_preview(&report).unwrap()["status"],
            "current-hancom-output"
        );
        fs::write(&pdf, "%PDF-1.7\n바뀐 미리보기").unwrap();
        assert!(verify_preview(&report).unwrap_err().starts_with("PDF 변경"));
        fs::write(&source, "바뀐 원본").unwrap();
        assert!(
            verify_preview(&report)
                .unwrap_err()
                .starts_with("원본 변경")
        );
        assert!(bind_preview(&source, &pdf, &expected, 2.0).is_err());
        assert_eq!(report, report_before);
        assert_eq!(fs::read_to_string(&source).unwrap(), "바뀐 원본");
        assert_eq!(fs::read_to_string(&pdf).unwrap(), "%PDF-1.7\n바뀐 미리보기");
    }
    #[test]
    fn bind_checks_pdf_header_and_safe_page_count() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.hwpx");
        let pdf = directory.path().join("preview.pdf");
        fs::write(&source, b"synthetic").unwrap();
        let expected = hash_file(&source).unwrap();
        fs::write(&pdf, b"not-pdf").unwrap();
        assert_eq!(
            bind_preview(&source, &pdf, &expected, 1.0).unwrap_err(),
            "PDF 형식 오류"
        );
        fs::write(&pdf, b"%PDF-").unwrap();
        for pages in [
            0.0,
            -1.0,
            1.5,
            f64::NAN,
            f64::INFINITY,
            9_007_199_254_740_992.0,
        ] {
            assert_eq!(
                bind_preview(&source, &pdf, &expected, pages).unwrap_err(),
                "쪽 수 오류"
            );
        }
        assert_eq!(
            bind_preview(&source, &pdf, &expected, 9_007_199_254_740_991.0).unwrap()["pageCount"],
            9_007_199_254_740_991_u64
        );
    }
}
