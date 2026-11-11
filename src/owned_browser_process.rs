//! An unnamed, non-inheritable Windows Job owns only the process created here
//! and its descendants. The primary thread stays suspended until assignment.
//! A successful wait means the entire Job is empty, not merely its root process.
//!
//! Microsoft contracts used here:
//! https://learn.microsoft.com/windows/win32/procthread/job-objects
//! https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
//! https://learn.microsoft.com/cpp/c-language/parsing-c-command-line-arguments
//! https://learn.microsoft.com/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject
//! https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess
//! https://learn.microsoft.com/windows/win32/api/jobapi/nf-jobapi-isprocessinjob

#[cfg(windows)]
pub use native::{OwnedProcess, spawn_hidden_detached};

#[cfg(not(windows))]
pub struct OwnedProcess;

#[cfg(not(windows))]
impl OwnedProcess {
    pub fn spawn(_: &std::path::Path, _: &[std::ffi::OsString]) -> Result<Self, String> {
        Err("브라우저 프로세스 소유는 Windows에서만 지원합니다.".into())
    }
    pub fn try_wait(&mut self) -> Result<Option<i32>, String> {
        Err("Windows 브라우저 프로세스가 없습니다.".into())
    }
    pub fn kill(&mut self) -> Result<(), String> {
        Err("Windows 브라우저 프로세스가 없습니다.".into())
    }
    pub fn wait(&mut self) -> Result<(), String> {
        Err("Windows 브라우저 프로세스가 없습니다.".into())
    }
}

