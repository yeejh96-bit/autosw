//! Owned hidden Chromium/CDP transport shared by canvas measurement and PDF printing.
use crate::owned_browser_process::OwnedProcess;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    ffi::OsString,
    fs,
    net::TcpStream,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket};

#[derive(Clone, Copy)]
pub enum LaunchPolicy {
    Canvas,
    Pdf,
}

// Render-affecting and background-resource defaults used by the fixed Playwright
// launcher. Browser sandbox remains enabled; these flags alter only our process.
const PDF_ARGUMENTS: &[&str] = &[
    "--disable-field-trial-config",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-back-forward-cache",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-component-extensions-with-background-pages",
    "--disable-default-apps",
    "--disable-dev-shm-usage",
    "--disable-edgeupdater",
    "--disable-features=AvoidUnnecessaryBeforeUnloadCheckSync,BoundaryEventDispatchTracksNodeRemoval,DestroyProfileOnBrowserClose,DialMediaRouteProvider,GlobalMediaControls,HttpsUpgrades,LensOverlay,MediaRouter,PaintHolding,ThirdPartyStoragePartitioning,BlockOriginHeaderModificationOnRedirect,Translate,AutoDeElevate,OptimizationHints,msForceBrowserSignIn,msEdgeUpdateLaunchServicesPreferredVersion",
    "--allow-pre-commit-input",
    "--disable-hang-monitor",
    "--disable-ipc-flooding-protection",
    "--disable-popup-blocking",
    "--disable-prompt-on-repost",
    "--disable-renderer-backgrounding",
    "--disable-updater-scheduler",
    "--force-color-profile=srgb",
    "--use-mock-keychain",
    "--no-service-autorun",
    "--export-tagged-pdf",
    "--disable-search-engine-choice-screen",
    "--edge-skip-compat-layer-relaunch",
    "--disable-infobars",
    "--hide-scrollbars",
    "--mute-audio",
    "--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4",
];
fn pdf_candidates() -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    for key in ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        if let Some(base) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            prefixes.push(PathBuf::from(base));
        }
    }
    if let Some(drive) = std::env::var_os("HOMEDRIVE").filter(|v| !v.is_empty()) {
        let mut base = drive.clone();
        base.push("\\Program Files");
        prefixes.push(PathBuf::from(base));
        let mut base = drive;
        base.push("\\Program Files (x86)");
        prefixes.push(PathBuf::from(base));
    }
    let mut paths = Vec::new();
    for relative in [
        "Microsoft/Edge/Application/msedge.exe",
        "Google/Chrome/Application/chrome.exe",
    ] {
        if let Some(path) = prefixes
            .iter()
            .map(|base| base.join(relative))
            .find(|path| path.exists())
        {
            paths.push(path);
        }
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|v| PathBuf::from(v).join("AppData/Local"))
        });
    if let Some(base) = base
        && let Ok(entries) = fs::read_dir(base.join("ms-playwright"))
    {
        // Node readdir returns names sorted using UTF-16 on Windows.
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path().join("chrome-win64/chrome.exe");
            if entry.file_name().to_string_lossy().starts_with("chromium-") && path.is_file() {
                paths.push(path);
                break;
            }
        }
    }
    paths
}

pub struct BrowserSession {
    socket: WebSocket<TcpStream>,
    process: OwnedProcess,
    profile: Option<tempfile::TempDir>,
    session: String,
    events: VecDeque<Value>,
    auto_dismiss_dialogs: bool,
    next_id: u64,
}

pub(crate) fn canvas_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for relative in [
        "Microsoft/Edge/Application/msedge.exe",
        "Google/Chrome/Application/chrome.exe",
    ] {
        for variable in ["PROGRAMFILES(X86)", "PROGRAMFILES", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(variable) {
                paths.push(PathBuf::from(base).join(relative));
            }
        }
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA")
        && let Ok(entries) = fs::read_dir(PathBuf::from(base).join("ms-playwright"))
    {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if entry.file_name().to_string_lossy().starts_with("chromium-") {
                paths.push(entry.path().join("chrome-win64/chrome.exe"));
            }
        }
    }
    paths.retain(|path| path.is_file());
    paths
}

fn endpoint(text: &str) -> Result<(u16, &str), String> {
    let mut lines = text.lines();
    let port: u16 = lines
        .next()
        .ok_or("브라우저 포트 없음")?
        .parse()
        .map_err(|_| "브라우저 포트 오류")?;
    let path = lines.next().ok_or("브라우저 연결 경로 없음")?;
    let suffix = path
        .strip_prefix("/devtools/browser/")
        .ok_or("브라우저 연결 경로 오류")?;
    if port == 0
        || suffix.is_empty()
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("브라우저 연결 경로 오류".into());
    }
    Ok((port, path))
}

