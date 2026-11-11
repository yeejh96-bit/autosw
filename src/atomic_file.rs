//! 완성된 파일만 공개한다. 신규 생성은 기존 파일을 보호하고, 교체는 완성본을 이동한다.
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn create_new(path: &Path, content: &[u8]) -> io::Result<()> {
    create_new_with(path, content, |from, to| fs::hard_link(from, to))
}

pub fn overwrite(path: &Path, content: &[u8]) -> io::Result<()> {
    create_new_with(path, content, |from, to| fs::rename(from, to))
}

fn create_new_with(
    path: &Path,
    content: &[u8],
    publish: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("저장 폴더가 없습니다."))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(parent)?;
    let result = (|| {
        temporary.write_all(content)?;
        temporary.as_file().sync_all()?;
        publish(temporary.path(), path)
    })();
    // 공개 완료 후 정리 오류로 저장 재시도를 유도하지 않는다.
    let temporary_path = temporary.path().to_owned();
    if let Err(error) = temporary.close() {
        // rename 성공 시 임시 경로는 이미 사라져 있다.
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!(
                "[경고] 저장 임시 파일 정리 실패: {} ({error})",
                temporary_path.display()
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_publishes_complete_bytes_and_leaves_no_temporary() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("기록.md");
        fs::write(&target, b"old").unwrap();
        overwrite(&target, "새 내용\r\n".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "새 내용\r\n");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn locked_replacement_preserves_original_and_cleans_temporary() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("잠긴 기록.md");
        fs::write(&target, b"old").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        assert!(overwrite(&target, b"new").is_err());
        drop(locked);
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_creation_preserves_first_file_and_cleans_temporary() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("한글 # 기록.md");
        let result = create_new_with(&target, b"second", |from, to| {
            fs::write(to, b"first")?;
            fs::hard_link(from, to)
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"first");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_publication_does_not_leave_partial_output() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("기록.md");
        let result = create_new_with(&target, b"content", |_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "합성 권한 오류",
            ))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(!target.exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
