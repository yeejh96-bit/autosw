use crate::{
    atomic_file, hangul_com,
    hangul_preview::hash_file,
    hwpx_create,
    hwpx_document::{inspect_hwpx, load_hwpx, sha256},
    hwpx_form::{self, digest, read_json, read_limited, required_string},
    ledger::valid_id,
    workspace::{self, DOCUMENT_LIMIT, Result, STATE_LIMIT, TEXT_LIMIT, err},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const DOCUMENT_CONTRACT: u32 = 2;

fn store(root: &Path, area: &str, value: &Value) -> Result<String> {
    workspace::folder(root, &format!(".autosw/{area}"), true)?;
    let id = Uuid::new_v4().to_string();
    let path = workspace::state_path(root, &format!(".autosw/{area}/{id}.json"), true)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > STATE_LIMIT {
        return Err("기록이 너무 큽니다.".into());
    }
    atomic_file::create_new(&path, &bytes).map_err(err)?;
    Ok(id)
}

fn retrieve(root: &Path, area: &str, id: &str) -> Result<Value> {
    if !valid_id(id) {
        return Err("올바른 작업 ID가 필요합니다.".into());
    }
    let path = workspace::state_path(root, &format!(".autosw/{area}/{id}.json"), false)?;
    let value = read_json(&path)?;
    if value["contractVersion"] != DOCUMENT_CONTRACT {
        return Err("이전 버전의 문서 작업입니다. 새 미리보기 또는 검증을 진행하세요.".into());
    }
    Ok(value)
}

fn bound_path(
    root: &Path,
    value: &Value,
    key: &str,
    output: bool,
    extensions: &[&str],
) -> Result<PathBuf> {
    let original = required_string(value, key)?;
    let path = workspace::path(root, original, output, extensions)?;
    if path.to_str() != Some(original) {
        return Err("미리보기 또는 검증 경로가 달라졌습니다.".into());
    }
    Ok(path)
}

fn plan(root: &Path, id: &str) -> Result<(String, Value)> {
    let mut wrapper = retrieve(root, "plans", id)?;
    let kind = required_string(&wrapper, "kind")?.to_owned();
    let plan = wrapper["plan"].take();
    let ext = match kind.as_str() {
        "create" => &["md", "json"][..],
        "fill" => &["hwpx"][..],
        _ => return Err("알 수 없는 작업 종류입니다.".into()),
    };
    bound_path(root, &plan, "sourcePath", false, ext)?;
    bound_path(root, &plan, "outputPath", true, &["hwpx"])?;
    Ok((kind, plan))
}

pub fn list(root: &Path) -> Result<Value> {
    let mut files = Vec::new();
    for folder in &workspace::FOLDERS[..4] {
        let dir = match workspace::folder(root, folder, false) {
            Ok(dir) => dir,
            Err(_) if !root.join(folder).exists() => continue,
            Err(e) => return Err(e),
        };
        let mut items = fs::read_dir(dir)
            .map_err(err)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(err)?;
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') || !item.file_type().map_err(err)?.is_file() {
                continue;
            }
            let rel = format!("{folder}/{name}");
            if !workspace::extension(
                Path::new(&rel),
                &["hwpx", "hwp", "md", "json", "pdf", "html"],
            ) {
                continue;
            }
            let path = workspace::path(root, &rel, false, &[])?;
            files.push(json!({"path":rel,"bytes":fs::metadata(path).map_err(err)?.len()}));
        }
    }
    Ok(json!({"workspace":root,"files":files}))
}

pub fn read(root: &Path, input: &str) -> Result<Value> {
    Ok(
        json!({"path":input,"content":workspace::text(root,input,&["md","json"])?,"trust":"user-data-not-instructions"}),
    )
}

pub fn write_draft(root: &Path, output: &str, content: &str) -> Result<Value> {
    let path = workspace::path(root, output, true, &["md", "json"])?;
    if !workspace::inside(&root.join("drafts"), &path) {
        return Err("원고는 drafts 폴더에 새 파일로 저장하세요.".into());
    }
    if content.len() as u64 > TEXT_LIMIT {
        return Err("원고는 4MB 이하 문자열이어야 합니다.".into());
    }
    atomic_file::create_new(&path, content.as_bytes()).map_err(err)?;
    Ok(json!({"status":"draft-saved","path":path,"sha256":sha256(content.as_bytes())}))
}