fn close_profile(profile: tempfile::TempDir) -> Result<(), String> {
    // Keep ownership of the exact temporary path so raw Windows error codes
    // remain available; TempDir::close wraps them with a path error.
    let path = profile.keep();
    let mut result = fs::remove_dir_all(&path);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error)
                if matches!(error.raw_os_error(), Some(5 | 32 | 33))
                    && Instant::now() < deadline =>
            {
                // This exact absolute directory was allocated by TempDir for
                // this browser alone. Windows may release file handles just
                // after the final process/accounting notification.
                std::thread::sleep(Duration::from_millis(50));
                result = fs::remove_dir_all(&path);
            }
            Err(error) => return Err(format!("브라우저 정리 실패: {error}; {}", path.display())),
        }
    }
}

impl BrowserSession {
    pub fn launch(policy: LaunchPolicy) -> Result<Self, String> {
        Self::launch_with(policy, |_| Ok(())).map(|(browser, ())| browser)
    }
    /// Canvas initialization belongs to each candidate attempt. PDF page setup
    /// remains fatal after the browser has launched, as in the fixed wrapper.
    pub fn launch_with<T>(
        policy: LaunchPolicy,
        configure: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<(Self, T), String> {
        let candidates = match policy {
            LaunchPolicy::Canvas => canvas_candidates(),
            LaunchPolicy::Pdf => pdf_candidates(),
        };
        Self::launch_candidates_with(policy, candidates, configure)
    }
    fn launch_candidates_with<T>(
        policy: LaunchPolicy,
        candidates: Vec<PathBuf>,
        mut configure: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<(Self, T), String> {
        for candidate in candidates {
            match Self::launch_at(&candidate, policy) {
                Ok(mut canvas) => match canvas
                    .setup_target(policy)
                    .and_then(|()| configure(&mut canvas))
                {
                    Ok(configured) => return Ok((canvas, configured)),
                    Err(error) => {
                        canvas.close_inner()?;
                        if matches!(policy, LaunchPolicy::Pdf) {
                            return Err(error);
                        }
                    }
                },
                Err(error) if error.starts_with("브라우저 정리 실패") => return Err(error),
                Err(_) => {}
            }
        }
        Err(match policy {
            LaunchPolicy::Canvas => {
                "브라우저를 찾거나 연결하지 못했습니다. Microsoft Edge 설치를 확인해 주세요."
            }
            LaunchPolicy::Pdf => "브라우저를 찾지 못했습니다. Microsoft Edge 설치를 확인해 주세요.",
        }
        .into())
    }
    fn launch_at(executable: &Path, policy: LaunchPolicy) -> Result<Self, String> {
        let profile = tempfile::Builder::new()
            .prefix(match policy {
                LaunchPolicy::Canvas => "my-system-rust-canvas-",
                LaunchPolicy::Pdf => "my-system-rust-pdf-",
            })
            .tempdir()
            .map_err(|e| e.to_string())?;
        Self::launch_with_profile(executable, profile, policy)
    }
    fn launch_with_profile(
        executable: &Path,
        profile: tempfile::TempDir,
        policy: LaunchPolicy,
    ) -> Result<Self, String> {
        let mut arguments: Vec<OsString> = [
            "--headless",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-extensions",
            "--metrics-recording-only",
            "--password-store=basic",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        if matches!(policy, LaunchPolicy::Pdf) {
            arguments.retain(|arg| arg != "--disable-gpu");
            arguments.extend(PDF_ARGUMENTS.iter().map(OsString::from));
        }
        arguments.push(format!("--user-data-dir={}", profile.path().display()).into());
        arguments.push("about:blank".into());
        let mut process = match OwnedProcess::spawn(executable, &arguments) {
            Ok(process) => process,
            Err(error) => {
                close_profile(profile)?;
                return Err(error);
            }
        };
        let connect = (|| {
            let deadline = Instant::now()
                + Duration::from_secs(match policy {
                    LaunchPolicy::Canvas => 20,
                    LaunchPolicy::Pdf => 180,
                });
            let text = loop {
                if process.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err("브라우저 시작 실패".to_owned());
                }
                if let Ok(text) = fs::read_to_string(profile.path().join("DevToolsActivePort"))
                    && endpoint(&text).is_ok()
                {
                    break text;
                }
                if Instant::now() >= deadline {
                    return Err("브라우저 연결 시간 초과".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            let (port, path) = endpoint(&text)?;
            let stream =
                TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(5))
                    .map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(30)))
                .map_err(|e| e.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(30)))
                .map_err(|e| e.to_string())?;
            stream.set_nodelay(true).map_err(|e| e.to_string())?;
            let (socket, _) = tungstenite::client(format!("ws://127.0.0.1:{port}{path}"), stream)
                .map_err(|e| e.to_string())?;
            Ok(socket)
        })();
        let socket = match connect {
            Ok(socket) => socket,
            Err(error) => {
                process
                    .kill()
                    .and_then(|()| process.wait())
                    .map_err(|cleanup| {
                        format!(
                            "브라우저 정리 실패: {cleanup}; {}",
                            profile.path().display()
                        )
                    })?;
                close_profile(profile)?;
                return Err(error);
            }
        };
        Ok(Self {
            socket,
            process,
            profile: Some(profile),
            session: String::new(),
            events: VecDeque::new(),
            auto_dismiss_dialogs: matches!(policy, LaunchPolicy::Pdf),
            next_id: 0,
        })
    }
    fn setup_target(&mut self, policy: LaunchPolicy) -> Result<(), String> {
        let mut target_params = json!({"url":"about:blank"});
        if matches!(policy, LaunchPolicy::Pdf) {
            let context = self.call(
                "Target.createBrowserContext",
                json!({"disposeOnDetach":true}),
                false,
            )?;
            target_params["browserContextId"] = context["browserContextId"].clone();
        }
        let target = self.call("Target.createTarget", target_params, false)?;
        let attached = self.call(
            "Target.attachToTarget",
            json!({"targetId":target["targetId"],"flatten":true}),
            false,
        )?;
        self.session = attached["sessionId"]
            .as_str()
            .ok_or("브라우저 세션 없음")?
            .into();
        Ok(())
    }
    pub fn call(&mut self, method: &str, params: Value, session: bool) -> Result<Value, String> {
        self.call_timeout(method, params, session, Some(Duration::from_secs(30)))
    }
    /// None preserves Playwright's unbounded evaluate/print/stream calls.
    pub fn call_timeout(
        &mut self,
        method: &str,
        params: Value,
        session: bool,
        timeout: Option<Duration>,
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id":id,"method":method,"params":params});
        if session {
            request["sessionId"] = json!(self.session);
        }
        self.socket
            .send(Message::Text(request.to_string().into()))
            .map_err(|e| e.to_string())?;
        let deadline = timeout.map(|duration| Instant::now() + duration);
        loop {
            let response = self.read_message(deadline)?;
            if response["id"].as_u64() != Some(id) {
                self.keep_event(response)?;
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(format!("브라우저 명령 오류: {error}"));
            }
            let result = response
                .get("result")
                .cloned()
                .ok_or("브라우저 결과 없음")?;
            if result.get("exceptionDetails").is_some() {
                return Err("브라우저 스크립트 실행 실패".into());
            }
            return Ok(result);
        }
    }
    fn read_message(&mut self, deadline: Option<Instant>) -> Result<Value, String> {
        loop {
            let remaining = match deadline {
                Some(end) => Some(
                    end.checked_duration_since(Instant::now())
                        .filter(|v| !v.is_zero())
                        .ok_or("브라우저 응답 시간 초과")?,
                ),
                None => None,
            };
            self.socket
                .get_mut()
                .set_read_timeout(remaining)
                .map_err(|e| e.to_string())?;
            match self.socket.read().map_err(|e| e.to_string())? {
                Message::Text(text) => {
                    return serde_json::from_str(&text).map_err(|e| e.to_string());
                }
                Message::Close(_) => return Err("브라우저 연결 종료".into()),
                _ => {}
            }
        }
    }
    fn keep_event(&mut self, event: Value) -> Result<(), String> {
        if event.get("method").is_none() {
            return Ok(());
        }
        if event["sessionId"].as_str() != Some(self.session.as_str()) {
            return Ok(());
        }
        if event["method"] == "Inspector.targetCrashed" {
            return Err("브라우저 페이지 종료".into());
        }
        if self.auto_dismiss_dialogs && event["method"] == "Page.javascriptDialogOpening" {
            self.next_id += 1;
            let request = json!({"id":self.next_id,"sessionId":self.session,"method":"Page.handleJavaScriptDialog","params":{"accept":false}});
            self.socket
                .send(Message::Text(request.to_string().into()))
                .map_err(|e| e.to_string())?;
        }
        // Only navigation lifecycle events are consumed; discard other traffic.
        if event["method"] == "Page.lifecycleEvent" {
            self.events.push_back(event);
        }
        Ok(())
    }
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
    pub fn wait_for_load(
        &mut self,
        frame: &str,
        loader: &str,
        deadline: Instant,
    ) -> Result<(), String> {
        loop {
            if let Some(index) = self.events.iter().position(|event| {
                event["params"]["name"] == "load"
                    && event["params"]["frameId"] == frame
                    && event["params"]["loaderId"] == loader
            }) {
                self.events.remove(index);
                return Ok(());
            }
            let event = self.read_message(Some(deadline))?;
            self.keep_event(event)?;
        }
    }
    pub fn close(mut self) -> Result<(), String> {
        self.close_inner()
    }
    fn close_inner(&mut self) -> Result<(), String> {
        if self.profile.is_none() {
            return Ok(());
        }
        let _ = self.call("Browser.close", json!({}), false);
        self.process
            .kill()
            .and_then(|()| self.process.wait())
            .map_err(|error| format!("브라우저 정리 실패: {error}"))?;
        if let Some(profile) = self.profile.take() {
            close_profile(profile)?;
        }
        Ok(())
    }
}
impl Drop for BrowserSession {
    fn drop(&mut self) {
        if let Err(error) = self.close_inner() {
            eprintln!("{error}");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_pdf_candidates_return_the_original_installation_error() {
        // Windows may restore PROGRAMFILES at process startup, so a synthetic
        // empty inventory is tested here rather than falsifying the host's env.
        assert_eq!(
            BrowserSession::launch_candidates_with(LaunchPolicy::Pdf, Vec::new(), |_| Ok(()))
                .err()
                .unwrap(),
            "브라우저를 찾지 못했습니다. Microsoft Edge 설치를 확인해 주세요."
        );
    }
    #[cfg(windows)]
    #[test]
    fn installed_browser_retries_canvas_setup_but_not_pdf_setup() {
        let candidates = canvas_candidates();
        assert!(!candidates.is_empty(), "회귀 검증용 설치 브라우저 없음");
        // Repeat the installed inventory so even a machine with only one
        // usable browser can exercise a second, separately owned attempt.
        let repeated = candidates.iter().chain(&candidates).cloned().collect();
        let mut profiles = Vec::new();
        let (mut browser, context) =
            BrowserSession::launch_candidates_with(LaunchPolicy::Canvas, repeated, |browser| {
                if let Some(previous) = profiles.last() {
                    assert!(!std::path::Path::new(previous).exists());
                }
                profiles.push(browser.profile.as_ref().unwrap().path().to_owned());
                let context = crate::browser_canvas::Canvas::configure(browser)?;
                if profiles.len() == 1 {
                    Err("synthetic canvas setup failure".into())
                } else {
                    Ok(context)
                }
            })
            .unwrap();
        let measured=browser.call("Runtime.callFunctionOn",json!({"objectId":context,"functionDeclaration":"function(){return this.measureText('ABC').width;}","returnByValue":true}),true).unwrap();
        assert!(measured["result"]["value"].as_f64().unwrap() > 0.0);
        browser.close().unwrap();
        assert_eq!(profiles.len(), 2);
        assert!(profiles.iter().all(|path| !path.exists()));

        let repeated = candidates.iter().chain(&candidates).cloned().collect();
        let mut profiles = Vec::new();
        let failed =
            BrowserSession::launch_candidates_with(LaunchPolicy::Pdf, repeated, |browser| {
                profiles.push(browser.profile.as_ref().unwrap().path().to_owned());
                Err::<(), _>("synthetic PDF setup failure".into())
            });
        assert_eq!(failed.err().unwrap(), "synthetic PDF setup failure");
        assert_eq!(profiles.len(), 1);
        assert!(profiles.iter().all(|path| !path.exists()));
    }
    #[test]
    fn debugging_endpoint_is_only_a_local_browser_path() {
        assert_eq!(
            endpoint("1234\n/devtools/browser/abc-12\n").unwrap(),
            (1234, "/devtools/browser/abc-12")
        );
        for text in [
            "0\n/devtools/browser/a",
            "42\nhttp://remote",
            "42\n/devtools/browser/../../x",
            "65536\n/devtools/browser/a",
        ] {
            assert!(endpoint(text).is_err());
        }
    }
    #[test]
    fn failed_browser_start_removes_its_owned_profile() {
        let profile = tempfile::Builder::new()
            .prefix("my-system-rust-canvas-failure-")
            .tempdir()
            .unwrap();
        let path = profile.path().to_owned();
        // The Rust test executable rejects Chromium's flags and exits without
        // ever offering a debugging endpoint. Only this new process is owned.
        assert!(
            BrowserSession::launch_with_profile(
                &std::env::current_exe().unwrap(),
                profile,
                LaunchPolicy::Canvas
            )
            .is_err()
        );
        assert!(!path.exists());
    }
    #[cfg(windows)]
    #[test]
    fn profile_cleanup_waits_for_a_releasing_windows_file_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        let profile = tempfile::Builder::new()
            .prefix("my-system-rust-canvas-lock-")
            .tempdir()
            .unwrap();
        let path = profile.path().to_owned();
        let lock = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .share_mode(0)
            .open(path.join("owned-lock"))
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(lock);
        });
        let result = close_profile(profile);
        release.join().unwrap();
        result.unwrap();
        assert!(!path.exists());
    }
}
