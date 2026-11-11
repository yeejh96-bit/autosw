//! DOCX/HWPX 읽기·평문/구조 변환. 원본과 기존 결과는 덮어쓰지 않는다.
use crate::{
    atomic_file,
    office_text::{self, TextMode},
    office_zip,
};
use chrono::Local;
use regex::Regex;
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const DIFFERENT: &str = "기존 결과가 원본과 다릅니다. 불완전한 변환 또는 수정 여부를 확인하세요. 기존 파일은 보존했습니다.";
static HEADER_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(---\n출처: [^\n]*\n변환일: )[0-9]{4}-[0-9]{2}-[0-9]{2}(\n---(?:\n|$))").unwrap()
});
static SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Contents/section([0-9]+)\.xml$").unwrap());
static SECTION_PS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Contents/section([0-9]+)\.xml$").unwrap());

pub fn decode_text(bytes: &[u8], mode: TextMode) -> Result<String, String> {
    if mode == TextMode::PowerShell {
        let utf32 = if bytes.starts_with(&[255, 254, 0, 0]) {
            Some(true)
        } else if bytes.starts_with(&[0, 0, 254, 255]) {
            Some(false)
        } else {
            None
        };
        if let Some(little) = utf32 {
            if (bytes.len() - 4) % 4 != 0 {
                return Err("잘못된 UTF-32 본문입니다.".into());
            }
            return bytes[4..]
                .chunks_exact(4)
                .map(|c| {
                    let array: [u8; 4] = c.try_into().unwrap();
                    let point = if little {
                        u32::from_le_bytes(array)
                    } else {
                        u32::from_be_bytes(array)
                    };
                    char::from_u32(point).ok_or_else(|| "잘못된 UTF-32 본문입니다.".into())
                })
                .collect();
        }
        let utf16 = if bytes.starts_with(&[255, 254]) {
            Some(true)
        } else if bytes.starts_with(&[254, 255]) {
            Some(false)
        } else {
            None
        };
        if let Some(little) = utf16 {
            if bytes.len() % 2 != 0 {
                return Err("잘못된 UTF-16 본문입니다.".into());
            }
            let units: Vec<_> = bytes[2..]
                .chunks_exact(2)
                .map(|c| {
                    if little {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            return String::from_utf16(&units).map_err(|e| format!("잘못된 UTF-16 본문: {e}"));
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|e| format!("잘못된 UTF-8 본문: {e}"))?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text).into())
}

fn comparable(text: &str, mode: TextMode) -> String {
    let stripped = if mode == TextMode::PowerShell {
        text.trim_start_matches('\u{feff}')
    } else {
        text.strip_prefix('\u{feff}').unwrap_or(text)
    };
    HEADER_DATE
        .replace(&stripped.replace("\r\n", "\n"), "${1}DATE$2")
        .into_owned()
}

fn save(path: &Path, content: &str, mode: TextMode, structure: bool) -> Result<bool, String> {
    if path.try_exists().map_err(|e| e.to_string())? {
        if !path.is_file() {
            return Err("결과 경로가 파일이 아닙니다.".into());
        }
        let existing = decode_text(&fs::read(path).map_err(|e| e.to_string())?, mode)?;
        let same = if structure {
            let a = serde_json::from_str::<Value>(&existing).map_err(|_| DIFFERENT)?;
            let b = serde_json::from_str::<Value>(content).map_err(|_| DIFFERENT)?;
            a == b
        } else {
            comparable(&existing, mode) == comparable(content, mode)
        };
        if !same {
            return Err(DIFFERENT.into());
        }
        return Ok(false);
    }
    atomic_file::create_new(path, content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(true)
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}
fn contained(base: &Path, target: &Path) -> bool {
    let base = path_key(base).trim_end_matches('\\').to_owned();
    let target = path_key(target);
    target == base || target.starts_with(&(base + "\\"))
}
fn canonical_future(path: &Path) -> Result<PathBuf, String> {
    if path.try_exists().map_err(|e| e.to_string())? {
        return fs::canonicalize(path).map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("결과 경로의 상위 폴더가 없습니다.")?;
    Ok(canonical_future(parent)?.join(path.file_name().ok_or("잘못된 결과 경로입니다.")?))
}
fn absolute(value: &str) -> Result<PathBuf, String> {
    std::path::absolute(value).map_err(|e| e.to_string())
}

pub(crate) struct ConversionPaths {
    pub(crate) input_root: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) files: Vec<PathBuf>,
    canonical_output: PathBuf,
}

impl ConversionPaths {
    pub(crate) fn target(&self, relative: &Path, extension: &str) -> Result<PathBuf, String> {
        let target = self.output.join(relative).with_extension(extension);
        // 디렉터리 연결로 결과 경계가 바뀌면 생성·읽기 전에 거부한다.
        if !contained(&self.canonical_output, &canonical_future(&target)?) {
            return Err("결과 파일이 지정한 결과 폴더 밖을 가리킵니다.".into());
        }
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        Ok(target)
    }
}
fn list_files(
    directory: &Path,
    extension: &str,
    mode: TextMode,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by(|a, b| {
        a.file_name()
            .to_string_lossy()
            .encode_utf16()
            .cmp(b.file_name().to_string_lossy().encode_utf16())
    });
    let mut directories = vec![];
    for entry in entries {
        #[cfg(windows)]
        if mode == TextMode::PowerShell {
            use std::os::windows::fs::MetadataExt;
            // Get-ChildItem 기본값은 숨김 파일·폴더를 제외한다. 시스템 단독은 포함한다.
            if entry
                .metadata()
                .map_err(|e| e.to_string())?
                .file_attributes()
                & 2
                != 0
            {
                continue;
            }
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            directories.push(entry.path());
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension))
        {
            output.push(entry.path());
        }
    }
    for directory in directories {
        list_files(&directory, extension, mode, output)?;
    }
    Ok(())
}

pub(crate) fn conversion_paths(
    input: &str,
    output: &str,
    kind: &str,
    mode: TextMode,
) -> Result<ConversionPaths, String> {
    let input = absolute(input)?;
    let output = absolute(output)?;
    let metadata = fs::metadata(&input).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    if mode == TextMode::PowerShell {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 2 != 0 {
            return Err("숨김 입력은 기존 PowerShell 경로에서 읽을 수 없습니다.".into());
        }
    }
    if !metadata.is_dir()
        && (!metadata.is_file()
            || !input
                .extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(kind)))
    {
        return Err(format!("입력 파일은 .{kind} 형식이어야 합니다."));
    }
    let canonical_input = fs::canonicalize(&input).map_err(|e| e.to_string())?;
    let canonical_output = canonical_future(&output)?;
    if metadata.is_dir()
        && (contained(&input, &output) || contained(&canonical_input, &canonical_output))
    {
        return Err("결과 폴더는 입력 폴더 밖으로 지정하세요.".into());
    }
    let input_root = if metadata.is_dir() {
        input.clone()
    } else {
        input.parent().unwrap().to_path_buf()
    };
    let mut files = vec![];
    if metadata.is_dir() {
        list_files(&input, kind, mode, &mut files)?;
    } else {
        files.push(input);
    }
    if mode == TextMode::PowerShell {
        crate::windows_sort::sort_paths(&mut files)?;
    }
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    Ok(ConversionPaths {
        input_root,
        output,
        files,
        canonical_output,
    })
}

fn plain_content(
    bytes: &[u8],
    kind: &str,
    mode: TextMode,
    filename: &str,
    day: &str,
) -> Result<String, String> {
    let archive = match mode {
        TextMode::Node => office_zip::read_zip(bytes)?,
        TextMode::PowerShell => office_zip::read_zip_powershell(bytes)?,
    };
    let mut sources = vec![];
    if kind == "docx" {
        let entry = archive
            .entries
            .iter()
            .find(|e| {
                if mode == TextMode::PowerShell {
                    e.name
                        .replace('\\', "/")
                        .eq_ignore_ascii_case("word/document.xml")
                } else {
                    e.name == "word/document.xml"
                }
            })
            .ok_or("DOCX 본문(word/document.xml)이 없습니다.")?;
        sources.push(archive.read_entry(entry)?);
    } else {
        let pattern = if mode == TextMode::PowerShell {
            &*SECTION_PS
        } else {
            &*SECTION
        };
        let mut sections = vec![];
        for (index, entry) in archive.entries.iter().enumerate() {
            if mode == TextMode::Node
                && archive.entries[..index]
                    .iter()
                    .any(|e| e.name == entry.name)
            {
                continue;
            }
            if let Some(c) = pattern.captures(&entry.name) {
                let number = if mode == TextMode::PowerShell {
                    c[1].parse::<i32>()
                        .map_err(|_| "HWPX 본문 번호가 너무 큽니다.")? as f64
                } else {
                    c[1].parse::<f64>().unwrap_or(f64::INFINITY)
                };
                sections.push((number, entry));
            }
        }
        if sections.is_empty() {
            return Err("HWPX 본문(Contents/section*.xml)이 없습니다.".into());
        }
        sections.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, entry) in sections {
            sources.push(archive.read_entry(entry)?);
        }
    }
    let eol = if mode == TextMode::PowerShell {
        "\r\n"
    } else {
        "\n"
    };
    let mut content = format!("---{eol}출처: {filename}{eol}변환일: {day}{eol}---{eol}{eol}");
    for bytes in sources {
        let xml = decode_text(&bytes, mode)?;
        for paragraph in office_text::paragraphs(&xml, kind == "docx", mode)? {
            let text = paragraph.trim_matches(office_text::net_space);
            if !text.is_empty() {
                content.push_str(text);
                content.push_str(eol);
                content.push_str(eol);
            }
        }
    }
    Ok(content)
}

