//! New-document approval plans and protected publication. Rendering is used only
//! for preview creation; validation/apply/verify reconstruct the approved bytes.
//! File previews select the original native or layout engine explicitly.
use crate::{
    atomic_file,
    hangul_preview::canonical_path,
    hwpx_compose::{Composed, compose_hwpx, document_from_markdown, normalize_document},
    hwpx_document::{load_hwpx, sha256},
    hwpx_form::{destination, digest, pretty, read_limited, required_string, validate_independent},
    hwpx_render::{preview_html, render_hwpx},
};
use serde_json::{Value, json};
use std::{collections::HashMap, ffi::OsString, fs, path::Path};

const SOURCE_LIMIT: u64 = 4 * 1024 * 1024;
const HWPX_LIMIT: u64 = 128 * 1024 * 1024;

struct Source {
    path: String,
    bytes: Vec<u8>,
}

fn source(file: &Path) -> Result<Source, String> {
    let path = canonical_path(file)?;
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > SOURCE_LIMIT {
        return Err("입력은 4MB 이하 파일이어야 합니다.".into());
    }
    // The bounded second read also protects against growth after stat.
    let bytes = read_limited(Path::new(&path), SOURCE_LIMIT)?;
    Ok(Source { path, bytes })
}

fn decode(bytes: &[u8]) -> Result<&str, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    // TextDecoder removes one initial UTF-8 BOM; the original replace removes
    // one more. Three BOMs leave the third for the JSON/Markdown parser.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text))
}

fn model_for(src: &Source) -> Result<Value, String> {
    let text = decode(&src.bytes)?;
    let extension = Path::new(&src.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("md") {
        return document_from_markdown(text);
    }
    if !extension.eq_ignore_ascii_case("json") {
        return Err("생성 입력은 .json 또는 .md".into());
    }
    normalize_document(&serde_json::from_str(text).map_err(|error| error.to_string())?)
}

fn absent(file: &str) -> Result<(), String> {
    if Path::new(file)
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        return Err(format!("기존 파일은 덮어쓰지 않습니다: {file}"));
    }
    Ok(())
}

fn plan_body(src: &Source, output: &str, generated: &Composed, preview: &Value) -> Value {
    json!({
        "schemaVersion":1,
        "sourcePath":src.path,
        "sourceSha256":sha256(&src.bytes),
        "outputPath":output,
        "model":generated.model,
        "expectedOutputSha256":sha256(&generated.bytes),
        "preview":preview
    })
}

/// Build an approval plan using a real installed-font Chromium preview.
pub fn preview_creation(input: &Path, output: &Path) -> Result<Value, String> {
    let src = source(input)?;
    let output = destination(output, "hwpx")?;
    absent(&output)?;
    let generated = compose_hwpx(&model_for(&src)?)?;
    let render = render_hwpx(&generated.bytes)?;
    let mut body = plan_body(&src, &output, &generated, &render["report"]);
    body["planSha256"] = json!(digest(&body));
    Ok(body)
}

/// Reconstruct approved output from the current source rather than trusting the
/// saved model or expected output hash on their own.
pub fn validate_creation(plan: &Value) -> Result<Composed, String> {
    let object = plan.as_object().ok_or("잘못된 생성 미리보기")?;
    let mut body = object.clone();
    body.shift_remove("planSha256");
    if plan["schemaVersion"].as_f64() != Some(1.0)
        || plan["planSha256"].as_str() != Some(digest(&Value::Object(body)).as_str())
    {
        return Err("미리보기 내용 변경".into());
    }
    let source_path = required_string(plan, "sourcePath")?;
    let src = source(Path::new(source_path))?;
    if src.path != source_path || plan["sourceSha256"].as_str() != Some(sha256(&src.bytes).as_str())
    {
        return Err("원고 변경 — 새 미리보기가 필요합니다.".into());
    }
    let output_path = required_string(plan, "outputPath")?;
    if destination(Path::new(output_path), "hwpx")? != output_path {
        return Err("출력 경로 변경".into());
    }
    let generated = compose_hwpx(&model_for(&src)?)?;
    if digest(&generated.model) != digest(&plan["model"])
        || plan["expectedOutputSha256"].as_str() != Some(sha256(&generated.bytes).as_str())
    {
        return Err("생성 규칙·원고·미리보기 불일치".into());
    }
    Ok(generated)
}

