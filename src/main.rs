use autosw::{
    ledger, service,
    workspace::{self, Result},
};
use serde_json::json;
use std::{
    collections::HashMap,
    io::{self, Write},
    process::ExitCode,
};

const COMMANDS: &[(&str, &[&str])] = &[
    ("init", &["workspace"]),
    ("doctor", &["workspace"]),
    ("guide", &["topic"]),
    ("list", &["workspace"]),
    ("read", &["workspace", "input"]),
    ("write-draft", &["workspace", "output", "content"]),
    ("inspect", &["workspace", "input"]),
    (
        "preview",
        &["workspace", "kind", "input", "output", "request"],
    ),
    ("apply", &["workspace", "plan", "confirm"]),
    ("verify", &["workspace", "plan"]),
    ("render", &["workspace", "input", "output", "engine"]),
    (
        "check-hangul",
        &[
            "workspace",
            "input",
            "output",
            "pdf",
            "plan",
            "format",
            "verification",
        ],
    ),
    (
        "publish",
        &[
            "workspace",
            "report",
            "confirm",
            "output",
            "expected",
            "reviewed",
        ],
    ),
    ("ledger-preview", &["workspace", "input"]),
    (
        "ledger-apply",
        &["workspace", "plan", "confirm", "approved"],
    ),
    (
        "ledger-query",
        &["workspace", "program", "from", "to", "kind"],
    ),
    (
        "ledger-report",
        &["workspace", "program", "from", "to", "kind"],
    ),
];

fn run(args: &[String]) -> Result<String> {
    let Some(command) = args.first().filter(|s| s.as_str() != "--help") else {
        return Ok(format!(
            "autosw: {}\n작업 명령은 --workspace <개인폴더>를 사용합니다. 자세한 절차: guide 또는 README.md\n",
            COMMANDS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    };
    if command == "--version" && args.len() == 1 {
        return Ok(format!("autosw {}\n", env!("CARGO_PKG_VERSION")));
    }
    let (_, allowed) = COMMANDS
        .iter()
        .find(|(name, _)| name == command)
        .ok_or("알 수 없는 명령입니다. --help를 확인하세요.")?;
    let mut rest = args[1..].iter();
    let mut opts = HashMap::new();
    while let Some(arg) = rest.next() {
        let key = arg
            .strip_prefix("--")
            .filter(|s| allowed.contains(s) && !opts.contains_key(s))
            .ok_or("잘못되거나 중복된 인자입니다.")?;
        let value = if matches!(key, "approved" | "reviewed") {
            ""
        } else {
            rest.next()
                .filter(|s| !s.starts_with("--"))
                .map(String::as_str)
                .ok_or_else(|| format!("--{key} 값이 필요합니다."))?
        };
        opts.insert(key, value);
    }
    let opt = |key| opts.get(key).copied();
    let req = |key| opt(key).ok_or_else(|| format!("--{key} 값을 지정하세요."));
    let workspace = opt("workspace");
    let result = match command.as_str() {
        "init" => workspace::init(workspace)?,
        "doctor" => service::doctor(workspace)?,
        "guide" => service::guide(opt("topic"))?,
        name => {
            let root = workspace::root(workspace)?;
            match name {
                "list" => service::list(&root)?,
                "read" => service::read(&root, req("input")?)?,
                "write-draft" => service::write_draft(&root, req("output")?, req("content")?)?,
                "inspect" => service::inspect(&root, req("input")?)?,
                "preview" => service::preview(
                    &root,
                    req("kind")?,
                    req("input")?,
                    req("output")?,
                    opt("request"),
                )?,
                "apply" => service::apply(&root, req("plan")?, req("confirm")?)?,
                "verify" => service::verify(&root, req("plan")?)?,
                "render" => service::render(
                    &root,
                    req("input")?,
                    req("output")?,
                    opt("engine").unwrap_or("native"),
                )?,
                "check-hangul" => service::check_hangul(
                    &root,
                    service::HangulOptions {
                        input: req("input")?,
                        output: req("output")?,
                        pdf: req("pdf")?,
                        plan: opt("plan"),
                        format: opt("format").unwrap_or("HWPX"),
                        verification: opt("verification"),
                    },
                )?,
                "publish" => service::publish(
                    &root,
                    req("report")?,
                    req("confirm")?,
                    req("output")?,
                    opt("expected"),
                    opt("reviewed").is_some(),
                )?,
                "ledger-preview" => ledger::preview_file(&root, req("input")?)?,
                "ledger-apply" => ledger::apply(
                    &root,
                    req("plan")?,
                    req("confirm")?,
                    opt("approved").is_some(),
                )?,
                "ledger-query" | "ledger-report" => {
                    let mut filter = json!({});
                    for key in ["program", "from", "to", "kind"] {
                        if let Some(value) = opt(key) {
                            filter[key] = json!(value);
                        }
                    }
                    if name == "ledger-query" {
                        ledger::query(&root, &filter)?
                    } else {
                        ledger::report(&root, &filter)?
                    }
                }
                _ => unreachable!(),
            }
        }
    };
    Ok(serde_json::to_string_pretty(&result).map_err(|e| e.to_string())? + "\n")
}

fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|s| {
            s.into_string()
                .map_err(|_| "올바른 Unicode 인자를 사용하세요.".into())
        })
        .collect::<Result<Vec<_>>>();
    match args.and_then(|args| run(&args)).and_then(|output| {
        io::stdout()
            .lock()
            .write_all(output.as_bytes())
            .map_err(workspace::err)
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("실패: {error}");
            ExitCode::FAILURE
        }
    }
}
