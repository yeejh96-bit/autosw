use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const LEDGER: &[u8] = include_bytes!("fixtures/ledger-v1.json");
const REPORT: &str = include_str!("fixtures/ledger-v1-report.json");

fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_autosw"))
        .arg(args[0])
        .arg("--workspace")
        .arg(root)
        .args(&args[1..])
        .output()
        .unwrap()
}
fn ok(root: &Path, args: &[&str]) -> Value {
    let output = call(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fail(root: &Path, args: &[&str]) {
    assert!(!call(root, args).status.success(), "{args:?}");
}
fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap()
}

#[test]
fn convert_writes_markdown_inside_workspace_and_protects_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("한글 # 변환");
    ok(&root, &["init"]);
    let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0];
    let docx = autosw::office_zip::create_zip(&[(
        "word/document.xml".into(),
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>합성 워드 본문</w:t></w:r></w:p></w:body></w:document>"#.as_bytes().to_vec(),
    )])
    .unwrap();
    let hwpx = autosw::office_zip::create_zip(&[(
        "Contents/section0.xml".into(),
        r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:t>합성 한글 본문</hp:t></hp:run></hp:p></hs:sec>"#.as_bytes().to_vec(),
    )])
    .unwrap();
    let sources = root.join("templates");
    fs::create_dir(sources.join("하위")).unwrap();
    fs::write(sources.join("같은이름.docx"), &docx).unwrap();
    fs::write(sources.join("하위/같은이름.hwpx"), &hwpx).unwrap();
    fs::write(sources.join("암호.docx"), ole).unwrap();
    fs::write(sources.join("~$잠금.docx"), b"lock").unwrap();

    let args = ["convert", "--input", "templates", "--output", "drafts/변환"];
    let first = ok(&root, &args);
    assert_eq!(first["status"], "converted-with-skips");
    assert_eq!(first["converted"], 2);
    assert_eq!(first["skippedProtected"], 1);
    let word = ok(&root, &["read", "--input", "drafts/변환/같은이름.docx.md"]);
    assert!(field(&word, "content").contains("합성 워드 본문"));
    assert!(
        fs::read_to_string(root.join("drafts/변환/하위/같은이름.hwpx.md"))
            .unwrap()
            .contains("합성 한글 본문")
    );
    assert!(!root.join("drafts/변환/암호.docx.md").exists());
    assert_eq!(fs::read(sources.join("같은이름.docx")).unwrap(), docx);
    assert_eq!(ok(&root, &args)["reused"], 2);

    let result = root.join("drafts/변환/같은이름.docx.md");
    fs::write(&result, "사용자가 고친 글").unwrap();
    fail(&root, &args);
    assert_eq!(fs::read_to_string(&result).unwrap(), "사용자가 고친 글");

    for (input, output) in [
        ("templates", "templates/결과"),
        ("drafts", "drafts/변환/다시"),
        ("../밖.docx", "drafts/결과"),
        ("templates/같은이름.docx", "../밖"),
        (".autosw", "drafts/결과"),
    ] {
        fail(&root, &["convert", "--input", input, "--output", output]);
    }
    fs::write(sources.join("이름만.hwpx"), ole).unwrap();
    fail(&root, &["inspect", "--input", "templates/이름만.hwpx"]);
}

