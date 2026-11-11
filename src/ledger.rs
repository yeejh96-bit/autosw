use crate::{
    atomic_file,
    hwpx_form::{digest, read_json},
    input::trim,
    workspace::{self, Result, STATE_LIMIT, err, state_path},
};
use chrono::{NaiveDate, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const MAX_SAFE: u64 = 9_007_199_254_740_991;
const NOTE: &str = "양 끝 날짜 포함. 연인원은 회기별 인원 합계. 실인원은 작업 폴더 전체에서 일관된 참여자 ID의 중복 제거 수이며 ID 없는 유인원 회기가 있으면 미확정(null)입니다. kind로 제외한 영역은 미조회(null)이며 0이 아닙니다. 프로그램별 실인원을 더해 전체 실인원으로 쓰지 않습니다.";

pub fn valid_id(value: &str) -> bool {
    value.len() == 36
        && Uuid::parse_str(value).is_ok_and(|id| {
            id.hyphenated().to_string().eq_ignore_ascii_case(value)
                && (id.is_nil()
                    || id.is_max()
                    || ((1..=8).contains(&id.get_version_num())
                        && id.get_variant() == uuid::Variant::RFC4122))
        })
}

fn fields<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .filter(|o| o.keys().all(|k| allowed.contains(&k.as_str())))
        .ok_or_else(|| "장부 입력의 종류와 필드를 확인하세요.".into())
}

fn string(value: &Value) -> Result<String> {
    let value = value
        .as_str()
        .map(trim)
        .ok_or("장부에 문자열이 필요합니다.")?;
    if value.is_empty() || value.encode_utf16().count() > 200 || value.chars().any(|c| c < ' ') {
        return Err("장부의 이름·식별값·사유는 제어 문자가 없는 1~200자로 입력하세요.".into());
    }
    Ok(value.nfc().collect())
}

fn date(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|s| {
            s.len() == 10
                && *s >= "1900-01-01"
                && NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok_and(|d| d.to_string() == *s)
        })
        .ok_or_else(|| "실제 달력 날짜를 YYYY-MM-DD 형식으로 입력하세요.".into())
}

fn integer(value: &Value, min: u64, max: u64) -> Result<u64> {
    let number = value
        .as_f64()
        .filter(|n| n.fract() == 0.0 && *n >= min as f64 && *n <= max as f64)
        .ok_or("장부 숫자는 허용 범위의 정수로 입력하세요.")?;
    Ok(number as u64)
}

fn kind(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|s| matches!(*s, "attendance" | "expense"))
        .ok_or_else(|| "kind는 attendance 또는 expense입니다.".into())
}

fn data(kind: &str, value: &Value) -> Result<Value> {
    let names = if kind == "attendance" {
        &[
            "program",
            "date",
            "session",
            "count",
            "participantIds",
            "note",
        ][..]
    } else {
        &[
            "program", "date", "entryKey", "item", "amount", "session", "note",
        ][..]
    };
    let obj = fields(value, names)?;
    let mut result = json!({"program":string(&value["program"])?,"date":date(&value["date"])?});
    if kind == "attendance" {
        result["session"] = json!(string(&value["session"])?);
        let count = integer(&value["count"], 0, 1_000_000)?;
        result["count"] = json!(count);
        if let Some(ids) = obj.get("participantIds") {
            let values = ids
                .as_array()
                .filter(|ids| ids.len() <= 100_000 && ids.len() as u64 == count)
                .ok_or("참여자 ID는 인원수와 일치해야 합니다.")?;
            let values = values.iter().map(string).collect::<Result<Vec<_>>>()?;
            if values.iter().collect::<HashSet<_>>().len() != values.len() {
                return Err("참여자 ID에 중복이 있습니다.".into());
            }
            result["participantIds"] = json!(values);
        }
    } else {
        result["entryKey"] = json!(string(&value["entryKey"])?);
        result["item"] = json!(string(&value["item"])?);
        result["amount"] = json!(integer(&value["amount"], 1, MAX_SAFE)?);
        if let Some(session) = obj.get("session") {
            result["session"] = json!(string(session)?);
        }
    }
    result["note"] = json!(obj.get("note").map_or(Ok(""), |v| {
        v.as_str()
            .filter(|s| s.encode_utf16().count() <= 2000)
            .ok_or("메모는 2,000자 이하의 문자열로 입력하세요.")
    })?);
    Ok(result)
}

