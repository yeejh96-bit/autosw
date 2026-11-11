//! Windows Hancom Automation entry points. File operations stay separate from COM calls.
use crate::{atomic_file, hangul_preview, hwpx_form, office_convert, office_text::TextMode};
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(windows)]
mod native;

trait Hangul {
    fn open(&mut self, path: &Path, format: &str, options: &str) -> Result<bool, String>;
    fn save(&mut self, path: &Path, format: &str) -> Result<bool, String>;
    fn clear(&mut self) -> Result<(), String>;
    fn close_document(&mut self) -> Result<(), String>;
    fn pages(&mut self) -> Result<i32, String>;
}

#[cfg(windows)]
fn start(interactive: bool) -> Result<Box<dyn Hangul>, String> {
    Ok(Box::new(native::Session::start(interactive)?))
}

#[cfg(not(windows))]
fn start(_: bool) -> Result<Box<dyn Hangul>, String> {
    Err("한글 자동화는 한글이 설치된 Windows에서만 실행할 수 있습니다.".into())
}

pub fn run_convert(args: &[OsString], output: &mut impl Write) -> Result<i32, String> {
    convert(args, output, || start(true))
}

fn convert(
    args: &[OsString],
    mut output: impl Write,
    start: impl FnOnce() -> Result<Box<dyn Hangul>, String>,
) -> Result<i32, String> {
    const USAGE: &str = "convert-hwp --input 원본파일또는폴더 --output 결과폴더";
    let line = |output: &mut dyn Write, message: &str| {
        writeln!(output, "{message}").map_err(|e| e.to_string())
    };
    if args == ["--help"] {
        line(&mut output, USAGE)?;
        return Ok(0);
    }
    let mut input = None;
    let mut destination = None;
    if args.len() != 4 {
        return Err(USAGE.into());
    }
    for pair in args.chunks_exact(2) {
        let value = pair[1]
            .to_str()
            .filter(|value| !crate::input::trim(value).is_empty())
            .ok_or(USAGE)?;
        match pair[0].to_str() {
            Some("--input") if input.is_none() => input = Some(value),
            Some("--output") if destination.is_none() => destination = Some(value),
            _ => return Err(USAGE.into()),
        }
    }
    let paths = office_convert::conversion_paths(
        input.ok_or(USAGE)?,
        destination.ok_or(USAGE)?,
        "hwp",
        TextMode::PowerShell,
    )?;
    if paths.files.is_empty() {
        line(
            &mut output,
            "지정한 입력 폴더에 변환할 .hwp 파일이 없습니다.",
        )?;
        return Ok(0);
    }
    line(
        &mut output,
        &format!("총 {}개 파일을 변환합니다...", paths.files.len()),
    )?;
    line(
        &mut output,
        "한글이 자동으로 켜집니다. 변환 중에는 다른 한글 창을 건드리지 마세요.",
    )?;
    line(&mut output, "")?;
    output.flush().map_err(|e| e.to_string())?;

    let mut hangul = start()?;
    let (mut saved, mut skipped) = (0, 0);
    let mut failed = Vec::new();
    for (index, source) in paths.files.iter().enumerate() {
        let relative = source
            .strip_prefix(&paths.input_root)
            .map_err(|e| e.to_string())?;
        let target = paths.target(relative, "hwpx")?;
        let status = if target.try_exists().map_err(|e| e.to_string())? {
            skipped += 1;
            "건너뜀(이미 변환됨)"
        } else if convert_file(hangul.as_mut(), source, &target)? {
            saved += 1;
            "완료"
        } else {
            failed.push(relative);
            "실패"
        };
        line(
            &mut output,
            &format!(
                "[{}/{}] {status}: {}",
                index + 1,
                paths.files.len(),
                relative.display()
            ),
        )?;
        output.flush().map_err(|e| e.to_string())?;
    }
    drop(hangul);

    line(
        &mut output,
        &format!(
            "\n끝났습니다. 성공 {saved}, 실패 {}, 건너뜀 {skipped}",
            failed.len()
        ),
    )?;
    line(
        &mut output,
        &format!("결과 위치: {}", paths.output.display()),
    )?;
    if !failed.is_empty() {
        line(&mut output, "실패한 파일:")?;
        for relative in &failed {
            line(&mut output, &format!(" - {}", relative.display()))?;
        }
    }
    Ok(i32::from(!failed.is_empty()))
}

