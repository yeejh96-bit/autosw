//! PowerShell-compatible path ordering through Windows NLS, without lossy text conversion.
use std::path::PathBuf;

pub fn sort_paths(paths: &mut [PathBuf]) -> Result<(), String> {
    #[cfg(windows)]
    {
        native::sort_paths(paths)
    }
    #[cfg(not(windows))]
    {
        let _ = paths;
        Err("PowerShell 경로 정렬은 Windows에서만 지원합니다.".into())
    }
}

#[cfg(windows)]
mod native {
    use std::{cmp::Ordering, os::windows::ffi::OsStrExt, path::PathBuf};
    use windows_sys::Win32::Globalization::{
        CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringEx, NORM_IGNORECASE,
    };

    struct NativeString {
        units: Vec<u16>,
        length: i32,
    }
    impl NativeString {
        fn from_path(path: &std::path::Path) -> Result<Self, String> {
            let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
            let length = i32::try_from(units.len())
                .map_err(|_| "Windows 경로 정렬 길이 한도를 넘었습니다.")?;
            // Keep a valid pointer even for an empty path. The terminator is
            // excluded from the explicit length passed to CompareStringEx.
            units.push(0);
            Ok(Self { units, length })
        }
    }

    #[allow(unsafe_code)]
    fn compare(left: &NativeString, right: &NativeString) -> Result<Ordering, String> {
        // SAFETY: both immutable UTF-16 buffers remain allocated throughout the
        // call, each explicit nonnegative length was checked against i32::MAX,
        // and each buffer contains that many units plus a terminator. The API
        // only reads these buffers. A null locale means LOCALE_NAME_USER_DEFAULT;
        // optional version/reserved pointers are null and lParam is zero.
        let result = unsafe {
            CompareStringEx(
                std::ptr::null(),
                NORM_IGNORECASE,
                left.units.as_ptr(),
                left.length,
                right.units.as_ptr(),
                right.length,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        match result {
            CSTR_LESS_THAN => Ok(Ordering::Less),
            CSTR_EQUAL => Ok(Ordering::Equal),
            CSTR_GREATER_THAN => Ok(Ordering::Greater),
            _ => Err(format!(
                "Windows 경로 정렬 실패: {}",
                std::io::Error::last_os_error()
            )),
        }
    }

    pub fn sort_paths(paths: &mut [PathBuf]) -> Result<(), String> {
        let strings: Vec<_> = paths
            .iter()
            .map(|path| NativeString::from_path(path))
            .collect::<Result<_, _>>()?;
        let mut order: Vec<usize> = (0..paths.len()).collect();
        let mut scratch = vec![0; paths.len()];
        let mut width = 1;
        // A fallible stable merge sort keeps the caller's paths unchanged if
        // Windows reports an error; sort_by cannot propagate comparison errors.
        while width < order.len() {
            let mut start = 0;
            while start < order.len() {
                let middle = start.saturating_add(width).min(order.len());
                let end = middle.saturating_add(width).min(order.len());
                let (mut left, mut right, mut output) = (start, middle, start);
                while left < middle && right < end {
                    if compare(&strings[order[left]], &strings[order[right]])? != Ordering::Greater
                    {
                        scratch[output] = order[left];
                        left += 1;
                    } else {
                        scratch[output] = order[right];
                        right += 1;
                    }
                    output += 1;
                }
                for index in &order[left..middle] {
                    scratch[output] = *index;
                    output += 1;
                }
                for index in &order[right..end] {
                    scratch[output] = *index;
                    output += 1;
                }
                start = end;
            }
            std::mem::swap(&mut order, &mut scratch);
            width = width.saturating_mul(2);
        }
        let sorted: Vec<PathBuf> = order
            .into_iter()
            .map(|index| paths[index].clone())
            .collect();
        paths.clone_from_slice(&sorted);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        #[test]
        fn mixed_paths_match_stable_sort_with_windows_collation() {
            let names = [
                r"z.docx",
                r"가상 문서.docx",
                r"a\deep\둘째.docx",
                r"a\첫째.docx",
                r"B.DOCX",
                r"c\끝.docx",
                r"a-b.docx",
                r"a_b.docx",
                r"a.b.docx",
                r"a10.docx",
                r"a2.docx",
                r"가나다\한글.docx",
            ];
            let mut paths: Vec<PathBuf> = names
                .iter()
                .map(|name| PathBuf::from(format!(r"C:\합성 정렬\{name}")))
                .collect();
            let mut expected = paths.clone();
            expected.sort_by(|left, right| {
                compare(
                    &NativeString::from_path(left).unwrap(),
                    &NativeString::from_path(right).unwrap(),
                )
                .unwrap()
            });
            sort_paths(&mut paths).unwrap();
            assert_eq!(paths, expected);
        }
        #[test]
        fn equal_names_keep_input_order_and_utf16_is_not_replaced() {
            let mut paths = vec![
                PathBuf::from("b"),
                PathBuf::from("a"),
                PathBuf::from("A"),
                PathBuf::from("a"),
            ];
            sort_paths(&mut paths).unwrap();
            assert_eq!(paths, ["a", "A", "a", "b"].map(PathBuf::from));
            let path = PathBuf::from(OsString::from_wide(&[b'a' as u16, 0xd800, b'b' as u16]));
            let native = NativeString::from_path(&path).unwrap();
            assert_eq!(native.units, [b'a' as u16, 0xd800, b'b' as u16, 0]);
            assert_eq!(native.length, 3);
            let mut empty: Vec<PathBuf> = vec![];
            sort_paths(&mut empty).unwrap();
            let mut with_empty = vec![PathBuf::from("a"), PathBuf::new()];
            sort_paths(&mut with_empty).unwrap();
            assert_eq!(with_empty, [PathBuf::new(), PathBuf::from("a")]);
        }
    }
}
