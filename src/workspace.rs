use crate::{atomic_file, hwpx_form::read_limited};
use serde_json::{Value, json};
use std::{
    env, fs, io,
    path::{Component, Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;
pub const TEXT_LIMIT: u64 = 4 * 1024 * 1024;
pub const DOCUMENT_LIMIT: u64 = 128 * 1024 * 1024;
pub const STATE_LIMIT: u64 = 16 * 1024 * 1024;
pub const FOLDERS: [&str; 5] = ["drafts", "templates", "outputs", "previews", ".autosw"];

pub fn err(e: io::Error) -> String {
    e.to_string()
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn desktop() -> Result<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr::null_mut};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath},
    };
    let mut raw = null_mut();
    let status = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, 0, null_mut(), &mut raw) };
    let desktop = if status >= 0 && !raw.is_null() {
        let len = (0..).find(|i| unsafe { *raw.add(*i) == 0 }).unwrap();
        Some(PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(raw, len)
        })))
    } else {
        None
    };
    unsafe { CoTaskMemFree(raw.cast()) };
    desktop
        .ok_or_else(|| "바탕화면 위치를 찾지 못했습니다. --workspace로 폴더를 지정하세요.".into())
}

#[cfg(not(windows))]
fn desktop() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Documents"))
        .ok_or_else(|| "작업 폴더를 지정하세요.".into())
}

pub fn requested(workspace: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = workspace {
        if path.is_empty() {
            return Err("작업 폴더를 지정하세요.".into());
        }
        return std::path::absolute(path).map_err(err);
    }
    match env::var_os("AUTOSW_WORKSPACE") {
        Some(path) => std::path::absolute(path).map_err(err),
        None => Ok(desktop()?.join("autosw-workspace")),
    }
}

pub fn canonical(path: &Path) -> Result<PathBuf> {
    let resolved = fs::canonicalize(path).map_err(err)?;
    #[cfg(windows)]
    {
        let text = resolved
            .to_str()
            .ok_or("경로가 올바른 Unicode가 아닙니다.")?;
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(drive) = text.strip_prefix(r"\\?\") {
            if drive.as_bytes().get(1) != Some(&b':') {
                return Err("일반 드라이브 또는 공유 폴더 경로를 사용하세요.".into());
            }
            return Ok(PathBuf::from(drive));
        }
    }
    Ok(resolved)
}

pub fn package_root() -> Result<PathBuf> {
    let exe = env::current_exe().map_err(err)?;
    // The installed executable is at runtime/autosw.exe; Cargo builds are under target/.
    for dir in exe.ancestors().skip(1) {
        if dir.join("plugin.json").is_file()
            && dir.join("skills/autosw-document/SKILL.md").is_file()
        {
            return canonical(dir);
        }
    }
    Err("설치된 autosw 플러그인 폴더를 찾을 수 없습니다.".into())
}

pub fn inside(root: &Path, path: &Path) -> bool {
    // Canonical Windows paths use the spelling of the existing root. The final component
    // of a new path is checked separately, without accepting case aliases of a directory.
    path.starts_with(root)
}