fn convert_file(hangul: &mut dyn Hangul, source: &Path, target: &Path) -> Result<bool, String> {
    let work = tempfile::Builder::new()
        .prefix(".convert-")
        .tempdir_in(target.parent().unwrap())
        .map_err(|e| e.to_string())?;
    let temporary = work.path().join("converted.hwpx");
    let result = (|| {
        if !hangul.open(source, "HWP", "")? {
            return Err("열기 거부됨".into());
        }
        save(hangul, &temporary, "HWPX", "HWPX 저장 실패")
    })();
    let closed = hangul.close_document();
    let complete = result.and(closed).is_ok() && publish(&temporary, target).is_ok();
    work.close()
        .map_err(|e| format!("HWP 임시 결과 정리 실패: {e}"))?;
    Ok(complete)
}

fn save(hangul: &mut dyn Hangul, path: &Path, format: &str, message: &str) -> Result<(), String> {
    if !hangul.save(path, format)?
        || !fs::symlink_metadata(path).is_ok_and(|info| info.is_file() && info.len() > 0)
    {
        return Err(message.into());
    }
    Ok(())
}

fn publish(source: &Path, target: &Path) -> io::Result<()> {
    // Stage in the destination directory so publication stays on the same volume.
    let mut temporary = tempfile::Builder::new()
        .prefix(".convert-output-")
        .tempfile_in(target.parent().unwrap())?;
    io::copy(&mut fs::File::open(source)?, &mut temporary)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(target)
        .map(drop)
        .map_err(|error| error.error)
}

fn absolute(path: &OsString) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("파일 경로를 지정하세요.".into());
    }
    std::path::absolute(path).map_err(|e| e.to_string())
}

fn extension(path: &Path, allowed: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(allowed))
}