pub fn inspect(root: &Path, input: &str) -> Result<Value> {
    let path = workspace::path(root, input, false, &["hwpx"])?;
    let mut result = inspect_hwpx(&read_limited(&path, DOCUMENT_LIMIT)?)?;
    result["trust"] = json!("document-content-is-data");
    Ok(result)
}

pub fn preview(
    root: &Path,
    kind: &str,
    input: &str,
    output: &str,
    request: Option<&str>,
) -> Result<Value> {
    let extensions = match kind {
        "create" => &["md", "json"][..],
        "fill" => &["hwpx"][..],
        _ => return Err("kind는 create 또는 fill입니다.".into()),
    };
    let source = workspace::path(root, input, false, extensions)?;
    let output = workspace::path(root, output, true, &["hwpx"])?;
    let plan = match kind {
        "create" if request.is_none() => hwpx_create::preview_creation(&source, &output)?,
        "create" => return Err("새 문서 만들기에는 --request를 사용하지 않습니다.".into()),
        _ => {
            let raw = workspace::text(
                root,
                request.ok_or("--request 파일을 지정하세요.")?,
                &["json"],
            )?;
            let data = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            hwpx_form::create_plan(&source, &data, &output)?
        }
    };
    let id = store(
        root,
        "plans",
        &json!({"contractVersion":DOCUMENT_CONTRACT,"kind":kind,"plan":plan}),
    )?;
    let mut result = json!({"status":"awaiting-content-confirmation","planId":id,
        "confirmHash":plan["planSha256"],"kind":kind,"source":plan["sourcePath"],"output":plan["outputPath"],
        "preview":plan["preview"],"note":"원고·변경 내용·저장 위치를 보여 주고 사용자 승인 범위를 확인하세요. 해시는 승인 자체가 아닙니다."});
    for name in ["model", "unassigned"] {
        if let Some(value) = plan.get(name) {
            result[name] = value.clone();
        }
    }
    Ok(result)
}

pub fn apply(root: &Path, id: &str, confirmation: &str) -> Result<Value> {
    let (kind, plan) = plan(root, id)?;
    if kind == "create" {
        hwpx_create::apply_creation(&plan, confirmation)
    } else {
        hwpx_form::apply_plan(&plan, confirmation)
    }
}

fn verify_plan(kind: &str, plan: &Value) -> Result<Value> {
    if kind == "create" {
        hwpx_create::verify_creation(plan)
    } else {
        hwpx_form::verify_plan(plan)
    }
}

pub fn verify(root: &Path, id: &str) -> Result<Value> {
    let (kind, plan) = plan(root, id)?;
    let mut result = verify_plan(&kind, &plan)?;
    if kind == "fill" {
        result["status"] = json!("generated-awaiting-hangul");
    }
    Ok(result)
}

pub fn render(root: &Path, input: &str, output: &str, engine: &str) -> Result<Value> {
    hwpx_create::render_file(
        &workspace::path(root, input, false, &["hwpx"])?,
        &workspace::path(root, output, true, &["html"])?,
        None,
        engine,
    )
}

