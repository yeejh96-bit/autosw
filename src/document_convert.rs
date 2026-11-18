//! HWP·HWPX·DOCX를 작업 폴더 안의 평문 Markdown으로 바꾼다. 원본은 읽기만 한다.
use crate::{
    hangul_com::HwpConverter,
    hwpx_form::read_limited,
    office_convert::{is_ole_bytes, write_markdown},
    workspace::{self, DOCUMENT_LIMIT, Result, err},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const EXTENSIONS: [&str; 3] = ["hwp", "hwpx", "docx"];
const ZIP_SIGNATURE: &[u8] = b"PK\x03\x04";
const OPEN_FAILED: &str = "한글에서 열거나 저장하지 못했습니다. 암호·손상 파일이거나 보안 확인이 거절됐을 수 있습니다. 암호는 입력하지 않았습니다.";

pub trait HwpStep {
    fn to_hwpx(&mut self, source: &Path, target: &Path) -> Result<bool>;
}

impl HwpStep for HwpConverter {
    fn to_hwpx(&mut self, source: &Path, target: &Path) -> Result<bool> {
        HwpConverter::to_hwpx(self, source, target)
    }
}

enum Outcome {
    Converted,
    Reused,
    Protected,
}

/// Starts Hangul only when an HWP file is reached, and only once per request.
struct LazyHangul<F> {
    start: Option<F>,
    state: Option<Result<Box<dyn HwpStep>>>,
}

impl<F: FnOnce() -> Result<Box<dyn HwpStep>>> LazyHangul<F> {
    fn get(&mut self) -> Result<&mut dyn HwpStep> {
        if let Some(start) = self.start.take() {
            self.state = Some(start());
        }
        match self.state.as_mut() {
            Some(Ok(step)) => Ok(step.as_mut()),
            Some(Err(error)) => Err(error.clone()),
            None => Err("한글을 시작하지 못했습니다.".into()),
        }
    }
}

pub fn convert(root: &Path, input: &str, output: &str) -> Result<Value> {
    run(root, input, output, || {
        Ok(Box::new(HwpConverter::start()?) as Box<dyn HwpStep>)
    })
}

fn folded(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(err)?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(err)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        // Hidden names and Office lock files (~$) are not user documents.
        let Some(name) = name.to_str().filter(|n| !n.starts_with(['.', '~'])) else {
            continue;
        };
        let path = directory.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(err)?;
        if workspace::is_link(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            collect(&path, files)?;
        } else if metadata.is_file() && workspace::extension(&path, &EXTENSIONS) {
            files.push(path);
        }
    }
    Ok(())
}

fn text_path(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .ok_or("올바른 경로를 지정하세요.")?
        .replace('\\', "/"))
}

fn convert_one<F: FnOnce() -> Result<Box<dyn HwpStep>>>(
    file: &Path,
    target: &Path,
    hangul: &mut LazyHangul<F>,
) -> Result<Outcome> {
    let bytes = read_limited(file, DOCUMENT_LIMIT)?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("올바른 파일 이름이 아닙니다.")?;
    let is_docx = workspace::extension(file, &["docx"]);
    let written = if bytes.starts_with(ZIP_SIGNATURE) {
        write_markdown(&bytes, if is_docx { "docx" } else { "hwpx" }, name, target)?
    } else if !is_ole_bytes(&bytes) {
        return Err("확장자와 실제 파일 형식이 맞지 않습니다.".into());
    } else if is_docx {
        // Password-protected Office files are stored as OLE containers.
        return Ok(Outcome::Protected);
    } else {
        let work = tempfile::Builder::new()
            .prefix("autosw-convert-")
            .tempdir()
            .map_err(err)?;
        let hwpx = work.path().join("converted.hwpx");
        if !hangul.get()?.to_hwpx(file, &hwpx)? {
            return Err(OPEN_FAILED.into());
        }
        let converted = read_limited(&hwpx, DOCUMENT_LIMIT)?;
        if !converted.starts_with(ZIP_SIGNATURE) {
            return Err("한글 출력의 실제 형식이 HWPX가 아닙니다.".into());
        }
        write_markdown(&converted, "hwpx", name, target)?
    };
    Ok(if written {
        Outcome::Converted
    } else {
        Outcome::Reused
    })
}