#[cfg(windows)]
mod native {
    use std::{
        ffi::{OsStr, OsString},
        io::Write,
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::Path,
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{
            ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess,
                OpenProcess, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, ResumeThread, STARTF_USESHOWWINDOW, STARTUPINFOW,
                TerminateProcess, WaitForSingleObject,
            },
        },
    };

    const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

    fn os_error(action: &str) -> String {
        format!("{action}: {}", std::io::Error::last_os_error())
    }

    fn wide(value: &OsStr) -> Result<Vec<u16>, String> {
        let units: Vec<_> = value.encode_wide().collect();
        if units.contains(&0) {
            return Err("브라우저 실행 경로·인자에 NUL 문자를 사용할 수 없습니다.".into());
        }
        Ok(units)
    }

    // Quote every ordinary argument using the MS C runtime rules. Backslashes
    // are doubled only immediately before a quote, including the closing quote.
    fn quote_argument(units: &[u16], output: &mut Vec<u16>) {
        output.push(b'"' as u16);
        let mut slashes = 0;
        for &unit in units {
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            if unit == b'"' as u16 {
                output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            } else {
                output.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            }
            output.push(unit);
            slashes = 0;
        }
        output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        output.push(b'"' as u16);
    }

    fn command_line(executable: &Path, args: &[OsString]) -> Result<(Vec<u16>, Vec<u16>), String> {
        let mut application = wide(executable.as_os_str())?;
        if application.is_empty() || application.contains(&(b'"' as u16)) {
            return Err("잘못된 브라우저 실행 파일 경로입니다.".into());
        }
        // argv[0] has special parsing rules: quote the path literally rather
        // than applying ordinary-argument backslash escaping to it.
        let mut command = vec![b'"' as u16];
        command.extend_from_slice(&application);
        command.push(b'"' as u16);
        for arg in args {
            let units = wide(arg)?;
            command.push(b' ' as u16);
            quote_argument(&units, &mut command);
            if command.len() >= 32767 {
                return Err("브라우저 명령줄 길이 한도를 넘었습니다.".into());
            }
        }
        if command.len() >= 32767 {
            return Err("브라우저 명령줄 길이 한도를 넘었습니다.".into());
        }
        application.push(0);
        command.push(0);
        Ok((application, command))
    }

    #[allow(unsafe_code)]
    fn own_new_handle(handle: HANDLE, action: &str) -> Result<OwnedHandle, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(os_error(action));
        }
        // SAFETY: callers pass a fresh owning handle returned by a successful
        // Windows creation API. It is transferred exactly once to OwnedHandle,
        // whose Drop closes it, including every early-return path.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    #[allow(unsafe_code)]
    fn new_job() -> Result<OwnedHandle, String> {
        // SAFETY: null security attributes request a non-inheritable handle;
        // null name creates a new unnamed Job, never opens another user's Job.
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        let job = own_new_handle(raw, "브라우저 작업 그룹 생성 실패")?;
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: job is live and owned. limits is initialized, correctly sized,
        // immutable for the call, and the API copies the structure. Breakaway
        // permissions are deliberately absent, so descendants stay owned.
        let result = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if result == 0 {
            return Err(os_error("브라우저 작업 그룹 종료 설정 실패"));
        }
        Ok(job)
    }

    // Before assignment succeeds, only this newly created suspended process
    // exists. This guard cannot target a pre-existing process or any parent Job.
    struct SuspendedProcess {
        process: Option<OwnedHandle>,
        thread: OwnedHandle,
    }

    #[allow(unsafe_code)]
    fn spawn_suspended(
        application: &[u16],
        command: &mut [u16],
    ) -> Result<SuspendedProcess, String> {
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESHOWWINDOW,
            wShowWindow: 0, // SW_HIDE; also applies if the executable is a GUI app.
            ..Default::default()
        };
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: application and writable command are NUL-terminated UTF-16
        // buffers alive for this call; startup/information have correct sizes.
        // Explicit application avoids executable-name ambiguity. No handles
        // are inherited, no shell runs, and the child starts suspended/hidden.
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                null(),
                null(),
                &startup,
                &mut information,
            )
        };
        if created == 0 {
            return Err(os_error("브라우저 프로세스 생성 실패"));
        }
        // SAFETY: successful CreateProcessW returns two distinct owning
        // handles. Both are transferred immediately to RAII owners, without
        // any fallible operation between the two transfers.
        let (process, thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(information.hProcess),
                OwnedHandle::from_raw_handle(information.hThread),
            )
        };
        Ok(SuspendedProcess {
            process: Some(process),
            thread,
        })
    }

    #[allow(unsafe_code)]
    fn stop_unassigned(process: &OwnedHandle) -> Result<(), String> {
        // SAFETY: this is the owning handle from CreateProcessW, still suspended
        // before assignment/resume. Terminating it cannot affect an existing
        // browser or any other process in a containing Job.
        let terminated = unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        if terminated == 0 {
            let error = os_error("시작 전 브라우저 종료 실패");
            if root_exit_code(process)?.is_none() {
                return Err(error);
            }
        }
        // SAFETY: the live owned process handle has synchronization rights.
        let status = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
        match status {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err("시작 전 브라우저 종료 확인 시간 초과".into()),
            _ => Err(os_error("시작 전 브라우저 종료 확인 실패")),
        }
    }

    impl Drop for SuspendedProcess {
        fn drop(&mut self) {
            if let Some(process) = &self.process
                && let Err(error) = stop_unassigned(process)
            {
                let _ = writeln!(std::io::stderr().lock(), "[오류] {error}");
            }
        }
    }

    #[allow(unsafe_code)]
    fn assign(job: &OwnedHandle, process: &OwnedHandle) -> Result<(), String> {
        // SAFETY: both handles are live and locally owned; process has not been
        // resumed. No process IDs or handles obtained by enumeration are used.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
            return Err(os_error("브라우저 작업 그룹 연결 실패"));
        }
        Ok(())
    }

    #[allow(unsafe_code)]
    fn resume(thread: &OwnedHandle) -> Result<(), String> {
        // SAFETY: this is the primary thread handle from our successful
        // CREATE_SUSPENDED call. Its owning process is already assigned.
        let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
        if previous == u32::MAX {
            Err(os_error("브라우저 시작 재개 실패"))
        } else if previous != 1 {
            Err("브라우저 시작 전 중지 상태가 달라졌습니다.".into())
        } else {
            Ok(())
        }
    }

    pub fn spawn_hidden_detached(executable: &Path, args: &[OsString]) -> Result<(), String> {
        let (application, mut command) = command_line(executable, args)?;
        let mut pending = spawn_suspended(&application, &mut command)?;
        resume(&pending.thread)?;
        // Once the primary thread is running, detached mode transfers no Job
        // ownership. Drop only our handles and leave the new process running.
        drop(pending.process.take());
        Ok(())
    }

    #[allow(unsafe_code)]
    fn active_processes(job: &OwnedHandle) -> Result<u32, String> {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the handle is our live non-null Job. The API writes only the
        // correctly sized initialized information buffer; no pointer escapes.
        let result = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut information as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if result == 0 {
            return Err(os_error("브라우저 작업 그룹 종료 확인 실패"));
        }
        Ok(information.ActiveProcesses)
    }

    #[allow(unsafe_code)]
    fn job_process_ids(job: &OwnedHandle) -> Result<Vec<u32>, String> {
        // Same C layout as JOBOBJECT_BASIC_PROCESS_ID_LIST, with sufficient
        // trailing storage for this bounded browser tree. An overflow is an
        // explicit error, never permission to treat an incomplete list as empty.
        #[repr(C)]
        struct ProcessIds {
            assigned: u32,
            count: u32,
            ids: [usize; 4096],
        }
        let mut information = ProcessIds {
            assigned: 0,
            count: 0,
            ids: [0; 4096],
        };
        // SAFETY: the Job is our live non-null handle. repr(C) provides the
        // documented DWORD/DWORD/ULONG_PTR array layout and alignment, and the
        // writable buffer length includes every array element. No pointer escapes.
        let result = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicProcessIdList,
                (&mut information as *mut ProcessIds).cast(),
                size_of::<ProcessIds>() as u32,
                null_mut(),
            )
        };
        if result == 0 {
            return Err(os_error("브라우저 작업 그룹 프로세스 목록 확인 실패"));
        }
        if information.count as usize > information.ids.len()
            || information.assigned != information.count
        {
            return Err("브라우저 작업 그룹 프로세스 목록이 불완전합니다.".into());
        }
        information.ids[..information.count as usize]
            .iter()
            .map(|&id| {
                u32::try_from(id).map_err(|_| "브라우저 프로세스 번호 범위 오류".to_string())
            })
            .collect()
    }

    #[allow(unsafe_code)]
    fn open_job_member(job: &OwnedHandle, id: u32) -> Result<Option<OwnedHandle>, String> {
        // SAFETY: id came only from our Job's process list. Request read-only
        // membership-query and synchronization rights, never termination rights.
        // The PID may race with exit/reuse; membership is checked on the stable
        // handle below before storing or waiting on it. No privilege is enabled.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                id,
            )
        };
        if raw.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(None); // The process disappeared before OpenProcess.
            }
            return Err(format!(
                "브라우저 자식 프로세스 대기 핸들 열기 실패: {error}"
            ));
        }
        let process = own_new_handle(raw, "브라우저 자식 프로세스 핸들 소유 실패")?;
        let mut member = 0;
        // SAFETY: both handles are live, job is explicitly our non-null Job,
        // and member is a correctly sized writable BOOL. This also rejects a
        // recycled PID belonging to another process without touching that process.
        if unsafe { IsProcessInJob(process.as_raw_handle(), job.as_raw_handle(), &mut member) } == 0
        {
            return Err(os_error("브라우저 자식 프로세스 소유 확인 실패"));
        }
        Ok((member != 0).then_some(process))
    }

    #[allow(unsafe_code)]
    fn process_signaled(process: &OwnedHandle) -> Result<bool, String> {
        // SAFETY: only owned, membership-verified process handles with
        // SYNCHRONIZE access reach this function, and remain live for the call.
        match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(os_error("브라우저 자식 프로세스 종료 대기 실패")),
        }
    }

    #[allow(unsafe_code)]
    fn root_exit_code(process: &OwnedHandle) -> Result<Option<i32>, String> {
        // SAFETY: the locally owned process handle remains valid throughout
        // both calls. Polling first distinguishes an actual exit code 259 from
        // the STILL_ACTIVE value GetExitCodeProcess returns for live processes.
        let status = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) };
        match status {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                // SAFETY: code is writable and process remains a live handle
                // to an exited process; the API writes a single DWORD.
                if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
                    Err(os_error("브라우저 종료 코드 확인 실패"))
                } else {
                    Ok(Some(code as i32))
                }
            }
            _ => Err(os_error("브라우저 종료 상태 확인 실패")),
        }
    }

    #[allow(unsafe_code)]
    fn terminate_job(job: &OwnedHandle) -> Result<(), String> {
        // SAFETY: this unnamed non-inheritable Job was created here and contains
        // only our suspended-then-assigned root and its descendants. Never pass
        // null, a parent Job, or a handle opened by name.
        if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
            return Err(os_error("브라우저 작업 그룹 종료 실패"));
        }
        Ok(())
    }

    pub struct OwnedProcess {
        job: OwnedHandle,
        process: OwnedHandle,
        members: Vec<(u32, OwnedHandle)>,
        exit_code: Option<i32>,
    }

    impl OwnedProcess {
        pub fn spawn(executable: &Path, args: &[OsString]) -> Result<Self, String> {
            let (application, mut command) = command_line(executable, args)?;
            let job = new_job()?;
            let mut pending = spawn_suspended(&application, &mut command)?;
            assign(&job, pending.process.as_ref().unwrap())?;
            // The Job becomes the termination owner before the primary thread
            // can run. A resume failure drops result and kills that same Job.
            let result = Self {
                job,
                process: pending.process.take().unwrap(),
                members: Vec::new(),
                exit_code: None,
            };
            resume(&pending.thread)?;
            Ok(result)
        }

        fn capture_members(&mut self) -> Result<(), String> {
            for id in job_process_ids(&self.job)? {
                if self.members.iter().any(|(known, _)| *known == id) {
                    continue;
                }
                if let Some(handle) = open_job_member(&self.job, id)? {
                    // Keep even signaled handles until the owner is dropped:
                    // retaining the process object also prevents PID reuse.
                    self.members.push((id, handle));
                }
            }
            Ok(())
        }

        /// Some(code) means the Job is empty, root has exited, and every captured
        /// member is signaled. An exited root with live descendants returns None.
        pub fn try_wait(&mut self) -> Result<Option<i32>, String> {
            if self.exit_code.is_some() {
                return Ok(self.exit_code);
            }
            self.capture_members()?;
            for (_, member) in &self.members {
                if !process_signaled(member)? {
                    return Ok(None);
                }
            }
            if active_processes(&self.job)? == 0 {
                self.exit_code = root_exit_code(&self.process)?;
            }
            Ok(self.exit_code)
        }

        /// Terminate only this Job, then confirm all descendants have exited.
        pub fn kill(&mut self) -> Result<(), String> {
            if self.exit_code.is_none() {
                // TerminateJobObject behaves as asynchronous TerminateProcess
                // for each member. ActiveProcesses can reach zero before the
                // process object is signaled and file handles are released.
                // Capture stable member handles BEFORE requesting termination.
                let captured = self.capture_members();
                let terminated = terminate_job(&self.job);
                // Even a capture failure must still terminate our own Job;
                // return the error rather than claim cleanup is now safe.
                captured?;
                terminated?;
            }
            self.wait()
        }

        /// Bounded wait. Profile cleanup may proceed on Ok, but must separately
        /// handle transient sharing violations: a member can naturally exit
        /// between snapshots, and file-system filters can also delay deletion.
        pub fn wait(&mut self) -> Result<(), String> {
            let deadline = Instant::now() + EXIT_TIMEOUT;
            loop {
                if self.try_wait()?.is_some() {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err("브라우저 자식 프로세스 종료 확인 시간 초과".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for OwnedProcess {
        fn drop(&mut self) {
            if self.exit_code.is_none()
                && let Err(error) = self.kill()
            {
                let _ = writeln!(std::io::stderr().lock(), "[오류] {error}");
            }
            // OwnedHandle closes the Job even if explicit termination failed.
            // KILL_ON_JOB_CLOSE remains the last safeguard; the Job handle was
            // never inherited, duplicated, or exposed outside this owner.
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            env, fs,
            os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, process::CommandExt},
            process::{Command, Stdio},
        };

        #[test]
        fn command_line_keeps_quotes_empty_backslashes_and_utf16() {
            let args: Vec<OsString> = ["", "a b", "a\"b", "C:\\끝\\", "한글 # 😀"]
                .into_iter()
                .map(Into::into)
                .collect();
            let (app, line) = command_line(Path::new(r"C:\합성 폴더\browser.exe"), &args).unwrap();
            assert_eq!(app.last(), Some(&0));
            assert_eq!(
                String::from_utf16(&line[..line.len() - 1]).unwrap(),
                "\"C:\\합성 폴더\\browser.exe\" \"\" \"a b\" \"a\\\"b\" \"C:\\끝\\\\\" \"한글 # 😀\""
            );
            let raw = OsString::from_wide(&[b'A' as u16, 0xd800, b'B' as u16]);
            let (_, line) = command_line(Path::new(r"C:\test.exe"), &[raw]).unwrap();
            assert!(line.windows(3).any(|w| w == [65, 0xd800, 66]));
            assert!(command_line(Path::new("bad\0.exe"), &[]).is_err());
            assert!(command_line(Path::new(r"C:\test.exe"), &["a\0b".into()]).is_err());
            assert!(command_line(Path::new(r"C:\test.exe"), &["a".repeat(32767).into()]).is_err());
        }

        fn worker_args(mode: &str, paths: &[&Path]) -> Vec<OsString> {
            let (_, module) = module_path!().split_once("::").unwrap();
            let mut args = vec![
                "--exact".into(),
                format!("{module}::process_worker").into(),
                "--".into(),
                mode.into(),
            ];
            args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
            args
        }

        #[test]
        fn process_worker() {
            let mut args = env::args_os().skip_while(|arg| arg != "--").skip(1);
            let Some(mode) = args.next() else { return };
            match mode.to_str().unwrap() {
                "arguments" => {
                    let output = args.next().unwrap();
                    let values: Vec<_> = args.map(|value| value.into_string().unwrap()).collect();
                    fs::write(output, serde_json::to_vec(&values).unwrap()).unwrap();
                    std::process::exit(259);
                }
                "parent" => {
                    let ready = args.next().unwrap();
                    let lock = args.next().unwrap();
                    let _child = Command::new(env::current_exe().unwrap())
                        .args(worker_args("hold", &[Path::new(&ready), Path::new(&lock)]))
                        .creation_flags(0x0800_0000)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap();
                    until(|| Path::new(&ready).is_file());
                    std::process::exit(17);
                }
                "hold" => {
                    let ready = args.next().unwrap();
                    let lock = args.next().unwrap();
                    let file = fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .share_mode(0)
                        .open(lock)
                        .unwrap();
                    fs::write(ready, "ready").unwrap();
                    std::thread::sleep(Duration::from_secs(120));
                    drop(file);
                }
                "sleep" => std::thread::sleep(Duration::from_secs(120)),
                _ => panic!("unknown synthetic worker mode"),
            }
        }
        fn until(mut condition: impl FnMut() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !condition() {
                assert!(Instant::now() < deadline, "합성 프로세스 준비 시간 초과");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn tempdir() -> tempfile::TempDir {
            tempfile::Builder::new()
                .prefix("my-system-owned-process-")
                .tempdir()
                .unwrap()
        }

        #[test]
        fn actual_child_keeps_argument_boundaries_and_exit_259() {
            let temp = tempdir();
            let output = temp.path().join("arguments.json");
            let expected = ["", "a b", "a\"b", "C:\\끝\\", "한글 # 😀"];
            let mut args = worker_args("arguments", &[&output]);
            args.extend(expected.map(OsString::from));
            let mut process = OwnedProcess::spawn(&env::current_exe().unwrap(), &args).unwrap();
            process.wait().unwrap();
            assert_eq!(process.try_wait().unwrap(), Some(259));
            let actual: Vec<String> = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
            assert_eq!(actual, expected);
            drop(process);
            temp.close().unwrap();
        }

        #[test]
        fn job_waits_for_descendant_after_root_exit_and_kills_only_its_tree() {
            let temp = tempdir();
            let ready = temp.path().join("자식 # ready");
            let lock = temp.path().join("owned-lock");
            let mut process = OwnedProcess::spawn(
                &env::current_exe().unwrap(),
                &worker_args("parent", &[&ready, &lock]),
            )
            .unwrap();
            until(|| ready.is_file() && root_exit_code(&process.process).unwrap() == Some(17));
            assert_eq!(
                process.try_wait().unwrap(),
                None,
                "살아 있는 자식은 완료가 아님"
            );
            assert!(active_processes(&process.job).unwrap() >= 1);
            assert!(
                !process.members.is_empty(),
                "자식의 종료 대기 핸들을 확보해야 함"
            );
            process.kill().unwrap();
            assert_eq!(process.try_wait().unwrap(), Some(17));
            assert_eq!(active_processes(&process.job).unwrap(), 0);
            assert!(
                process
                    .members
                    .iter()
                    .all(|(_, handle)| process_signaled(handle).unwrap())
            );
            let reopened = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock)
                .unwrap();
            drop(reopened);
            drop(process);
            temp.close().unwrap();
        }

        #[test]
        fn drop_ends_owned_process_and_missing_executable_fails_closed() {
            let temp = tempdir();
            assert!(OwnedProcess::spawn(&temp.path().join("absent.exe"), &[]).is_err());
            let ready = temp.path().join("ready");
            let lock = temp.path().join("lock");
            let process = OwnedProcess::spawn(
                &env::current_exe().unwrap(),
                &worker_args("hold", &[&ready, &lock]),
            )
            .unwrap();
            until(|| ready.is_file());
            drop(process);
            let reopened = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock)
                .unwrap();
            drop(reopened);
            temp.close().unwrap();
        }

        #[test]
        fn different_job_member_is_not_retained_waited_or_terminated() {
            let executable = env::current_exe().unwrap();
            let args = worker_args("sleep", &[]);
            let mut first = OwnedProcess::spawn(&executable, &args).unwrap();
            let mut second = OwnedProcess::spawn(&executable, &args).unwrap();
            let ids = job_process_ids(&first.job).unwrap();
            assert!(!ids.is_empty());
            for id in ids {
                assert!(open_job_member(&second.job, id).unwrap().is_none());
            }
            first.kill().unwrap();
            assert_eq!(second.try_wait().unwrap(), None);
            second.kill().unwrap();
        }
    }
}