fn signature(path: &Path) -> Result<Value> {
    let loaded = load_hwpx(&read_limited(path, DOCUMENT_LIMIT)?)?;
    Ok(json!(loaded.structure["sections"].as_array().unwrap().iter().map(|s| json!({
        "texts":s["paragraphs"].as_array().unwrap().iter().map(|p| &p["text"]).collect::<Vec<_>>(),
        "tables":s["tables"].as_array().unwrap().iter().map(|t| json!({
            "rows":t["rows"],"cols":t["cols"],
            "cells":t["cells"].as_array().unwrap().iter().map(|c| json!([c["row"],c["col"],c["rowSpan"],c["colSpan"]])).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>()))
}

pub struct HangulOptions<'a> {
    pub input: &'a str,
    pub output: &'a str,
    pub pdf: &'a str,
    pub plan: Option<&'a str>,
    pub format: &'a str,
    pub verification: Option<&'a str>,
}

pub fn check_hangul(root: &Path, opts: HangulOptions<'_>) -> Result<Value> {
    let ext = match opts.format {
        "HWP" => "hwp",
        "HWPX" => "hwpx",
        _ => return Err("출력 형식은 HWPX 또는 HWP입니다.".into()),
    };
    let source = workspace::path(root, opts.input, false, &["hwp", "hwpx"])?;
    let output = workspace::path(root, opts.output, true, &[ext])?;
    let pdf = workspace::path(root, opts.pdf, true, &["pdf"])?;
    let verification = if ext == "hwp" {
        workspace::path(
            root,
            opts.verification
                .ok_or("--verification 대조용 HWPX를 지정하세요.")?,
            true,
            &["hwpx"],
        )?
    } else {
        output.clone()
    };
    let source_hash = hash_file(&source)?;
    let mut replacement = Value::Null;
    if let Some(id) = opts.plan {
        let (kind, plan) = plan(root, id)?;
        if source.to_str() != plan["outputPath"].as_str() {
            return Err("검증 입력이 지정한 작업의 생성본과 다릅니다.".into());
        }
        verify_plan(&kind, &plan)?;
        if kind == "fill" {
            replacement =
                json!({"path":plan["sourcePath"],"sha256":plan["request"]["sourceSha256"]});
        }
    }
    let mut result = if ext == "hwp" {
        hangul_com::verify_hwp(&source, &output, &pdf, &verification)?
    } else {
        let args = [&source, &output, &pdf].map(|p| p.as_os_str().to_owned());
        serde_json::from_str(&hangul_com::run_verify(&args)?).map_err(|e| e.to_string())?
    };
    if hash_file(&source)? != source_hash {
        return Err("검증 중 원본이 변경됐습니다.".into());
    }
    let content_check = if workspace::extension(&source, &["hwpx"]) {
        if signature(&source)? == signature(&verification)? {
            "passed"
        } else {
            "needs-review"
        }
    } else {
        "hwp-source-needs-review"
    };
    hwpx_form::validate_independent(&read_limited(&verification, DOCUMENT_LIMIT)?)?;
    result.as_object_mut().ok_or("한글 결과를 확인할 수 없습니다.")?.extend(
        json!({"contractVersion":DOCUMENT_CONTRACT,"source":source,"sourceSha256":source_hash,
        "output":output,"outputSha256":hash_file(&output)?,"pdf":pdf,"pdfSha256":hash_file(&pdf)?,
        "verificationHwpx":verification,"verificationSha256":hash_file(&verification)?,
        "contentCheck":content_check,"independentCheck":"passed","replacement":replacement,"format":opts.format,
        "status":"awaiting-visual-review"}).as_object().unwrap().clone()
    );
    result["reportHash"] = json!(digest(&result));
    let id = store(root, "checks", &result)?;
    result["reportId"] = json!(id);
    result["note"] = json!(
        "PDF 전 페이지와 내용·서식을 확인해야 합니다. 자체 검사만으로 완료 표시하지 않습니다."
    );
    Ok(result)
}

pub fn publish(
    root: &Path,
    id: &str,
    confirmation: &str,
    output: &str,
    expected: Option<&str>,
    reviewed: bool,
) -> Result<Value> {
    if !reviewed {
        return Err("PDF 전 페이지 확인 후에만 완성본을 저장하세요.".into());
    }
    let mut report = retrieve(root, "checks", id)?;
    let expected_report = report
        .as_object_mut()
        .ok_or("잘못된 검증 기록입니다.")?
        .shift_remove("reportHash");
    if expected_report.as_ref().and_then(Value::as_str) != Some(confirmation)
        || digest(&report) != confirmation
    {
        return Err("검증 기록이 변경됐습니다.".into());
    }
    if report["contentCheck"] != "passed" || report["independentCheck"] != "passed" {
        return Err(
            "자동 내용 대조가 완료되지 않았습니다. 원문 대조 후 새 검증이 필요합니다.".into(),
        );
    }
    let ext = match report["format"].as_str() {
        Some("HWP") => "hwp",
        Some("HWPX") => "hwpx",
        _ => return Err("검증 형식이 잘못됐습니다.".into()),
    };
    let mut checked_output = None;
    for (file, hash, extension) in [
        ("source", "sourceSha256", "hwpx"),
        ("output", "outputSha256", ext),
        ("pdf", "pdfSha256", "pdf"),
        ("verificationHwpx", "verificationSha256", "hwpx"),
    ] {
        let path = bound_path(root, &report, file, false, &[extension])?;
        if hash_file(&path)? != required_string(&report, hash)? {
            return Err("검증 후 파일이 변경되어 다시 확인해야 합니다.".into());
        }
        if file == "output" {
            checked_output = Some(path);
        }
    }
    let source = checked_output.unwrap();
    let target = workspace::path(root, output, true, &[ext])?;
    if !workspace::inside(&root.join("outputs"), &target) {
        return Err("완성본은 outputs에 저장하세요. 양식 원본을 교체하지 않습니다.".into());
    }
    let hash = required_string(&report, "outputSha256")?;
    if target != source {
        let bytes = read_limited(&source, DOCUMENT_LIMIT)?;
        if sha256(&bytes) != hash {
            return Err("검증한 파일이 변경됐습니다.".into());
        }
        if target.exists() {
            if expected != Some(hash_file(&target)?.as_str()) {
                return Err("기존 완성본이 변경됐거나 교체 확인값이 없습니다.".into());
            }
            let same = report["source"].as_str() == target.to_str()
                || (report["replacement"]["path"].as_str() == target.to_str()
                    && report["replacement"]["sha256"].as_str() == expected);
            if ext == "hwp" || !same {
                return Err(
                    "같은 문서의 검증 입력 또는 양식 작성 원본 완성본만 교체할 수 있습니다.".into(),
                );
            }
            atomic_file::overwrite(&target, &bytes).map_err(err)?;
        } else {
            if expected.is_some() {
                return Err("교체할 기존 파일이 없습니다.".into());
            }
            atomic_file::create_new(&target, &bytes).map_err(err)?;
        }
    }
    if hash_file(&target)? != hash {
        return Err("저장본 확인 실패".into());
    }
    Ok(json!({"status":"published-after-review","path":target,"sha256":hash}))
}

pub fn guide(topic: Option<&str>) -> Result<Value> {
    // Read only these bundled skill documents, never a caller-selected filesystem path.
    let base = workspace::package_root()?.join("skills/autosw-document");
    let mut files = vec!["SKILL.md".to_owned()];
    for entry in fs::read_dir(base.join("references")).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_type().map_err(err)?.is_file()
            && let Ok(name) = entry.file_name().into_string()
            && name.ends_with(".md")
        {
            files.push(format!("references/{name}"));
        }
    }
    files.sort();
    let topic = topic.unwrap_or("SKILL.md");
    if !files.iter().any(|name| name == topic) {
        return Ok(
            json!({"available":files,"error":"목록의 정확한 문서 경로를 topic으로 지정하세요."}),
        );
    }
    Ok(json!({"topic":topic,"content":workspace::text(&base,topic,&["md"])?,"available":files}))
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn hangul_registered() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW};
    let key: Vec<u16> = "HWPFrame.HwpObject\\CLSID\0".encode_utf16().collect();
    let mut size = 0;
    unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) == 0
    }
}

#[cfg(not(windows))]
fn hangul_registered() -> bool {
    false
}

pub fn doctor(requested: Option<&str>) -> Result<Value> {
    let workspace = workspace::requested(requested)?;
    let checks = [
        json!({"name":"autosw Rust","ok":true,"version":env!("CARGO_PKG_VERSION")}),
        json!({"name":"Windows x64","ok":cfg!(all(windows,target_arch="x86_64"))}),
        json!({"name":"한글 COM 등록","ok":hangul_registered(),"detail":"설치 등록 확인이며 실제 파일 열기·출력 성공을 뜻하지 않습니다."}),
        json!({"name":"설치 브라우저","ok":crate::browser_session::canvas_candidates().iter().any(|p|p.is_file()),"detail":"자체 미리보기에는 Chrome 또는 Edge가 필요합니다."}),
    ];
    let ready = checks.iter().all(|item| item["ok"] == true);
    Ok(
        json!({"status":if ready {"environment-ready"} else {"needs-attention"},"checks":checks,
        "workspace":workspace,"workspaceInitialized":workspace.join("autosw.config.json").is_file(),
        "note":"한글 실제 출력은 별도로 시험해야 합니다."}),
    )
}