/// Publish only a fully validated, newly created output. Approval is the exact
/// saved plan hash; existing outputs are never overwritten.
pub fn apply_creation(plan: &Value, confirmation: &str) -> Result<Value, String> {
    if Some(confirmation) != plan.get("planSha256").and_then(Value::as_str) {
        return Err("확인한 미리보기 SHA-256이 필요합니다.".into());
    }
    let generated = validate_creation(plan)?;
    let output = required_string(plan, "outputPath")?;
    absent(output)?;
    validate_independent(&generated.bytes)?;
    let current = source(Path::new(required_string(plan, "sourcePath")?))?;
    if plan["sourceSha256"].as_str() != Some(sha256(&current.bytes).as_str()) {
        return Err("저장 전 원고 변경".into());
    }
    atomic_file::create_new(Path::new(output), &generated.bytes)
        .map_err(|error| error.to_string())?;
    verify_creation(plan)
}

pub fn verify_creation(plan: &Value) -> Result<Value, String> {
    validate_creation(plan)?;
    let output = required_string(plan, "outputPath")?;
    // HWPX readers already impose this input bound. Avoid allocating an
    // arbitrarily enlarged output merely to discover its hash has changed.
    let actual = read_limited(Path::new(output), 128 * 1024 * 1024)?;
    let hash = sha256(&actual);
    if plan["expectedOutputSha256"].as_str() != Some(hash.as_str()) {
        return Err(
            "생성본이 변경됐습니다. 한글 재저장본이면 별도 구조·화면 대조가 필요합니다.".into(),
        );
    }
    validate_independent(&actual)?;
    let document = load_hwpx(&actual)?;
    let sections = document.structure["sections"]
        .as_array()
        .ok_or("생성본 구획 정보가 없습니다.")?;
    let count = |key: &str| {
        sections
            .iter()
            .filter_map(|section| section[key].as_array())
            .map(Vec::len)
            .sum::<usize>()
    };
    Ok(json!({
        "status":"generated-awaiting-hangul",
        "outputPath":output,
        "sha256":hash,
        "independentCheck":"passed",
        "paragraphs":count("paragraphs"),
        "tables":count("tables")
    }))
}

fn read_plan(path: &Path) -> Result<Value, String> {
    serde_json::from_str(decode(&source(path)?.bytes)?).map_err(|error| error.to_string())
}

/// Render the actual file, recheck its hash, then publish HTML followed by the
/// optional report. If the second publication fails, the new HTML remains.
pub fn render_file(
    input: &Path,
    output: &Path,
    report: Option<&Path>,
    engine: &str,
) -> Result<Value, String> {
    render_file_with(input, output, report, engine, |bytes, engine| {
        if engine == "layout" {
            let rendered = crate::hwpx_layout::hwpx_loader::render_hwpx_layout(bytes)?;
            // Native layout numbers retain IEEE values internally. Cross the
            // original JSON.stringify boundary before pretty-printing so an
            // integer-valued float is written as 1 rather than 1.0.
            let report: Value = serde_json::from_str(&crate::hwpx_form::json_stringify(
                &rendered.report.to_json(),
            ))
            .map_err(|error| error.to_string())?;
            Ok(json!({
                "fontCss":rendered.font_css,
                "svgs":rendered.svgs,
                "report":report
            }))
        } else {
            render_hwpx(bytes)
        }
    })
}

