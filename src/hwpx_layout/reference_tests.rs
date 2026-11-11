//! Replays synthetic requests captured after each fixed JavaScript comparison passed.
//! Each compressed JSON Lines record pairs the input with the fully verified output
//! or rejection. The reference corpus runs without JavaScript or installed fonts.

use super::probe;
use crate::hwpx_form::json_stringify;
use data_encoding::BASE64;
use flate2::bufread::GzDecoder;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD_MARKER: &str = "MY_SYSTEM_LAYOUT_REFERENCE_CHILD";
const CHILD_TEST: &str = "hwpx_layout::reference_tests::isolated_font_case";
const FONT_NAMES: [&str; 2] = ["H2HDRM.TTF", "H2MJSM.TTF"];

fn compare(record: &Value, label: &str) {
    let command = record["command"].as_str().unwrap();
    let result = probe(command, &record["request"]);
    match record["status"].as_u64().unwrap() {
        0 => {
            let expected = record["expected"].as_str().unwrap();
            let actual = json_stringify(&result.unwrap_or_else(|err| panic!("{label}: {err}")));
            if actual != expected {
                let differing_byte = actual
                    .bytes()
                    .zip(expected.bytes())
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or_else(|| actual.len().min(expected.len()));
                panic!(
                    "{label}: output differs at byte {differing_byte} (expected {} bytes, actual {} bytes)",
                    expected.len(),
                    actual.len()
                );
            }
        }
        1 => assert_eq!(
            result.expect_err(label).trim_end(),
            record["error"].as_str().unwrap(),
            "{label}"
        ),
        status => panic!("{label}: unexpected captured status {status}"),
    }
}

fn compare_with_fonts(mut record: Value, label: &str) {
    let temporary = tempfile::Builder::new()
        .prefix("my-system-layout-reference-#-글꼴-")
        .tempdir()
        .unwrap();
    let system_root = temporary.path().join("합성 Windows #");
    let font_dir = system_root.join("Fonts");
    fs::create_dir_all(&font_dir).unwrap();
    let fonts = record
        .as_object_mut()
        .unwrap()
        .swap_remove("fonts")
        .unwrap();
    let mut before = Vec::new();
    for (name, encoded) in fonts.as_object().unwrap() {
        assert!(FONT_NAMES.contains(&name.as_str()), "{label}: {name}");
        let path = font_dir.join(name);
        let bytes = BASE64.decode(encoded.as_str().unwrap().as_bytes()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        before.push((path, bytes, modified));
    }
    let case_path = temporary.path().join("reference.json");
    fs::write(&case_path, serde_json::to_vec(&record).unwrap()).unwrap();

    // Give each child its own Windows font root without changing process-wide
    // environment variables while the other Rust tests run concurrently.
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .current_dir(temporary.path())
        .env(CHILD_MARKER, label)
        .env("SystemRoot", &system_root)
        .env("PATH", "")
        .env_remove("KORDOC_MAX_UNZIP_MB")
        .output()
        .unwrap();
    assert!(
        result.status.success() && String::from_utf8_lossy(&result.stdout).contains("1 passed"),
        "{label}: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_dir(&font_dir).unwrap().count(),
        before.len(),
        "{label}"
    );
    for (path, bytes, modified) in before {
        assert!(
            fs::read(&path).unwrap() == bytes,
            "{label}: changed {path:?}"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "{label}"
        );
    }
    assert_eq!(
        fs::read_dir(temporary.path()).unwrap().count(),
        2,
        "{label}"
    );
    let root = temporary.path().to_owned();
    temporary.close().unwrap();
    assert!(!root.exists(), "{label}: temporary files remain");
}

#[test]
fn isolated_font_case() {
    let Ok(label) = std::env::var(CHILD_MARKER) else {
        return;
    };
    let record = serde_json::from_reader(File::open("reference.json").unwrap()).unwrap();
    compare(&record, &label);
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/layout")
        .join(format!("{name}.jsonl.gz"))
}

fn replay(fixtures: &[&str]) {
    for name in fixtures {
        let path = fixture_path(name);
        let file = File::open(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let reader = BufReader::new(GzDecoder::new(BufReader::new(file)));
        let mut records = 0;
        for (index, line) in reader.lines().enumerate() {
            let label = format!("{name} request {}", index + 1);
            let record: Value =
                serde_json::from_str(&line.unwrap()).unwrap_or_else(|err| panic!("{label}: {err}"));
            if record["fonts"].is_object() {
                compare_with_fonts(record, &label);
            } else {
                compare(&record, &label);
            }
            records += 1;
        }
        assert!(records > 0, "empty layout reference corpus: {name}");
    }
}

#[test]
fn character_metrics_and_styles() {
    let provenance: Value =
        serde_json::from_str(include_str!("data/font-metrics-provenance.json")).unwrap();
    assert_eq!(
        crate::hwpx_document::sha256(include_bytes!("data/font-metrics.json")),
        provenance["outputSha256"].as_str().unwrap()
    );
    replay(&[
        "metrics",
        "utf16-metrics",
        "head-styles",
        "header-compat",
        "font-env",
        "line-plan",
        "text",
        "pua",
    ]);
}

#[test]
fn paragraphs_and_page_flow() {
    replay(&[
        "para-model",
        "numbering",
        "object-anchor",
        "page-stories",
        "reflow-helpers",
        "reflow-children",
        "reflow-para",
        "reflow-block",
    ]);
}

#[test]
fn table_and_shape_layout() {
    replay(&[
        "primitives",
        "table-flow",
        "table-pages",
        "table-split",
        "shape-geometry",
        "equation",
    ]);
}

#[test]
fn svg_and_complete_document() {
    replay(&[
        "svg-para",
        "svg-images",
        "svg-objects",
        "svg-table",
        "svg-assemble",
        "section-render",
        "hwpx-loader",
        "preview-fonts",
    ]);
}

#[test]
fn changed_document_renders_new_text_without_touching_the_original() {
    use super::hwpx_loader::render_hwpx_layout;
    use crate::hwpx_compose::compose_hwpx;
    use crate::hwpx_document::sha256;

    let make_document = |text| {
        compose_hwpx(&serde_json::json!({
            "schemaVersion": 1,
            "blocks": [
                {"type": "paragraph", "text": text},
                {"type": "table", "rows": [["항목", "값"], ["확인", "123"]]}
            ]
        }))
        .unwrap()
        .bytes
    };
    let original = make_document("수정 전 본문");
    let hash = sha256(&original);
    let first = render_hwpx_layout(&original).unwrap();
    let second = render_hwpx_layout(&make_document("수정 후 본문")).unwrap();
    assert_eq!(sha256(&original), hash);
    assert_eq!(first.report["sourceSha256"].as_str(), Some(hash.as_str()));
    assert_eq!(first.report["pageCount"].as_u64(), Some(1));
    assert_eq!(first.report["stats"]["tables"].as_u64(), Some(1));
    assert_eq!(first.report["hangulVerified"].as_bool(), Some(false));
    assert!(first.svgs[0].contains("수정 전 본문"));
    assert!(second.svgs[0].contains("수정 후 본문"));
    assert!(!second.svgs[0].contains("수정 전 본문"));
}
