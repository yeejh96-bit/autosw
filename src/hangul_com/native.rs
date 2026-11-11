use super::Hangul;
use crate::com_dispatch::{Apartment, Argument, Dispatch};
use std::{
    ffi::OsStr,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr::null,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
    System::{
        Com::{CLSCTX_ACTIVATE_32_BIT_SERVER, CLSCTX_LOCAL_SERVER},
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{CreateMutexW, GetCurrentProcessId},
    },
    UI::WindowsAndMessaging::GetWindowThreadProcessId,
};

pub(super) struct Session {
    // Drop the COM object before uninitializing its apartment and releasing the lease.
    object: Dispatch,
    _apartment: Apartment,
    _lease: OwnedHandle,
}

impl Session {
    pub(super) fn start(interactive: bool) -> Result<Self, String> {
        let lease = lease()?;
        let session = current_session()?;
        if !hwp_processes(session)?.is_empty() {
            return Err("한글이 이미 실행 중입니다. 한글을 닫은 뒤 다시 실행하세요.".into());
        }
        let apartment = Apartment::initialize()?;
        let create = |context| Dispatch::create("HWPFrame.HwpObject", context);
        let object = match create(CLSCTX_LOCAL_SERVER) {
            Err(error) if error.ends_with("0x80040154") => {
                create(CLSCTX_LOCAL_SERVER | CLSCTX_ACTIVATE_32_BIT_SERVER)?
            }
            result => result?,
        };
        let window = window(&object)?;
        let window_handle = window.get("WindowHandle", &[])?.integer()?;
        let pid = window_process(window_handle)?;
        // Do not perform Open/Close/Quit unless the automation object points to
        // the sole new Hwp.exe in this Windows session.
        if hwp_processes(session)? != [pid] {
            return Err("이번 자동화에서 시작한 한글 창을 확인할 수 없습니다.".into());
        }
        let session = Self {
            object,
            _apartment: apartment,
            _lease: lease,
        };
        if interactive {
            let _ = session.object.call(
                "RegisterModule",
                &[text("FilePathCheckDLL"), text("FilePathCheckerModule")],
            );
            let _ = window.put("Visible", Argument::Bool(true));
        }
        Ok(session)
    }
}

fn window(hwp: &Dispatch) -> Result<Dispatch, String> {
    let windows = hwp.get("XHwpWindows", &[])?.object()?;
    windows
        .get("Item", &[Argument::I32(0)])
        .or_else(|_| windows.call("Item", &[Argument::I32(0)]))?
        .object()
}

fn text(value: &str) -> Argument<'_> {
    Argument::Text(OsStr::new(value))
}

impl Hangul for Session {
    fn open(&mut self, path: &Path, format: &str, options: &str) -> Result<bool, String> {
        self.object
            .call(
                "Open",
                &[
                    Argument::Text(path.as_os_str()),
                    text(format),
                    text(options),
                ],
            )?
            .boolean()
    }

    fn save(&mut self, path: &Path, format: &str) -> Result<bool, String> {
        self.object
            .call(
                "SaveAs",
                &[Argument::Text(path.as_os_str()), text(format), text("")],
            )?
            .boolean()
    }

    fn clear(&mut self) -> Result<(), String> {
        self.object.call("Clear", &[Argument::I32(1)]).map(drop)
    }

    fn close_document(&mut self) -> Result<(), String> {
        self.object
            .get("XHwpDocuments", &[])
            .and_then(|documents| documents.object()?.call("Close", &[Argument::Bool(false)]))
            .map(drop)
            .or_else(|_| self.clear())
    }

    fn pages(&mut self) -> Result<i32, String> {
        self.object.get("PageCount", &[])?.integer()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Err(error) = self.object.call("Quit", &[]) {
            eprintln!("[경고] 이번 한글 인스턴스를 종료하지 못했습니다. {error}");
        }
    }
}

#[allow(unsafe_code)]
fn lease() -> Result<OwnedHandle, String> {
    let name: Vec<_> = "Local\\MySystemHancomAutomation"
        .encode_utf16()
        .chain([0])
        .collect();
    // A non-inheritable named kernel object exists for the duration of this session.
    let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    let status = unsafe { GetLastError() };
    if handle.is_null() {
        return Err(format!(
            "한글 자동화 잠금 오류: {}",
            std::io::Error::from_raw_os_error(status as i32)
        ));
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if status == ERROR_ALREADY_EXISTS {
        return Err("다른 한글 자동화가 실행 중입니다.".into());
    }
    Ok(handle)
}

#[allow(unsafe_code)]
fn process_session(pid: u32) -> Option<u32> {
    let mut session = 0;
    // ProcessIdToSessionId only writes to this local u32.
    (unsafe { ProcessIdToSessionId(pid, &mut session) } != 0).then_some(session)
}

#[allow(unsafe_code)]
fn current_session() -> Result<u32, String> {
    let session = process_session(unsafe { GetCurrentProcessId() })
        .ok_or("현재 Windows 세션을 확인할 수 없습니다.")?;
    if session == 0 {
        return Err("한글 자동화는 Windows 대화형 세션에서만 실행할 수 있습니다.".into());
    }
    Ok(session)
}

#[allow(unsafe_code)]
fn window_process(hwnd: i32) -> Result<u32, String> {
    let mut pid = 0;
    // The COM server exports HWND as a 32-bit Automation LONG.
    let owner = unsafe { GetWindowThreadProcessId(hwnd as isize as _, &mut pid) };
    if owner == 0 || pid == 0 {
        return Err("한글 자동화 창의 프로세스를 확인할 수 없습니다.".into());
    }
    Ok(pid)
}

#[allow(unsafe_code)]
fn hwp_processes(session: u32) -> Result<Vec<u32>, String> {
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    let mut result = Vec::new();
    // ToolHelp writes only to this initialized, correctly sized PROCESSENTRY32W.
    let mut has_entry = unsafe { Process32FirstW(handle.as_raw_handle(), &mut entry) };
    while has_entry != 0 {
        let length = entry
            .szExeFile
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case("hwp.exe") {
            let pid = entry.th32ProcessID;
            // A process may be exiting between the snapshot and the session query.
            // Conservatively count unclassified Hwp processes as potential collisions.
            if process_session(pid).is_none_or(|owner| owner == session) {
                result.push(pid);
            }
        }
        has_entry = unsafe { Process32NextW(handle.as_raw_handle(), &mut entry) };
    }
    let status = unsafe { GetLastError() };
    if status != ERROR_NO_MORE_FILES {
        return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
    }
    Ok(result)
}