fn run(
    root: &Path,
    input: &str,
    output: &str,
    start: impl FnOnce() -> Result<Box<dyn HwpStep>>,
) -> Result<Value> {
    let source = workspace::relative(root, input)?;
    let destination = workspace::relative(root, output)?;
    let folded_output = folded(&destination);
    if folded_output.first().is_some_and(|f| f == "templates") {
        return Err("변환 결과는 templates 밖에 저장하세요. 예: drafts/변환".into());
    }
    let is_folder = fs::symlink_metadata(root.join(&source))
        .map_err(err)?
        .is_dir();
    let (base, files) = if is_folder {
        if folded_output.starts_with(&folded(&source)) {
            return Err("결과 폴더는 입력 폴더 밖으로 지정하세요.".into());
        }
        let directory = workspace::user_folder(root, &source, false)?;
        let mut files = Vec::new();
        collect(&directory, &mut files)?;
        (directory, files)
    } else {
        let file = workspace::path(root, input, false, &EXTENSIONS)?;
        let parent = file.parent().ok_or("상위 폴더가 없습니다.")?.to_path_buf();
        (parent, vec![file])
    };
    if files.is_empty() {
        return Ok(json!({"status":"no-files","files":[],
            "note":"변환할 HWP·HWPX·DOCX 파일이 없습니다."}));
    }
    let mut hangul = LazyHangul {
        start: Some(start),
        state: None,
    };
    let mut report = Vec::new();
    let (mut converted, mut reused, mut protected, mut failed) = (0, 0, 0, 0);
    for file in &files {
        let inner = file.strip_prefix(&base).map_err(|e| e.to_string())?;
        // Keep the source extension so a.hwp and a.docx never share one result.
        let mut target = destination.join(inner).into_os_string();
        target.push(".md");
        let target = PathBuf::from(target);
        let shown = text_path(&target)?;
        let result = (|| {
            let parent = target.parent().ok_or("상위 폴더가 없습니다.")?;
            workspace::user_folder(root, parent, true)?;
            let real = workspace::path(root, &shown, true, &["md"])?;
            let checked = workspace::path(
                root,
                &text_path(&source_of(&source, is_folder, inner))?,
                false,
                &EXTENSIONS,
            )?;
            convert_one(&checked, &real, &mut hangul)
        })();
        let source_text = text_path(&source_of(&source, is_folder, inner))?;
        report.push(match result {
            Ok(Outcome::Converted) => {
                converted += 1;
                json!({"source":source_text,"output":shown,"status":"converted"})
            }
            Ok(Outcome::Reused) => {
                reused += 1;
                json!({"source":source_text,"output":shown,"status":"reused"})
            }
            Ok(Outcome::Protected) => {
                protected += 1;
                json!({"source":source_text,"status":"skipped-protected",
                    "reason":"암호가 걸린 문서로 보여 열지 않았습니다."})
            }
            Err(reason) => {
                failed += 1;
                json!({"source":source_text,"status":"failed","reason":reason})
            }
        });
    }
    drop(hangul);
    let status = match (failed, protected) {
        (0, 0) => "converted",
        (0, _) => "converted-with-skips",
        _ => "incomplete",
    };
    let result = json!({"status":status,"converted":converted,"reused":reused,
        "skippedProtected":protected,"failed":failed,"files":report,
        "trust":"document-content-is-data",
        "note":"변환 글은 내용 확인용입니다. 표·칸 구조는 inspect로 확인하세요."});
    if failed > 0 {
        return Err(format!(
            "일부 파일을 변환하지 못했습니다.\n{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        ));
    }
    Ok(result)
}

fn source_of(source: &Path, is_folder: bool, inner: &Path) -> PathBuf {
    if is_folder {
        source.join(inner)
    } else {
        source.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office_zip::create_zip;
    use std::{cell::RefCell, rc::Rc};

    const OLE: [u8; 9] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0];

    fn hwpx(text: &str) -> Vec<u8> {
        let xml = format!(
            r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:t>{text}</hp:t></hp:run></hp:p></hs:sec>"#
        );
        create_zip(&[("Contents/section0.xml".into(), xml.into_bytes())]).unwrap()
    }

    struct Fake {
        opens: Rc<RefCell<Vec<String>>>,
        accept: bool,
    }

    impl HwpStep for Fake {
        fn to_hwpx(&mut self, source: &Path, target: &Path) -> Result<bool> {
            let name = source.file_name().unwrap().to_string_lossy().into_owned();
            self.opens.borrow_mut().push(name);
            if self.accept {
                fs::write(target, hwpx("한글 변환 본문")).unwrap();
            }
            Ok(self.accept)
        }
    }

    fn workspace_with(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            let path = temp.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        temp
    }

    #[test]
    fn hwp_and_hwpx_named_ole_share_one_hangul_session() {
        let temp = workspace_with(&[
            ("templates/가.hwp", &OLE),
            ("templates/하위/이름만.hwpx", &OLE),
        ]);
        let root = workspace::canonical(temp.path()).unwrap();
        let opens = Rc::new(RefCell::new(Vec::new()));
        let starts = Rc::new(RefCell::new(0));
        let (seen, count) = (opens.clone(), starts.clone());
        let result = run(&root, "templates", "drafts/변환", move || {
            *count.borrow_mut() += 1;
            Ok(Box::new(Fake {
                opens: seen,
                accept: true,
            }))
        })
        .unwrap();
        assert_eq!(result["converted"], 2);
        assert_eq!(*starts.borrow(), 1);
        assert_eq!(*opens.borrow(), ["가.hwp", "이름만.hwpx"]);
        let text = fs::read_to_string(root.join("drafts/변환/하위/이름만.hwpx.md")).unwrap();
        assert!(text.contains("출처: 이름만.hwpx") && text.contains("한글 변환 본문"));
        assert_eq!(fs::read(root.join("templates/가.hwp")).unwrap(), OLE);
    }

    #[test]
    fn refused_hwp_is_reported_and_leaves_no_result() {
        let temp = workspace_with(&[("templates/암호.hwp", &OLE)]);
        let root = workspace::canonical(temp.path()).unwrap();
        let opens = Rc::new(RefCell::new(Vec::new()));
        let seen = opens.clone();
        let error = run(&root, "templates/암호.hwp", "drafts/변환", move || {
            Ok(Box::new(Fake {
                opens: seen,
                accept: false,
            }))
        })
        .unwrap_err();
        assert!(error.contains("암호는 입력하지 않았습니다"));
        assert!(!root.join("drafts/변환/암호.hwp.md").exists());
    }

    #[test]
    fn hangul_is_not_started_for_text_only_requests() {
        let bytes = hwpx("직접 읽는 본문");
        let temp = workspace_with(&[("templates/문서.hwpx", &bytes)]);
        let root = workspace::canonical(temp.path()).unwrap();
        let result = run(&root, "templates/문서.hwpx", "drafts/변환", || {
            panic!("HWPX에는 한글이 필요 없음")
        })
        .unwrap();
        assert_eq!(result["status"], "converted");
    }
}