fn package_separation(path: &Path) -> Result<()> {
    if let Ok(package) = package_root()
        && (inside(&package, path) || inside(path, &package))
    {
        return Err("설치 폴더와 분리된 개인 작업 폴더를 사용하세요.".into());
    }
    Ok(())
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn root(workspace: Option<&str>) -> Result<PathBuf> {
    let path = canonical(&requested(workspace)?)?;
    if !path.is_dir() {
        return Err("작업 위치는 폴더여야 합니다.".into());
    }
    package_separation(&path)?;
    Ok(path)
}

fn normal_name(name: &str, internal: bool) -> Result<()> {
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.contains([':', '/', '\\', '<', '>', '|', '?', '*', '"'])
        || name.chars().any(|c| c < ' ')
    {
        return Err("지원하지 않는 경로 표기입니다.".into());
    }
    let lower = name.to_ascii_lowercase();
    let stem = lower.split('.').next().unwrap().trim_end();
    if matches!(stem, "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
        || ["com", "lpt"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|s| {
                matches!(
                    s,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
        || matches!(lower.as_str(), ".env" | ".git" | ".local" | "node_modules")
        || lower.starts_with(".env.")
        || (!internal && name.starts_with('.'))
    {
        return Err("설정·비밀정보·내부 파일에는 접근할 수 없습니다.".into());
    }
    Ok(())
}

pub fn extension(path: &Path, extensions: &[&str]) -> bool {
    extensions.is_empty()
        || path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn checked(
    root: &Path,
    input: &str,
    output: bool,
    extensions: &[&str],
    internal: bool,
) -> Result<PathBuf> {
    let path = Path::new(input);
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .map_err(|_| "작업 폴더 밖에는 접근할 수 없습니다.")?
    } else {
        path
    };
    let mut current = root.to_path_buf();
    let mut components = relative.components().peekable();
    if components.peek().is_none() {
        return Err("파일을 지정하세요.".into());
    }
    while let Some(part) = components.next() {
        let Component::Normal(name) = part else {
            return Err("작업 폴더 밖 또는 별칭 경로에는 접근할 수 없습니다.".into());
        };
        normal_name(
            name.to_str().ok_or("올바른 파일 경로를 지정하세요.")?,
            internal,
        )?;
        current.push(name);
        let final_part = components.peek().is_none();
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if reparse(&meta)
                    || (final_part && !meta.is_file())
                    || (!final_part && !meta.is_dir())
                {
                    return Err("연결 경로나 올바르지 않은 파일·폴더는 사용할 수 없습니다.".into());
                }
                current = canonical(&current)?;
                if !inside(root, &current) {
                    return Err("작업 폴더 밖에는 접근할 수 없습니다.".into());
                }
                let actual = current
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("올바른 경로가 아닙니다.")?;
                normal_name(actual, internal)?;
            }
            Err(e) if final_part && output && e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(e)),
        }
    }
    if !extension(&current, extensions) {
        return Err("지원하지 않는 파일 확장자입니다.".into());
    }
    Ok(current)
}

pub fn path(root: &Path, input: &str, output: bool, extensions: &[&str]) -> Result<PathBuf> {
    ordinary_file(checked(root, input, output, extensions, false)?, output)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn single_link(file: &fs::File) -> bool {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;
    let mut data = MaybeUninit::uninit();
    unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), data.as_mut_ptr()) != 0
            && data.assume_init().nNumberOfLinks == 1
    }
}

#[cfg(unix)]
fn single_link(file: &fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata().is_ok_and(|meta| meta.nlink() == 1)
}

pub fn state_path(root: &Path, input: &str, output: bool) -> Result<PathBuf> {
    ordinary_file(checked(root, input, output, &[], true)?, output)
}

fn ordinary_file(path: PathBuf, output: bool) -> Result<PathBuf> {
    match fs::File::open(&path) {
        Ok(file) if !single_link(&file) => {
            Err("하드 링크로 연결된 파일은 사용할 수 없습니다.".into())
        }
        Ok(_) => Ok(path),
        Err(e) if output && e.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(e) => Err(err(e)),
    }
}

pub fn folder(root: &Path, name: &str, create: bool) -> Result<PathBuf> {
    let mut result = root.to_path_buf();
    for part in name.split('/') {
        normal_name(part, true)?;
        result.push(part);
        if create {
            match fs::create_dir(&result) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(err(e)),
            }
        }
        let metadata = fs::symlink_metadata(&result).map_err(err)?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err("작업 폴더 안의 일반 폴더를 사용하세요.".into());
        }
    }
    Ok(result)
}

pub fn text(root: &Path, input: &str, extensions: &[&str]) -> Result<String> {
    let bytes = read_limited(&path(root, input, false, extensions)?, TEXT_LIMIT)?;
    let value = std::str::from_utf8(&bytes).map_err(|_| "UTF-8 텍스트를 사용하세요.")?;
    Ok(value.trim_start_matches('\u{feff}').into())
}

pub fn init(workspace: Option<&str>) -> Result<Value> {
    let path = requested(workspace)?;
    // Resolve the nearest existing parent before creating any folders.
    let parent = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or("작업 폴더의 상위 경로를 확인하세요.")?;
    let canonical = canonical(parent)?.join(path.strip_prefix(parent).map_err(|e| e.to_string())?);
    package_separation(&canonical)?;
    fs::create_dir_all(&path).map_err(err)?;
    let real = root(path.to_str())?;
    for name in FOLDERS {
        folder(&real, name, true)?;
    }
    let config = self::path(&real, "autosw.config.json", true, &["json"])?;
    let settings = json!({"schemaVersion":1,"institution":"","writingPreferences":"","templateDirectory":"templates","outputDirectory":"outputs"});
    match atomic_file::create_new(&config, (settings.to_string() + "\n").as_bytes()) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(err(e)),
    }
    Ok(
        json!({"status":"ready","workspace":real,"configPath":config,"note":"기관명과 작성 선호만 설정하세요. 실제 양식은 templates에 직접 넣어 주세요."}),
    )
}