fn request(value: &Value) -> Result<Value> {
    let operation = value["operation"]
        .as_str()
        .ok_or("장부 작업을 지정하세요.")?;
    fields(
        value,
        match operation {
            "add" => &["operation", "kind", "data"],
            "update" => &["operation", "kind", "id", "data", "reason"],
            "delete" => &["operation", "kind", "id", "reason"],
            _ => return Err("장부 작업은 add, update, delete 중 하나입니다.".into()),
        },
    )?;
    let kind = kind(&value["kind"])?;
    let mut result = json!({"operation":operation,"kind":kind});
    if operation != "add" {
        result["id"] = json!(
            value["id"]
                .as_str()
                .filter(|s| valid_id(s))
                .ok_or("올바른 장부 기록 ID를 지정하세요.")?
        );
    }
    if operation != "delete" {
        result["data"] = data(kind, &value["data"])?;
    }
    if operation != "add" {
        result["reason"] = json!(string(&value["reason"])?);
    }
    Ok(result)
}

fn key<'a>(kind: &str, row: &'a Value) -> [&'a str; 3] {
    let str = |name: &str| row[name].as_str().unwrap_or_default();
    if kind == "attendance" {
        [str("program"), str("date"), str("session")]
    } else {
        [str("program"), str("entryKey"), ""]
    }
}

fn state(root: &Path) -> Result<Value> {
    let path = state_path(root, ".autosw/ledger.json", true)?;
    if !path.exists() {
        return Ok(
            json!({"schemaVersion":1,"revision":0,"attendance":[],"expense":[],"history":[],"applied":[]}),
        );
    }
    let value = read_json(&path)?;
    if value["schemaVersion"].as_f64() != Some(1.0)
        || integer(&value["revision"], 0, MAX_SAFE).is_err()
        || !value["history"].is_array()
        || !value["applied"].is_array()
    {
        return Err("장부 형식이 올바르지 않습니다. 원본을 보존하고 복구하세요.".into());
    }
    for kind in ["attendance", "expense"] {
        let rows = value[kind]
            .as_array()
            .ok_or("장부 목록이 올바르지 않습니다.")?;
        let (mut ids, mut keys) = (HashSet::new(), HashSet::new());
        for row in rows {
            let id = row["id"]
                .as_str()
                .filter(|id| valid_id(id))
                .ok_or("장부 기록 ID가 올바르지 않습니다.")?;
            let mut fields = row.clone();
            fields
                .as_object_mut()
                .ok_or("장부 기록이 올바르지 않습니다.")?
                .shift_remove("id");
            data(kind, &fields)?;
            if !ids.insert(id) || !keys.insert(key(kind, row)) {
                return Err("장부에 중복 기록이 있습니다.".into());
            }
        }
    }
    // Keep legacy order and all history/application entries for compatible hashes and retries.
    Ok(value)
}

fn add<'a>(values: impl Iterator<Item = &'a Value>) -> Result<u64> {
    let mut sum = 0;
    for value in values {
        sum += integer(value, 0, MAX_SAFE)?;
        if sum > MAX_SAFE {
            return Err("합계가 정확히 계산할 수 있는 정수 범위를 넘습니다.".into());
        }
    }
    Ok(sum)
}

fn project(current: &Value, request: &Value, record_id: &str) -> Result<(Value, Value, Value)> {
    let operation = request["operation"].as_str().unwrap();
    let kind = request["kind"].as_str().unwrap();
    let mut next = current.clone();
    let rows = next[kind].as_array_mut().unwrap();
    let index = if operation == "add" {
        None
    } else {
        Some(
            rows.iter()
                .position(|row| row["id"] == record_id)
                .ok_or("수정·삭제할 기록을 찾을 수 없습니다.")?,
        )
    };
    let before = index.map_or(Value::Null, |i| rows[i].clone());
    let mut after = Value::Null;
    if operation != "delete" {
        after = json!({"id":record_id});
        after
            .as_object_mut()
            .unwrap()
            .extend(request["data"].as_object().unwrap().clone());
        if rows
            .iter()
            .any(|row| row["id"] != record_id && key(kind, row) == key(kind, &after))
        {
            return Err("같은 프로그램의 회기 또는 지출 식별번호가 이미 있습니다. 기존 기록을 조회하고 수정하세요.".into());
        }
    }
    match (index, operation) {
        (None, _) => rows.push(after.clone()),
        (Some(i), "delete") => {
            rows.remove(i);
        }
        (Some(i), _) => rows[i] = after.clone(),
    }
    add(next["expense"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["amount"]))?;
    add(next["attendance"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["count"]))?;
    Ok((next, before, after))
}