#[test]
fn legacy_ledger_opens_and_reports_unchanged_before_approved_write() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("한글 # 기존 장부");
    ok(&root, &["init"]);
    let file = root.join(".autosw/ledger.json");
    fs::write(&file, LEDGER).unwrap();
    let config = root.join("autosw.config.json");
    let settings = br#"{"schemaVersion":1,"institution":"Existing"}"#;
    fs::write(&config, settings).unwrap();
    ok(&root, &["init"]);
    assert_eq!(fs::read(&config).unwrap(), settings);
    assert_eq!(
        ok(&root, &["ledger-report"]),
        serde_json::from_str::<Value>(REPORT).unwrap()
    );
    let expense = ok(
        &root,
        &[
            "ledger-query",
            "--kind",
            "expense",
            "--from",
            "2026-11-09",
            "--to",
            "2026-11-09",
        ],
    );
    assert_eq!(expense["totals"]["expenseTotal"], 1500);
    assert!(expense["totals"]["totalAttendance"].is_null());
    let change = json!({"operation":"add","kind":"expense","data":{"program":"합성 *교육* [A]","date":"2026-11-09","entryKey":"신규/1","item":"합성 종이","amount":500}});
    fs::write(root.join("drafts/change.json"), change.to_string()).unwrap();
    let preview = ok(&root, &["ledger-preview", "--input", "drafts/change.json"]);
    let (id, hash) = (field(&preview, "planId"), field(&preview, "confirmHash"));
    fail(&root, &["ledger-apply", "--plan", id, "--confirm", hash]);
    fail(
        &root,
        &[
            "ledger-apply",
            "--plan",
            id,
            "--confirm",
            &"0".repeat(64),
            "--approved",
        ],
    );
    assert_eq!(fs::read(&file).unwrap(), LEDGER);
    let args = [
        "ledger-apply",
        "--plan",
        id,
        "--confirm",
        hash,
        "--approved",
    ];
    let (a, b) = std::thread::scope(|thread| {
        let first = thread.spawn(|| call(&root, &args));
        let second = call(&root, &args);
        (first.join().unwrap(), second)
    });
    assert!(a.status.success() || b.status.success());
    for result in [a, b] {
        assert!(
            result.status.success()
                || String::from_utf8_lossy(&result.stderr).contains("다른 장부 쓰기")
        );
    }
    assert_eq!(ok(&root, &args)["status"], "already-applied");
    assert_eq!(ok(&root, &["ledger-query"])["totals"]["expenseTotal"], 2000);
    let old: Value = serde_json::from_slice(LEDGER).unwrap();
    let new: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(new["schemaVersion"], 1);
    for key in ["history", "applied", "attendance", "expense"] {
        assert!(
            new[key]
                .as_array()
                .unwrap()
                .starts_with(old[key].as_array().unwrap())
        );
    }
}

#[test]
fn document_preview_apply_fill_and_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("한글 # 문서");
    ok(&root, &["init"]);
    let (input, first, second) = ("drafts/원고.md", "outputs/원본.hwpx", "outputs/후보.hwpx");
    ok(
        &root,
        &[
            "write-draft",
            "--output",
            input,
            "--content",
            "# 합성 제목\n\n검증 문단\n",
        ],
    );
    fail(
        &root,
        &["write-draft", "--output", input, "--content", "교체 시도"],
    );
    fail(&root, &["read", "--input", "../밖.md"]);
    fail(&root, &["read", "--input", ".autosw/ledger.json"]);
    fail(
        &root,
        &[
            "write-draft",
            "--output",
            "drafts/nul.md",
            "--content",
            "예약 이름",
        ],
    );
    let plan = ok(
        &root,
        &[
            "preview", "--kind", "create", "--input", input, "--output", first,
        ],
    );
    let (id, hash) = (field(&plan, "planId"), field(&plan, "confirmHash"));
    fail(
        &root,
        &["apply", "--plan", id, "--confirm", &"0".repeat(64)],
    );
    assert!(!root.join(first).exists());
    ok(&root, &["apply", "--plan", id, "--confirm", hash]);
    assert_eq!(
        ok(&root, &["verify", "--plan", id])["independentCheck"],
        "passed"
    );
    let original = fs::read(root.join(first)).unwrap();
    let structure = ok(&root, &["inspect", "--input", first]);
    let target = structure["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["writable"] == true && p["before"][0] == "검증 문단")
        .unwrap();
    let request = json!({"schemaVersion":1,"sourceSha256":structure["sourceSha256"],"changes":[{"target":target["key"],"label":"검증","source":"합성 입력","action":"set","expected":target["before"],"values":["바꾼 문단"]}],"requiredTargets":[target["key"]],"unmapped":[]});
    fs::write(root.join("drafts/fill.json"), request.to_string()).unwrap();
    let plan = ok(
        &root,
        &[
            "preview",
            "--kind",
            "fill",
            "--input",
            first,
            "--output",
            second,
            "--request",
            "drafts/fill.json",
        ],
    );
    let id = field(&plan, "planId");
    ok(
        &root,
        &[
            "apply",
            "--plan",
            id,
            "--confirm",
            field(&plan, "confirmHash"),
        ],
    );
    assert_eq!(
        ok(&root, &["verify", "--plan", id])["status"],
        "generated-awaiting-hangul"
    );
    assert_eq!(fs::read(root.join(first)).unwrap(), original);
    assert!(
        ok(&root, &["inspect", "--input", second])
            .to_string()
            .contains("바꾼 문단")
    );
    let outside = temp.path().join("outside.md");
    fs::write(&outside, "합성 외부").unwrap();
    fs::hard_link(&outside, root.join("drafts/link.md")).unwrap();
    fail(&root, &["read", "--input", "drafts/link.md"]);
}