fn fresh_output(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Err(format!("기존 파일을 덮어쓰지 않습니다: {}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(format!("출력 폴더가 없습니다: {}", path.display()));
    }
    Ok(())
}

fn distinct_outputs(a: &Path, b: &Path) -> Result<(), String> {
    let real = |path: &Path| -> Result<PathBuf, String> {
        let parent = path.parent().ok_or("출력 폴더를 지정하세요.")?;
        Ok(fs::canonicalize(parent)
            .map_err(|e| e.to_string())?
            .join(path.file_name().ok_or("파일명을 지정하세요.")?))
    };
    let a = real(a)?;
    let b = real(b)?;
    if a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() {
        return Err("입력과 출력은 서로 다른 경로여야 합니다.".into());
    }
    Ok(())
}

fn validate_source_pdf(source: &Path, pdf: &Path) -> Result<(), String> {
    if !extension(source, "hwp") && !extension(source, "hwpx") {
        return Err("HWP 또는 HWPX 파일이 필요합니다.".into());
    }
    if !source.is_file() {
        return Err(format!("원본 파일이 없습니다: {}", source.display()));
    }
    if !extension(pdf, "pdf") {
        return Err("출력은 PDF여야 합니다.".into());
    }
    fresh_output(pdf)
}

struct RenderedCopy {
    work: tempfile::TempDir,
    source_hash: String,
    input_pages: i32,
    saved_pages: i32,
}

const CHECKED_HWPX: &str = "checked.hwpx";
const CHECKED_PDF: &str = "checked.pdf";
const CHECKED_HWP: &str = "checked.hwp";

fn render_copy(
    source: &Path,
    start: impl FnOnce() -> Result<Box<dyn Hangul>, String>,
    hwp_output: bool,
) -> Result<RenderedCopy, String> {
    let source_hash = hangul_preview::hash_file(source)?;
    let work = tempfile::Builder::new()
        .prefix("mysystem-hangul-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let local_source = work
        .path()
        .join("source")
        .with_extension(source.extension().ok_or("원본 확장자가 없습니다.")?);
    let local_hwpx = work.path().join(CHECKED_HWPX);
    let local_pdf = work.path().join(CHECKED_PDF);
    let local_hwp = work.path().join(CHECKED_HWP);
    fs::copy(source, &local_source).map_err(|e| e.to_string())?;
    if hangul_preview::hash_file(&local_source)? != source_hash {
        return Err("원본이 작업 중 변경되어 결과를 공개하지 않았습니다.".into());
    }
    let mut hangul = start()?;
    let options = "suspendpassword:TRUE;forceopen:TRUE";
    if !hangul.open(&local_source, "", options)? {
        return Err("한글에서 원본을 열지 못했습니다.".into());
    }
    let input_pages = hangul.pages()?;
    let (checked, format) = if hwp_output {
        (&local_hwp, "HWP")
    } else {
        (&local_hwpx, "HWPX")
    };
    save(
        hangul.as_mut(),
        checked,
        format,
        "한글 저장에 실패했습니다.",
    )?;
    {
        use std::io::Read;
        let expected: &[u8] = if hwp_output {
            &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]
        } else {
            b"PK\x03\x04"
        };
        let mut magic = [0; 8];
        fs::File::open(checked)
            .and_then(|mut f| f.read_exact(&mut magic[..expected.len()]))
            .map_err(|e| e.to_string())?;
        if &magic[..expected.len()] != expected {
            return Err(format!("한글 출력의 실제 형식이 {format}이 아닙니다."));
        }
    }
    hangul.clear()?;
    if !hangul.open(checked, format, options)? {
        return Err("저장본을 다시 열지 못했습니다.".into());
    }
    let saved_pages = hangul.pages()?;
    save(
        hangul.as_mut(),
        &local_pdf,
        "PDF",
        "한글 PDF 출력에 실패했습니다.",
    )?;
    if hwp_output {
        save(
            hangul.as_mut(),
            &local_hwpx,
            "HWPX",
            "HWP 재열기 후 대조용 HWPX 저장에 실패했습니다.",
        )?;
    }
    drop(hangul);
    if hangul_preview::hash_file(source)? != source_hash {
        return Err("원본이 작업 중 변경되어 결과를 공개하지 않았습니다.".into());
    }
    Ok(RenderedCopy {
        work,
        source_hash,
        input_pages,
        saved_pages,
    })
}

pub fn run_verify(args: &[OsString]) -> Result<String, String> {
    verify(args, || start(false))
}

fn verify(
    args: &[OsString],
    start: impl FnOnce() -> Result<Box<dyn Hangul>, String>,
) -> Result<String, String> {
    let [source, output, pdf] = args else {
        return Err("hangul-verify INPUT HWPX PDF".into());
    };
    let (source, output, pdf) = (absolute(source)?, absolute(output)?, absolute(pdf)?);
    validate_source_pdf(&source, &pdf)?;
    if !extension(&output, "hwpx") {
        return Err("출력은 HWPX와 PDF여야 합니다.".into());
    }
    fresh_output(&output)?;
    let rendered = render_copy(&source, start, false)?;
    publish(&rendered.work.path().join(CHECKED_HWPX), &output).map_err(|e| e.to_string())?;
    publish(&rendered.work.path().join(CHECKED_PDF), &pdf).map_err(|e| {
        format!(
            "HWPX는 저장됐지만 PDF 저장에 실패했습니다. HWPX: {}. 오류: {e}",
            output.display()
        )
    })?;
    rendered
        .work
        .close()
        .map_err(|e| format!("한글 검증 임시 파일 정리 실패: {e}"))?;
    Ok(format!(
        "{}\n",
        json!({"status":"awaiting-visual-review", "inputPages":rendered.input_pages, "savedPages":rendered.saved_pages, "output":output, "pdf":pdf})
    ))
}

pub fn verify_hwp(
    source: &Path,
    output: &Path,
    pdf: &Path,
    verification: &Path,
) -> Result<serde_json::Value, String> {
    validate_source_pdf(source, pdf)?;
    if !extension(output, "hwp") || !extension(verification, "hwpx") {
        return Err("출력은 HWP, PDF, 대조용 HWPX여야 합니다.".into());
    }
    for target in [output, verification] {
        fresh_output(target)?;
    }
    distinct_outputs(output, pdf)?;
    distinct_outputs(output, verification)?;
    distinct_outputs(pdf, verification)?;
    let rendered = render_copy(source, || start(false), true)?;
    let checked = fs::read(rendered.work.path().join(CHECKED_HWPX)).map_err(|e| e.to_string())?;
    hwpx_form::validate_independent(&checked)?;
    for (name, path) in [
        (CHECKED_HWP, output),
        (CHECKED_PDF, pdf),
        (CHECKED_HWPX, verification),
    ] {
        publish(&rendered.work.path().join(name), path).map_err(|e| {
            format!(
                "출력 저장 실패: {}. 먼저 저장된 결과가 있는지 확인하세요: {e}",
                path.display()
            )
        })?;
    }
    rendered
        .work
        .close()
        .map_err(|e| format!("한글 검증 임시 파일 정리 실패: {e}"))?;
    Ok(
        json!({"status":"awaiting-visual-review","inputPages":rendered.input_pages,"savedPages":rendered.saved_pages,"output":output,"pdf":pdf,"verificationHwpx":verification,"format":"HWP"}),
    )
}

pub fn run_preview(args: &[OsString]) -> Result<String, String> {
    preview(args, || start(false))
}

fn preview(
    args: &[OsString],
    start: impl FnOnce() -> Result<Box<dyn Hangul>, String>,
) -> Result<String, String> {
    let [source, pdf, report] = args else {
        return Err("hangul-preview create INPUT PDF REPORT".into());
    };
    let (source, pdf, report) = (absolute(source)?, absolute(pdf)?, absolute(report)?);
    validate_source_pdf(&source, &pdf)?;
    fresh_output(&report)?;
    distinct_outputs(&pdf, &report)?;
    let rendered = render_copy(&source, start, false)?;
    publish(&rendered.work.path().join(CHECKED_PDF), &pdf).map_err(|e| e.to_string())?;
    let result = (|| {
        let result = hangul_preview::bind_preview(
            &source,
            &pdf,
            &rendered.source_hash,
            f64::from(rendered.saved_pages),
        )?;
        let text = hwpx_form::pretty(&result)?;
        atomic_file::create_new(&report, text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(text)
    })();
    let cleaned = rendered
        .work
        .close()
        .map_err(|e| format!("한글 미리보기 임시 파일 정리 실패: {e}"));
    result
        .map_err(|e: String| {
            format!("미리보기 기록 실패. 생성된 PDF는 최신 확인을 통과하지 못했습니다. {e}")
        })
        .and_then(|text| cleaned.map(|()| text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    const HWPX: &[u8] = b"PK\x03\x04synthetic HWPX";
    const PDF: &[u8] = b"%PDF-1.7\nsynthetic PDF";
    type Events = Rc<RefCell<Vec<&'static str>>>;

    struct Mock {
        mode: &'static str,
        events: Events,
        changed_file: Option<PathBuf>,
    }

    impl Hangul for Mock {
        fn open(&mut self, path: &Path, format: &str, options: &str) -> Result<bool, String> {
            assert!(path.is_file());
            let event = match format {
                "" => "open_copy",
                "HWP" => "open_hwp",
                "HWPX" => "reopen",
                _ => unreachable!(),
            };
            if format != "HWP" {
                assert_eq!(options, "suspendpassword:TRUE;forceopen:TRUE");
            }
            self.events.borrow_mut().push(event);
            Ok(self.mode != event)
        }

        fn save(&mut self, path: &Path, format: &str) -> Result<bool, String> {
            self.events.borrow_mut().push(if format == "HWPX" {
                "save_hwpx"
            } else {
                "save_pdf"
            });
            let body = match (format, self.mode) {
                ("PDF", "missing_pdf") | ("HWPX", "missing") => return Ok(true),
                ("PDF", "invalid_pdf") => b"invalid PDF",
                ("PDF", _) => PDF,
                ("HWPX", "empty") => b"",
                ("HWPX", "invalid_hwpx") => b"invalid HWPX",
                ("HWPX", _) => HWPX,
                _ => unreachable!(),
            };
            fs::write(path, body).unwrap();
            if let Some(changed) = &self.changed_file {
                fs::write(changed, b"simultaneous write").unwrap();
            }
            Ok(self.mode != "partial")
        }

        fn clear(&mut self) -> Result<(), String> {
            self.events.borrow_mut().push("clear");
            Ok(())
        }

        fn close_document(&mut self) -> Result<(), String> {
            self.events.borrow_mut().push("close");
            if self.mode == "close" {
                return Err("close failure".into());
            }
            Ok(())
        }

        fn pages(&mut self) -> Result<i32, String> {
            self.events.borrow_mut().push("pages");
            Ok(2)
        }
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            self.events.borrow_mut().push("quit");
        }
    }

    fn mock(events: &Events, mode: &'static str, changed_file: Option<PathBuf>) -> Box<dyn Hangul> {
        Box::new(Mock {
            mode,
            events: events.clone(),
            changed_file,
        })
    }

    fn paths(base: &Path) -> [OsString; 3] {
        ["한글 # [원본].hwpx", "새 HWPX.hwpx", "새 PDF.pdf"]
            .map(|part| base.join(part).into_os_string())
    }

    #[test]
    fn conversion_continues_after_failure_and_retries_only_missing_results() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("원본 #");
        let destination = root.path().join("결과");
        fs::create_dir_all(input.join("하위")).unwrap();
        let source = input.join("하위/원본.hwp");
        fs::write(&source, b"unchanged").unwrap();
        let args = [
            "--input".into(),
            input.into_os_string(),
            "--output".into(),
            destination.clone().into_os_string(),
        ];
        for mode in ["open_hwp", "partial", "missing", "empty", "close"] {
            let events = Events::default();
            let mut output = Vec::new();
            assert_eq!(
                convert(&args, &mut output, || Ok(mock(&events, mode, None))).unwrap(),
                1
            );
            assert!(
                String::from_utf8(output)
                    .unwrap()
                    .contains("성공 0, 실패 1, 건너뜀 0")
            );
            assert_eq!(
                &events.borrow()[events.borrow().len() - 2..],
                &["close", "quit"]
            );
            assert_eq!(fs::read_dir(destination.join("하위")).unwrap().count(), 0);
        }
        let mut output = Vec::new();
        let events = Events::default();
        assert_eq!(
            convert(&args, &mut output, || Ok(mock(&events, "success", None))).unwrap(),
            0
        );
        assert_eq!(*events.borrow(), ["open_hwp", "save_hwpx", "close", "quit"]);
        let target = destination.join("하위/원본.hwpx");
        let before = target.metadata().unwrap().modified().unwrap();
        events.borrow_mut().clear();
        output.clear();
        assert_eq!(
            convert(&args, &mut output, || Ok(mock(&events, "partial", None))).unwrap(),
            0
        );
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("성공 0, 실패 0, 건너뜀 1")
        );
        assert_eq!(*events.borrow(), ["quit"]);
        assert_eq!(fs::read(&target).unwrap(), HWPX);
        assert_eq!(target.metadata().unwrap().modified().unwrap(), before);
        assert_eq!(fs::read(&source).unwrap(), b"unchanged");
    }

    #[test]
    fn verification_reopens_copy_and_checks_source_before_publication() {
        let root = tempfile::tempdir().unwrap();
        let args = paths(root.path());
        let [source, output, pdf] = args.each_ref().map(Path::new);
        fs::write(source, b"original").unwrap();
        for mode in [
            "open_copy",
            "partial",
            "reopen",
            "missing_pdf",
            "invalid_hwpx",
        ] {
            let events = Events::default();
            assert!(verify(&args, || Ok(mock(&events, mode, None))).is_err());
            assert_eq!(events.borrow().last(), Some(&"quit"));
            assert_eq!(fs::read(source).unwrap(), b"original");
            assert!(!output.exists() && !pdf.exists());
        }
        let events = Events::default();
        assert!(
            verify(&args, || Ok(mock(&events, "success", Some(source.into()))))
                .unwrap_err()
                .contains("원본이 작업 중 변경")
        );
        assert!(!output.exists() && !pdf.exists());
        events.borrow_mut().clear();
        fs::write(source, b"original").unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&verify(&args, || Ok(mock(&events, "success", None))).unwrap())
                .unwrap();
        assert_eq!(result["status"], "awaiting-visual-review");
        assert_eq!(result["inputPages"], 2);
        assert_eq!(result["savedPages"], 2);
        assert_eq!(
            *events.borrow(),
            [
                "open_copy",
                "pages",
                "save_hwpx",
                "clear",
                "reopen",
                "pages",
                "save_pdf",
                "quit"
            ]
        );
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert_eq!(fs::read(output).unwrap(), HWPX);
        assert_eq!(fs::read(pdf).unwrap(), PDF);
        assert!(
            verify(&args, || panic!(
                "기존 결과가 있을 때 한글을 시작하면 안 됨"
            ))
            .is_err()
        );
    }

    #[test]
    fn competing_files_are_preserved_and_partial_completion_is_explicit() {
        let root = tempfile::tempdir().unwrap();
        let args = paths(root.path());
        let [source, output, pdf] = args.each_ref().map(Path::new);
        fs::write(source, b"original").unwrap();
        let events = Events::default();
        let error = verify(&args, || Ok(mock(&events, "success", Some(pdf.into())))).unwrap_err();
        assert!(error.contains("HWPX는 저장됐지만 PDF 저장에 실패"));
        assert_eq!(fs::read(output).unwrap(), HWPX);
        assert_eq!(fs::read(pdf).unwrap(), b"simultaneous write");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    }

    #[test]
    fn preview_binds_output_and_rejects_existing_or_aliased_records() {
        let root = tempfile::tempdir().unwrap();
        let [source, _, pdf] = paths(root.path());
        let report = root.path().join("미리보기.json").into_os_string();
        fs::write(&source, b"original").unwrap();
        assert!(
            preview(&[source.clone(), pdf.clone(), pdf.clone()], || panic!(
                "출력 경로 별칭"
            ))
            .is_err()
        );
        let events = Events::default();
        let args = [source, pdf, report];
        let text = preview(&args, || Ok(mock(&events, "success", None))).unwrap();
        assert_eq!(fs::read_to_string(&args[2]).unwrap(), text);
        let stored = serde_json::from_str(&text).unwrap();
        assert_eq!(
            hangul_preview::verify_preview(&stored).unwrap()["pageCount"],
            2
        );
        assert!(preview(&args, || panic!("기존 PDF/보고서 덮어쓰기")).is_err());
        assert_eq!(fs::read(&args[1]).unwrap(), PDF);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    }
}