fn execute(args: &[OsString], kind: &str, stdout: &mut String) -> Result<i32, String> {
    let usage = format!(
        "사용법: my-system convert-{kind} --input \"원본 파일 또는 폴더\" --output \"결과 폴더\"{}",
        if kind == "hwpx" {
            " [--structure-only]"
        } else {
            ""
        }
    );
    let strings: Vec<_> = args
        .iter()
        .map(|s| {
            s.to_str()
                .ok_or("인자는 유효한 Unicode 문자열이어야 합니다.")
        })
        .collect::<Result<_, _>>()?;
    if strings == ["--help"] {
        stdout.push_str(&format!("{usage}\n"));
        return Ok(0);
    }
    let mut input = None;
    let mut output = None;
    let mut structure = false;
    let mut mode = TextMode::PowerShell;
    let mut has_mode = false;
    let mut i = 0;
    while i < strings.len() {
        let flag = strings[i];
        if flag == "--structure-only" && kind == "hwpx" && !structure {
            structure = true;
            i += 1;
            continue;
        }
        let value = *strings
            .get(i + 1)
            .filter(|s| !crate::input::trim(s).is_empty() && !s.starts_with("--"))
            .ok_or_else(|| usage.clone())?;
        match flag {
            "--input" if input.is_none() => input = Some(value),
            "--output" if output.is_none() => output = Some(value),
            "--text-mode" if !has_mode => {
                mode = match value {
                    "node" => TextMode::Node,
                    "powershell" => TextMode::PowerShell,
                    _ => return Err(usage),
                };
                has_mode = true;
            }
            _ => return Err(usage),
        }
        i += 2;
    }
    let paths = conversion_paths(
        input.ok_or_else(|| usage.clone())?,
        output.ok_or(usage)?,
        kind,
        mode,
    )?;
    let files = &paths.files;
    if files.is_empty() {
        stdout.push_str(&format!(
            "지정한 입력 폴더에 변환할 .{kind} 파일이 없습니다.\n"
        ));
        return Ok(0);
    }
    stdout.push_str(&format!("총 {}개 파일을 변환합니다...\n", files.len()));
    let day = Local::now().format("%Y-%m-%d").to_string();
    let (mut ok, mut fail, mut skip) = (0, 0, 0);
    for (n, file) in files.iter().enumerate() {
        let relative = file
            .strip_prefix(&paths.input_root)
            .map_err(|e| e.to_string())?;
        let target = paths.target(relative, if structure { "structure.json" } else { "md" })?;
        let converted = (|| {
            let bytes = fs::read(file).map_err(|e| e.to_string())?;
            let content = if structure {
                let data = if mode == TextMode::PowerShell {
                    crate::hwpx_structure::read_structure(&bytes)?
                } else {
                    crate::hwpx_document::load_hwpx(&bytes)?.structure
                };
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?
                )
            } else {
                plain_content(
                    &bytes,
                    kind,
                    mode,
                    &file.file_name().unwrap().to_string_lossy(),
                    &day,
                )?
            };
            save(&target, &content, mode, structure)
        })();
        let status = match converted {
            Ok(true) => {
                ok += 1;
                if structure { "구조 완료" } else { "완료" }.into()
            }
            Ok(false) => {
                skip += 1;
                if structure {
                    if mode == TextMode::Node {
                        "구조 재사용"
                    } else {
                        "건너뜀(구조 대조 완료)"
                    }
                } else {
                    "건너뜀(원본 대조 완료)"
                }
                .into()
            }
            Err(error) => {
                fail += 1;
                format!("실패: {} — {error}", relative.display())
            }
        };
        if status.starts_with("실패:") {
            stdout.push_str(&format!("[{}/{}] {status}\n", n + 1, files.len()));
        } else {
            stdout.push_str(&format!(
                "[{}/{}] {status}: {}\n",
                n + 1,
                files.len(),
                relative.display()
            ));
        }
    }
    stdout.push_str(&format!(
        "\n끝났습니다. 성공 {ok}, 실패 {fail}, 건너뜀 {skip}\n결과 위치: {}\n",
        paths.output.display()
    ));
    Ok(i32::from(fail > 0))
}

pub fn run(args: &[OsString], kind: &str) -> (i32, String, String) {
    let mut stdout = String::new();
    match execute(args, kind, &mut stdout) {
        Ok(code) => (code, stdout, String::new()),
        Err(error) => (1, stdout, format!("[오류] {error}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_markdown_reuses_only_header_date_and_never_overwrites() {
        let base = tempfile::tempdir().unwrap();
        let file = base.path().join("기존.md");
        let existing = "\u{feff}---\r\n출처: 합성.docx\r\n변환일: 2000-01-01\r\n---\r\n\r\n본문 2000-01-01\r\n\r\n";
        fs::write(&file, existing).unwrap();
        let content = "---\n출처: 합성.docx\n변환일: 2026-01-01\n---\n\n본문 2000-01-01\n\n";
        assert!(!save(&file, content, TextMode::PowerShell, false).unwrap());
        assert!(
            save(
                &file,
                &content.replace("본문 2000", "본문 2001"),
                TextMode::PowerShell,
                false
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), existing);
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 1);
    }
}