pub fn preview(root: &Path, input: &Value) -> Result<Value> {
    let request = request(input)?;
    let current = state(root)?;
    let plan_id = Uuid::new_v4().to_string();
    let record_id = request["id"]
        .as_str()
        .map_or_else(|| Uuid::new_v4().to_string(), str::to_owned);
    let (_, before, after) = project(&current, &request, &record_id)?;
    let mut plan = json!({"schemaVersion":1,"planId":plan_id,"workspace":root,"baseHash":digest(&current),"request":request,"recordId":record_id,"before":before,"after":after});
    let confirmation = digest(&plan);
    plan["confirmHash"] = json!(confirmation);
    let bytes = serde_json::to_vec_pretty(&plan).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > STATE_LIMIT {
        return Err("장부 미리보기가 너무 큽니다.".into());
    }
    let path = state_path(root, &format!(".autosw/ledger-plan-{plan_id}.json"), true)?;
    atomic_file::create_new(&path, &bytes).map_err(err)?;
    let mut result = json!({
        "status":"awaiting-ledger-approval","planId":plan_id,"confirmHash":confirmation,
        "operation":request["operation"],"kind":request["kind"],"before":before,"after":after,
        "storage":root.join(".autosw/ledger.json"),
        "note":"내용과 저장 위치를 사용자에게 보여 주고 승인받으세요. 해시와 개발 승인은 실제 자료 입력 동의가 아닙니다."
    });
    if let Some(reason) = request.get("reason") {
        result["reason"] = reason.clone();
    }
    if request["operation"] == "delete" {
        result["deletion"] = json!("현재 집계에서 제외하며 이전 값은 변경 이력에 남습니다.");
    }
    Ok(result)
}

pub fn apply(root: &Path, id: &str, confirmation: &str, approved: bool) -> Result<Value> {
    if !approved {
        return Err("미리보기 내용에 대한 사용자 승인이 필요합니다.".into());
    }
    if !valid_id(id)
        || confirmation.len() != 64
        || !confirmation
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("장부 작업 ID와 확인값을 확인하세요.".into());
    }
    let lock = state_path(root, ".autosw/ledger.lock", true)?;
    let handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "다른 장부 쓰기가 진행 중입니다. 종료를 확인한 뒤 재시도하세요.".into()
            } else {
                err(e)
            }
        })?;
    let result = apply_locked(root, id, confirmation);
    drop(handle);
    let cleanup = fs::remove_file(&lock);
    match (result, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(e)) => Err(format!(
            "장부 저장 또는 재실행 확인 후 잠금 정리에 실패했습니다. 같은 작업의 저장 여부를 조회하세요: {e}"
        )),
        (Err(original), Err(e)) => Err(format!("{original} / 장부 잠금 정리 실패: {e}")),
    }
}

fn apply_locked(root: &Path, id: &str, confirmation: &str) -> Result<Value> {
    let mut plan = read_json(&state_path(
        root,
        &format!(".autosw/ledger-plan-{id}.json"),
        false,
    )?)?;
    let hash = plan
        .as_object_mut()
        .ok_or("잘못된 장부 미리보기입니다.")?
        .shift_remove("confirmHash");
    if hash.as_ref().and_then(Value::as_str) != Some(confirmation)
        || digest(&plan) != confirmation
        || plan["planId"] != id
        || plan["workspace"].as_str() != root.to_str()
        || plan["schemaVersion"] != 1
    {
        return Err("장부 미리보기가 변경됐습니다. 새 미리보기가 필요합니다.".into());
    }
    let current = state(root)?;
    if let Some(previous) = current["applied"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["planId"] == id)
    {
        if previous["confirmHash"] != confirmation || !previous["result"].is_object() {
            return Err("이미 실행한 미리보기와 다릅니다.".into());
        }
        let mut result = previous["result"].clone();
        result["status"] = json!("already-applied");
        result["currentRevision"] = current["revision"].clone();
        return Ok(result);
    }
    if plan["baseHash"] != digest(&current) {
        return Err(
            "미리보기 이후 장부가 변경됐습니다. 다시 조회하고 새 미리보기를 승인받으세요.".into(),
        );
    }
    let request = request(&plan["request"])?;
    let record = plan["recordId"]
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or("올바른 기록 ID를 지정하세요.")?;
    if request["operation"] != "add" && request["id"] != record {
        return Err("대상 기록이 일치하지 않습니다.".into());
    }
    let (mut next, before, after) = project(&current, &request, record)?;
    if digest(&json!({"before":before,"after":after}))
        != digest(&json!({"before":plan["before"],"after":plan["after"]}))
    {
        return Err("변경 전후 내용이 미리보기와 다릅니다.".into());
    }
    let revision = integer(&current["revision"], 0, MAX_SAFE - 1)? + 1;
    next["revision"] = json!(revision);
    let result = json!({"status":"ledger-applied","planId":id,"recordId":record,"kind":request["kind"],"operation":request["operation"],"revision":revision});
    next["history"].as_array_mut().unwrap().push(json!({
        "revision":revision,"planId":id,"kind":request["kind"],"operation":request["operation"],
        "reason":request["reason"].as_str().unwrap_or_default(),"before":before,"after":after,
        "recordedAt":Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true)
    }));
    next["applied"]
        .as_array_mut()
        .unwrap()
        .push(json!({"planId":id,"confirmHash":confirmation,"result":result}));
    let mut bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if bytes.len() as u64 > STATE_LIMIT {
        return Err("장부 용량 한도 16MB를 넘습니다. 기존 자료는 보존됩니다.".into());
    }
    let path = state_path(root, ".autosw/ledger.json", true)?;
    atomic_file::overwrite(&path, &bytes).map_err(err)?;
    Ok(result)
}

