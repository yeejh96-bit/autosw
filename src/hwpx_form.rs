//! 기존 양식 작성의 구조 조회 → 미리보기 → 승인 해시 확인 → 새 파일 저장.
use crate::{
    atomic_file,
    hangul_preview::canonical_path,
    hwpx_document::{inspect_hwpx, load_hwpx, sha256},
    hwpx_edit::{Prepared, prepare_hwpx, verify_hwpx},
    hwpx_structure,
};
use serde_json::{Value, json};
use std::{collections::HashMap, ffi::OsString, fs, io::Read, path::Path};

// 승인 해시는 기존 JSON.stringify와 동일한 숫자·속성 순서로 계산한다.
pub fn json_stringify(value: &Value) -> String {
    match value {
        Value::Number(_) => crate::input::js_string(value),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(json_stringify)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut numeric = vec![];
            let mut ordinary = vec![];
            for key in values.keys() {
                if let Ok(index) = key.parse::<u32>()
                    && index != u32::MAX
                    && index.to_string() == *key
                {
                    numeric.push((index, key));
                } else {
                    ordinary.push(key);
                }
            }
            numeric.sort_by_key(|(index, _)| *index);
            let keys = numeric.iter().map(|(_, key)| *key).chain(ordinary);
            format!(
                "{{{}}}",
                keys.map(|key| format!(
                    "{}:{}",
                    serde_json::to_string(key).unwrap(),
                    json_stringify(&values[key])
                ))
                .collect::<Vec<_>>()
                .join(",")
            )
        }
        _ => serde_json::to_string(value).unwrap(),
    }
}
pub(crate) fn digest(value: &Value) -> String {
    sha256(json_stringify(value).as_bytes())
}
pub(crate) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("지원하지 않는 파일 크기·종류입니다.".into());
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("파일 크기 한도를 넘었습니다.".into());
    }
    Ok(bytes)
}
pub(crate) fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = read_limited(path, 16 * 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|e| e.to_string())
}
fn read_source(path: &Path) -> Result<(String, Vec<u8>), String> {
    let canonical = canonical_path(path)?;
    let bytes = read_limited(Path::new(&canonical), 128 * 1024 * 1024)?;
    Ok((canonical, bytes))
}
pub(crate) fn absolute(path: &Path) -> Result<String, String> {
    std::path::absolute(path)
        .map_err(|e| e.to_string())?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "파일 경로가 올바른 Unicode가 아닙니다.".into())
}
pub(crate) fn destination(path: &Path, extension: &str) -> Result<String, String> {
    let out = absolute(path)?;
    let path = Path::new(&out);
    if !path
        .extension()
        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(extension))
    {
        return Err(format!("새 산출물은 .{extension}로 지정하세요."));
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err("출력 폴더가 없습니다.".into());
    }
    Ok(out)
}
pub(crate) fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("잘못된 미리보기 항목: {key}"))
}
pub fn validate_independent(bytes: &[u8]) -> Result<(), String> {
    let independent = hwpx_structure::read_structure(bytes)?;
    if independent != load_hwpx(bytes)?.structure {
        return Err("작성 결과의 독립 구조가 다릅니다.".into());
    }
    Ok(())
}
fn plan_body(source: &str, output: &str, request: &Value, prepared: &Prepared) -> Value {
    json!({"schemaVersion":1,"sourcePath":source,"outputPath":output,"request":request,"preview":prepared.preview,"unassigned":prepared.unassigned,"expectedOutputSha256":prepared.report["outputSha256"],"visualStatus":"not-checked"})
}
pub fn create_plan(input: &Path, request: &Value, output: &Path) -> Result<Value, String> {
    let (source, bytes) = read_source(input)?;
    let output = destination(output, "hwpx")?;
    if Path::new(&output).try_exists().map_err(|e| e.to_string())? {
        return Err("출력 파일이 이미 있습니다. 새 이름을 지정하세요.".into());
    }
    let prepared = prepare_hwpx(&bytes, request)?;
    let mut body = plan_body(&source, &output, request, &prepared);
    body["planSha256"] = json!(digest(&body));
    Ok(body)
}
pub fn validate_plan(plan: &Value) -> Result<(Prepared, Vec<u8>), String> {
    let Some(object) = plan.as_object() else {
        return Err("잘못된 미리보기입니다.".into());
    };
    let hash = required_string(plan, "planSha256")?;
    let mut body = object.clone();
    body.shift_remove("planSha256");
    if digest(&Value::Object(body)) != hash {
        return Err("미리보기 내용이 변경됐습니다.".into());
    }
    let source_path = required_string(plan, "sourcePath")?;
    let output_path = required_string(plan, "outputPath")?;
    let (source, bytes) = read_source(Path::new(source_path))?;
    if source != source_path || absolute(Path::new(output_path))? != output_path {
        return Err("미리보기 경로가 변경됐습니다.".into());
    }
    let request = plan.get("request").ok_or("입력이 없습니다.")?;
    let prepared = prepare_hwpx(&bytes, request)?;
    let expected = plan_body(
        &source,
        &destination(Path::new(output_path), "hwpx")?,
        request,
        &prepared,
    );
    if digest(&expected) != hash {
        return Err(
            "미리보기와 현재 원본·입력·출력 경로가 다릅니다. 미리보기를 다시 확인하세요.".into(),
        );
    }
    Ok((prepared, bytes))
}
fn final_report(mut report: Value, output: &str) -> Value {
    report["independentCheck"] = json!("passed");
    report["outputPath"] = json!(output);
    report["status"] = json!("generated-awaiting-visual");
    report
}
pub fn apply_plan(plan: &Value, confirmation: &str) -> Result<Value, String> {
    if Some(confirmation) != plan.get("planSha256").and_then(Value::as_str) {
        return Err("확인한 미리보기의 SHA-256을 지정하세요.".into());
    }
    let (prepared, source_bytes) = validate_plan(plan)?;
    let output = required_string(plan, "outputPath")?;
    if Path::new(output).try_exists().map_err(|e| e.to_string())? {
        return Err("기존 산출물을 덮어쓰지 않습니다. verify로 기존 파일을 확인하세요.".into());
    }
    validate_independent(&prepared.output)?;
    let (_, current) = read_source(Path::new(required_string(plan, "sourcePath")?))?;
    if Some(sha256(&current).as_str())
        != plan["request"].get("sourceSha256").and_then(Value::as_str)
    {
        return Err("원본이 미리보기 이후 변경됐습니다.".into());
    }
    atomic_file::create_new(Path::new(output), &prepared.output).map_err(|e| e.to_string())?;
    let report = verify_hwpx(
        &source_bytes,
        &plan["request"],
        &read_limited(Path::new(output), 128 * 1024 * 1024)?,
    )?;
    Ok(final_report(report, output))
}
pub fn verify_plan(plan: &Value) -> Result<Value, String> {
    let (_, bytes) = validate_plan(plan)?;
    let output = required_string(plan, "outputPath")?;
    let actual = read_limited(Path::new(output), 128 * 1024 * 1024)?;
    let report = verify_hwpx(&bytes, &plan["request"], &actual)?;
    validate_independent(&actual)?;
    Ok(final_report(report, output))
}
pub(crate) fn pretty(value: &Value) -> Result<String, String> {
    fn normalize_zero(value: &mut Value) {
        match value {
            Value::Number(n) if n.as_f64() == Some(0.0) => *value = json!(0),
            Value::Array(values) => values.iter_mut().for_each(normalize_zero),
            Value::Object(values) => values.values_mut().for_each(normalize_zero),
            _ => {}
        }
    }
    let mut output = value.clone();
    normalize_zero(&mut output);
    Ok(serde_json::to_string_pretty(&output).map_err(|e| e.to_string())? + "\n")
}
pub fn run(args: &[OsString]) -> Result<String, String> {
    let args = args
        .iter()
        .map(|v| v.to_str().ok_or("인자가 올바른 Unicode가 아닙니다."))
        .collect::<Result<Vec<_>, _>>()?;
    let command = args.first().copied().unwrap_or("--help");
    if command == "--help" {
        return Ok("inspect --input HWPX [--output JSON]\npreview --input HWPX --request JSON --output 새HWPX --plan 새JSON\napply --plan JSON --confirm 미리보기SHA256\nverify --plan JSON\n".into());
    }
    let allowed: &[&str] = match command {
        "inspect" => &["input", "output"],
        "preview" => &["input", "request", "output", "plan"],
        "apply" => &["plan", "confirm"],
        "verify" => &["plan"],
        _ => return Err("알 수 없는 명령입니다. --help를 확인하세요.".into()),
    };
    let mut options = HashMap::new();
    for pair in args[1..].chunks(2) {
        let key = pair[0].strip_prefix("--").ok_or("잘못된 인자입니다.")?;
        let value = pair
            .get(1)
            .filter(|v| !v.is_empty() && !v.starts_with("--"))
            .ok_or("잘못된 인자입니다.")?;
        if !allowed.contains(&key) || options.insert(key, *value).is_some() {
            return Err("잘못되거나 중복된 인자입니다.".into());
        }
    }
    let required = if command == "inspect" {
        &["input"][..]
    } else {
        allowed
    };
    if required.iter().any(|key| !options.contains_key(key)) {
        return Err("필수 인자가 없습니다. --help를 확인하세요.".into());
    }
    match command {
        "inspect" => {
            let (_, bytes) = read_source(Path::new(options["input"]))?;
            let text = pretty(&inspect_hwpx(&bytes)?)?;
            if let Some(output) = options.get("output") {
                let output = absolute(Path::new(output))?;
                atomic_file::create_new(Path::new(&output), text.as_bytes())
                    .map_err(|e| e.to_string())?;
                Ok(format!("구조 저장: {output}\n"))
            } else {
                Ok(text + "\n")
            }
        }
        "preview" => {
            let plan = create_plan(
                Path::new(options["input"]),
                &read_json(Path::new(options["request"]))?,
                Path::new(options["output"]),
            )?;
            let plan_path = absolute(Path::new(options["plan"]))?;
            if plan_path.to_lowercase() == required_string(&plan, "outputPath")?.to_lowercase() {
                return Err("미리보기와 산출물 경로를 분리하세요.".into());
            }
            atomic_file::create_new(Path::new(&plan_path), pretty(&plan)?.as_bytes())
                .map_err(|e| e.to_string())?;
            let mut result = json!({"planPath": plan_path});
            result
                .as_object_mut()
                .unwrap()
                .extend(plan.as_object().unwrap().clone());
            pretty(&result)
        }
        "apply" => pretty(&apply_plan(
            &read_json(Path::new(options["plan"]))?,
            options["confirm"],
        )?),
        _ => pretty(&verify_plan(&read_json(Path::new(options["plan"]))?)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_digest_uses_javascript_numeric_keys_and_number_spelling() {
        let value: Value = serde_json::from_str(
            r#"{"b":-0,"10":1.0,"2":1e21,"01":1e-7,"a":["😀",1.0000000000000002]}"#,
        )
        .unwrap();
        assert_eq!(
            json_stringify(&value),
            r#"{"2":1e+21,"10":1,"b":0,"01":1e-7,"a":["😀",1.0000000000000002]}"#
        );
    }
    #[test]
    fn json_output_normalizes_negative_zero_without_changing_input() {
        let input: Value =
            serde_json::from_str(r#"{"pageCount":-0,"visualStatus":[-0.0,{"n":-0}]}"#).unwrap();
        let text = pretty(&input).unwrap();
        assert!(!text.contains("-0"));
        assert!(input["pageCount"].as_f64().unwrap().is_sign_negative());
    }
}