// Inject only rendering for deterministic file-race tests; all path checks,
// reads, hash verification and atomic publication are the production path.
fn render_file_with(
    input: &Path,
    output: &Path,
    report_path: Option<&Path>,
    engine: &str,
    render: impl FnOnce(&[u8], &str) -> Result<Value, String>,
) -> Result<Value, String> {
    if !matches!(engine, "native" | "layout") {
        return Err("미리보기 엔진은 native 또는 layout".into());
    }
    let real = canonical_path(input)?;
    if fs::metadata(&real)
        .map_err(|error| error.to_string())?
        .len()
        > HWPX_LIMIT
    {
        return Err("입력 크기 초과".into());
    }
    let output = destination(output, "html")?;
    let report_path = report_path
        .map(|path| destination(path, "json"))
        .transpose()?;
    absent(&output)?;
    if let Some(report) = &report_path {
        absent(report)?;
    }
    let bytes = read_limited(Path::new(&real), HWPX_LIMIT)?;
    let rendered = render(&bytes, engine)?;
    let current = read_limited(Path::new(&real), HWPX_LIMIT)?;
    if rendered["report"]["sourceSha256"].as_str() != Some(sha256(&current).as_str()) {
        return Err("렌더 중 입력 파일 변경".into());
    }
    atomic_file::create_new(Path::new(&output), preview_html(&rendered)?.as_bytes())
        .map_err(|error| error.to_string())?;
    if let Some(report) = &report_path {
        atomic_file::create_new(Path::new(report), pretty(&rendered["report"])?.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let mut report = rendered["report"].clone();
    report["outputPath"] = output.into();
    if let Some(path) = report_path {
        report["reportPath"] = path.into();
    }
    Ok(report)
}

pub fn run(args: &[OsString]) -> Result<String, String> {
    let args = args
        .iter()
        .map(|value| value.to_str().ok_or("인자가 올바른 Unicode가 아닙니다."))
        .collect::<Result<Vec<_>, _>>()?;
    let command = args.first().copied().unwrap_or("--help");
    if command.is_empty() || command == "--help" {
        return Ok("preview --input 원고.json|md --output 새.hwpx --plan 새.json\napply --plan JSON --confirm SHA256\nverify --plan JSON\nrender --input HWPX --output 새.html [--report 새.json]\n".into());
    }
    let allowed: &[&str] = match command {
        "preview" => &["input", "output", "plan"],
        "apply" => &["plan", "confirm"],
        "verify" => &["plan"],
        "render" => &["input", "output", "report", "engine"],
        _ => return Err("알 수 없는 명령".into()),
    };
    let mut options = HashMap::new();
    for pair in args[1..].chunks(2) {
        let key = pair[0].strip_prefix("--").ok_or("잘못되거나 중복된 인자")?;
        let value = pair
            .get(1)
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .ok_or("잘못되거나 중복된 인자")?;
        if !allowed.contains(&key) || options.insert(key, *value).is_some() {
            return Err("잘못되거나 중복된 인자".into());
        }
    }
    if allowed
        .iter()
        .filter(|&&key| !(command == "render" && matches!(key, "report" | "engine")))
        .any(|key| !options.contains_key(key))
    {
        return Err("필수 인자 없음".into());
    }
    match command {
        "preview" => {
            let plan_path = destination(Path::new(options["plan"]), "json")?;
            absent(&plan_path)?;
            let plan = preview_creation(Path::new(options["input"]), Path::new(options["output"]))?;
            atomic_file::create_new(Path::new(&plan_path), pretty(&plan)?.as_bytes())
                .map_err(|error| error.to_string())?;
            pretty(&json!({
                "planPath":plan_path,
                "planSha256":plan["planSha256"],
                "model":plan["model"],
                "preview":plan["preview"]
            }))
        }
        "apply" => pretty(&apply_creation(
            &read_plan(Path::new(options["plan"]))?,
            options["confirm"],
        )?),
        "verify" => pretty(&verify_creation(&read_plan(Path::new(options["plan"]))?)?),
        _ => pretty(&render_file(
            Path::new(options["input"]),
            Path::new(options["output"]),
            options.get("report").map(Path::new),
            options.get("engine").copied().unwrap_or("native"),
        )?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_render(bytes: &[u8]) -> Value {
        json!({"report":{"schemaVersion":1,"sourceSha256":sha256(bytes),"status":"approximate-preview","pageCount":1,"warnings":[],"hangulVerified":false},"svgs":["<svg xmlns=\"http://www.w3.org/2000/svg\"><text>합성 &amp; 본문</text></svg>"]})
    }

    #[test]
    fn rendering_rechecks_input_before_any_publication() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("원본.hwpx");
        let output = dir.path().join("결과.html");
        let report = dir.path().join("결과.json");
        fs::write(&input, b"synthetic before").unwrap();
        let error = render_file_with(&input, &output, Some(&report), "native", |bytes, _| {
            let render = synthetic_render(bytes);
            fs::write(&input, b"synthetic changed").unwrap();
            Ok(render)
        })
        .unwrap_err();
        assert_eq!(error, "렌더 중 입력 파일 변경");
        assert_eq!(fs::read(&input).unwrap(), b"synthetic changed");
        assert!(!output.exists());
        assert!(!report.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn rendering_keeps_competing_files_and_completed_html() {
        for compete_report in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("원본.hwpx");
            let output = dir.path().join("결과.html");
            let report = dir.path().join("결과.json");
            fs::write(&input, b"synthetic").unwrap();
            let raced = if compete_report { &report } else { &output };
            let result =
                render_file_with(&input, &output, Some(&report), "layout", |bytes, engine| {
                    assert_eq!(engine, "layout");
                    fs::write(raced, b"concurrent file").unwrap();
                    Ok(synthetic_render(bytes))
                });
            assert!(result.is_err());
            assert_eq!(fs::read(raced).unwrap(), b"concurrent file");
            if compete_report {
                assert_eq!(
                    fs::read_to_string(&output).unwrap(),
                    preview_html(&synthetic_render(b"synthetic")).unwrap()
                );
                let before = fs::metadata(&output).unwrap().modified().unwrap();
                assert!(
                    render_file_with(&input, &output, Some(&report), "native", |_, _| panic!(
                        "existing result must stop before rendering"
                    ))
                    .is_err()
                );
                assert_eq!(fs::metadata(&output).unwrap().modified().unwrap(), before);
            } else {
                assert!(!report.exists());
            }
            assert_eq!(fs::read(&input).unwrap(), b"synthetic");
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                if compete_report { 3 } else { 2 }
            );
        }
    }

    #[test]
    fn rendering_publishes_exact_html_and_optional_report() {
        for write_report in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("source.hwpx");
            let output = dir.path().join("preview.html");
            let report = dir.path().join("report.json");
            fs::write(&input, b"synthetic").unwrap();
            let result = render_file_with(
                &input,
                &output,
                write_report.then_some(report.as_path()),
                "native",
                |bytes, _| Ok(synthetic_render(bytes)),
            )
            .unwrap();
            let render = synthetic_render(b"synthetic");
            assert_eq!(
                fs::read_to_string(&output).unwrap(),
                preview_html(&render).unwrap()
            );
            assert_eq!(result["sourceSha256"], render["report"]["sourceSha256"]);
            assert_eq!(result["outputPath"].as_str(), output.to_str());
            assert_eq!(result.get("reportPath").is_some(), write_report);
            if write_report {
                assert_eq!(
                    fs::read_to_string(&report).unwrap(),
                    pretty(&render["report"]).unwrap()
                );
            }
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                if write_report { 3 } else { 2 }
            );
        }
    }

    #[test]
    fn rendering_preflight_and_engine_failure_do_not_publish() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.hwpx");
        let output = dir.path().join("preview.html");
        let report = dir.path().join("report.json");
        fs::write(&input, b"synthetic").unwrap();
        assert_eq!(
            render_file_with(&input, &output, None, "other", |_, _| panic!(
                "invalid engine"
            ))
            .unwrap_err(),
            "미리보기 엔진은 native 또는 layout"
        );
        assert_eq!(
            render_file_with(&input, &output, None, "native", |_, _| Err(
                "render failure".into()
            ))
            .unwrap_err(),
            "render failure"
        );
        fs::write(&report, b"already present").unwrap();
        assert!(
            render_file_with(&input, &output, Some(&report), "native", |_, _| panic!(
                "existing report"
            ))
            .is_err()
        );
        assert!(!output.exists());
        assert_eq!(fs::read(&report).unwrap(), b"already present");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    fn manuscript() -> Value {
        json!({"schemaVersion":1,"title":"가상 원고","blocks":[
            {"type":"heading","text":"합성 제목"},{"type":"paragraph","text":"검증 & <내용> 😀"}
        ]})
    }
    fn synthetic_plan(directory: &Path) -> (Value, Vec<u8>) {
        let input = directory.join("가상 # 원고.json");
        let bytes = serde_json::to_vec(&manuscript()).unwrap();
        fs::write(&input, &bytes).unwrap();
        let src = source(&input).unwrap();
        let generated = compose_hwpx(&model_for(&src).unwrap()).unwrap();
        let output = destination(&directory.join("가상 결과.hwpx"), "hwpx").unwrap();
        // The browser is deliberately absent from general cargo unit tests.
        let mut plan = plan_body(
            &src,
            &output,
            &generated,
            &json!({"status":"synthetic-unit-preview"}),
        );
        reseal(&mut plan);
        (plan, bytes)
    }
    fn reseal(plan: &mut Value) {
        plan.as_object_mut().unwrap().shift_remove("planSha256");
        plan["planSha256"] = json!(digest(plan));
    }

    #[test]
    fn source_decoding_removes_exactly_two_boms_and_rejects_invalid_utf8() {
        assert_eq!(decode(b"text").unwrap(), "text");
        assert_eq!(decode("\u{feff}text".as_bytes()).unwrap(), "text");
        assert_eq!(decode("\u{feff}\u{feff}text".as_bytes()).unwrap(), "text");
        assert_eq!(
            decode("\u{feff}\u{feff}\u{feff}text".as_bytes()).unwrap(),
            "\u{feff}text"
        );
        assert!(decode(&[0xff]).is_err());
        let json = serde_json::to_string(&manuscript()).unwrap();
        for count in 0..=2 {
            let src = Source {
                path: "synthetic.JSON".into(),
                bytes: format!("{}{json}", "\u{feff}".repeat(count)).into_bytes(),
            };
            assert_eq!(model_for(&src).unwrap()["title"], "가상 원고");
        }
        let src = Source {
            path: "synthetic.json".into(),
            bytes: format!("\u{feff}\u{feff}\u{feff}{json}").into_bytes(),
        };
        assert!(model_for(&src).is_err());
        let md = Source {
            path: "synthetic.MD".into(),
            bytes: b"# Heading\n\nBody".to_vec(),
        };
        assert_eq!(model_for(&md).unwrap()["title"], "Heading");
        let other = Source {
            path: "synthetic.txt".into(),
            bytes: b"# Heading".to_vec(),
        };
        assert_eq!(model_for(&other).unwrap_err(), "생성 입력은 .json 또는 .md");
    }

    #[test]
    fn approval_and_regeneration_validation_are_read_only() {
        let directory = tempfile::tempdir().unwrap();
        let (plan, original) = synthetic_plan(directory.path());
        assert!(validate_creation(&plan).is_ok());
        assert_eq!(
            fs::read(plan["sourcePath"].as_str().unwrap()).unwrap(),
            original
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(
            apply_creation(&plan, "unapproved").unwrap_err(),
            "확인한 미리보기 SHA-256이 필요합니다."
        );
        let mut changed = plan.clone();
        changed["preview"]["status"] = json!("changed");
        assert_eq!(
            validate_creation(&changed).unwrap_err(),
            "미리보기 내용 변경"
        );
        reseal(&mut changed);
        assert!(validate_creation(&changed).is_ok()); // No new preview schema restriction.
        changed["model"]["title"] = json!("다른 원고");
        reseal(&mut changed);
        assert_eq!(
            validate_creation(&changed).unwrap_err(),
            "생성 규칙·원고·미리보기 불일치"
        );
        changed = plan.clone();
        changed["expectedOutputSha256"] = json!("different");
        reseal(&mut changed);
        assert!(validate_creation(&changed).is_err());
        changed = plan.clone();
        changed["schemaVersion"] = json!(true);
        reseal(&mut changed);
        assert_eq!(
            validate_creation(&changed).unwrap_err(),
            "미리보기 내용 변경"
        );
        fs::write(plan["sourcePath"].as_str().unwrap(), b"changed").unwrap();
        assert_eq!(
            validate_creation(&plan).unwrap_err(),
            "원고 변경 — 새 미리보기가 필요합니다."
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn apply_verifies_structure_and_preserves_existing_or_modified_output() {
        let directory = tempfile::tempdir().unwrap();
        let (plan, original) = synthetic_plan(directory.path());
        let hash = plan["planSha256"].as_str().unwrap();
        let report = apply_creation(&plan, hash).unwrap();
        assert_eq!(report["status"], "generated-awaiting-hangul");
        assert_eq!(report["independentCheck"], "passed");
        assert_eq!(report["paragraphs"], 2);
        assert_eq!(report["tables"], 0);
        assert_eq!(report["sha256"], plan["expectedOutputSha256"]);
        assert_eq!(verify_creation(&plan).unwrap(), report);
        let output = Path::new(plan["outputPath"].as_str().unwrap());
        let generated = fs::read(output).unwrap();
        assert!(
            apply_creation(&plan, hash)
                .unwrap_err()
                .starts_with("기존 파일은 덮어쓰지 않습니다: ")
        );
        assert_eq!(fs::read(output).unwrap(), generated);
        fs::write(output, b"modified").unwrap();
        assert_eq!(
            verify_creation(&plan).unwrap_err(),
            "생성본이 변경됐습니다. 한글 재저장본이면 별도 구조·화면 대조가 필요합니다."
        );
        assert_eq!(fs::read(output).unwrap(), b"modified");
        assert_eq!(
            fs::read(plan["sourcePath"].as_str().unwrap()).unwrap(),
            original
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn source_limits_and_cli_rejections_do_not_launch_a_browser() {
        let directory = tempfile::tempdir().unwrap();
        assert!(source(directory.path()).is_err());
        let oversized = directory.path().join("large.json");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(SOURCE_LIMIT + 1).unwrap();
        drop(file);
        assert_eq!(
            source(&oversized).err().unwrap(),
            "입력은 4MB 이하 파일이어야 합니다."
        );
        for args in [
            vec!["verify"],
            vec!["verify", "--plan"],
            vec!["verify", "--plan", "x", "--plan", "y"],
            vec!["verify", "--input", "x"],
            vec!["verify", "--plan", "--unknown"],
            vec!["render", "--input", "x"],
        ] {
            let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
            assert!(run(&args).is_err());
        }
        assert!(
            run(&[])
                .unwrap()
                .contains("apply --plan JSON --confirm SHA256")
        );
    }
}