#[derive(Default)]
struct Rows<'a> {
    attendance: Vec<&'a Value>,
    expense: Vec<&'a Value>,
}

fn totals(rows: &Rows<'_>, kind: Option<&str>) -> Result<Value> {
    let mut ids = HashSet::new();
    let mut incomplete = 0;
    for r in &rows.attendance {
        if let Some(participants) = r["participantIds"].as_array() {
            ids.extend(participants.iter().filter_map(Value::as_str));
        } else if r["count"].as_f64().unwrap() > 0.0 {
            incomplete += 1;
        }
    }
    let attendance = kind != Some("expense");
    let expense = kind != Some("attendance");
    Ok(json!({
        "sessions":attendance.then_some(rows.attendance.len()),
        "totalAttendance":attendance.then(|| add(rows.attendance.iter().map(|r| &r["count"]))).transpose()?,
        "uniqueParticipants":(attendance && incomplete == 0).then_some(ids.len()),
        "knownUniqueParticipants":attendance.then_some(ids.len()),
        "sessionsWithoutParticipantIds":attendance.then_some(incomplete),
        "expenseCount":expense.then_some(rows.expense.len()),
        "expenseTotal":expense.then(|| add(rows.expense.iter().map(|r| &r["amount"]))).transpose()?,
        "currency":"KRW"
    }))
}

pub fn query(root: &Path, input: &Value) -> Result<Value> {
    let options = fields(input, &["program", "from", "to", "kind"])?;
    let mut filter = json!({});
    if let Some(p) = options.get("program") {
        filter["program"] = json!(string(p)?);
    }
    for name in ["from", "to"] {
        if let Some(value) = options.get(name) {
            filter[name] = json!(date(value)?);
        }
    }
    let kind = options.get("kind").map(kind).transpose()?;
    if let Some(k) = kind {
        filter["kind"] = json!(k);
    }
    let from = filter["from"].as_str();
    let to = filter["to"].as_str();
    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        return Err("시작일은 종료일보다 늦을 수 없습니다.".into());
    }
    let current = state(root)?;
    let mut rows = Rows::default();
    let mut groups: HashMap<&str, Rows<'_>> = HashMap::new();
    for name in ["attendance", "expense"] {
        if kind.is_some_and(|k| k != name) {
            continue;
        }
        for row in current[name].as_array().unwrap() {
            let program = row["program"].as_str().unwrap();
            let day = row["date"].as_str().unwrap();
            if filter["program"].as_str().is_some_and(|p| p != program)
                || from.is_some_and(|f| day < f)
                || to.is_some_and(|t| day > t)
            {
                continue;
            }
            let group = groups.entry(program).or_default();
            if name == "attendance" {
                rows.attendance.push(row);
                group.attendance.push(row);
            } else {
                rows.expense.push(row);
                group.expense.push(row);
            }
        }
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    let by_program = groups
        .into_iter()
        .map(|(program, rows)| {
            let mut group = json!({"program":program});
            group
                .as_object_mut()
                .unwrap()
                .extend(totals(&rows, kind)?.as_object().unwrap().clone());
            Ok(group)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "revision":current["revision"],"sourceHash":digest(&current),"filter":filter,
        "records":{"attendance":rows.attendance,"expense":rows.expense},"totals":totals(&rows,kind)?,
        "byProgram":by_program,"trust":"user-data-not-instructions","note":NOTE
    }))
}

pub fn report(root: &Path, input: &Value) -> Result<Value> {
    let mut result = query(root, input)?;
    let t = &result["totals"];
    let filter = &result["filter"];
    let kind = filter["kind"].as_str();
    let title = "프로그램 실적·지출 집계";
    let mut lines = vec![
        format!("# {title}"),
        String::new(),
        format!(
            "조회 프로그램: {}",
            filter["program"].as_str().unwrap_or("전체")
        ),
        format!(
            "조회 종류: {}",
            match kind {
                Some("attendance") => "인원 실적만 (지출 미조회)",
                Some("expense") => "지출만 (인원 실적 미조회)",
                _ => "인원 실적과 지출",
            }
        ),
        format!(
            "조회 기간: {} ~ {}",
            filter["from"].as_str().unwrap_or("제한 없음"),
            filter["to"].as_str().unwrap_or("제한 없음")
        ),
        format!("자료 버전: {}", result["revision"]),
        format!("자료 확인값: {}", result["sourceHash"].as_str().unwrap()),
        String::new(),
    ];
    if kind != Some("expense") {
        lines.push(format!("회기 수: {}", t["sessions"]));
        lines.push(format!("연인원: {}명", t["totalAttendance"]));
        lines.push(if t["uniqueParticipants"].is_null() {
            "실인원: 미확정 (참여자 ID 없는 회기 포함)".into()
        } else {
            format!("실인원: {}명", t["uniqueParticipants"])
        });
    }
    if kind != Some("attendance") {
        lines.push(format!(
            "지출: {}원 ({}건)",
            t["expenseTotal"], t["expenseCount"]
        ));
    }
    lines.extend([String::new(), "## 프로그램별 집계".into()]);
    for row in result["byProgram"].as_array().unwrap() {
        lines.extend([
            String::new(),
            format!("프로그램: {}", row["program"].as_str().unwrap()),
        ]);
        if kind != Some("expense") {
            let unique = if row["uniqueParticipants"].is_null() {
                "미확정".into()
            } else {
                format!("{}명", row["uniqueParticipants"])
            };
            lines.push(format!(
                "회기 {}회, 연인원 {}명, 실인원 {unique}",
                row["sessions"], row["totalAttendance"]
            ));
        }
        if kind != Some("attendance") {
            lines.push(format!("지출 {}원", row["expenseTotal"]));
        }
    }
    lines.extend([String::new(),"이 자료는 조회 시점의 집계입니다. 문서 반영 전 기간·프로그램·자료 버전을 확인하고, 참여자 반응·성과·평가는 확인된 사실로 별도 작성합니다.".into()]);
    let blocks = lines
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(text) = s.strip_prefix("# ") {
                json!({"type":"heading","level":1,"text":text})
            } else if let Some(text) = s.strip_prefix("## ") {
                json!({"type":"heading","level":2,"text":text})
            } else {
                json!({"type":"paragraph","text":s})
            }
        })
        .collect::<Vec<_>>();
    let markdown = lines
        .iter()
        .map(|line| {
            if line.starts_with("프로그램: ") || line.starts_with("조회 프로그램: ") {
                let mut escaped = String::new();
                for c in line.chars() {
                    if "\\`*_{}[]()#+.!<>|&~-".contains(c) {
                        escaped.push('\\');
                    }
                    escaped.push(c);
                }
                escaped
            } else {
                line.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    result["document"] = json!({"schemaVersion":1,"title":title,"blocks":blocks});
    result["markdown"] = json!(markdown);
    result["status"] = json!("report-data-ready");
    result["note"] = json!(format!(
        "{NOTE} 한글 원고는 특수문자 보존을 위해 document를 JSON으로 저장하세요. Markdown은 대화용입니다. 원고·파일 작성 승인은 별도로 확인하고 조회 뒤 장부가 변경되면 다시 집계하세요."
    ));
    Ok(result)
}

pub fn preview_file(root: &Path, input: &str) -> Result<Value> {
    let content = workspace::text(root, input, &["md", "json"])?;
    preview(
        root,
        &serde_json::from_str(&content).map_err(|e| e.to_string())?,
    )
}
