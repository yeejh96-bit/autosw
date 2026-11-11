# 공개 전 확인

autosw는 GitHub 저장소의 Codex 플러그인으로 배포합니다. 공개 자체는 사용자가 승인한 범위에서 진행합니다.

1. Rust 1.89 이상 및 Visual C++ x64 빌드 도구가 있는 개발자 PowerShell에서 아래를 실행합니다. `Cargo.toml`과 `plugin.json`의 버전을 맞춥니다.

   ```powershell
   cargo fmt --all -- --check
   cargo clippy --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
   cargo test --locked --target x86_64-pc-windows-msvc
   .\scripts\package.ps1
   .\scripts\package.ps1 -Check
   ```

2. `runtime/`의 배포용 실행 파일·확인값·라이선스와 스킬을 함께 배포합니다. `target/`, 업무 폴더, 로그, 자격 증명, 개인 설정을 포함하지 않습니다.
3. PowerShell 7에서 `./tests/plugin-install.ps1`을 실행합니다. 별도 Codex 설정과 합성 업무 폴더로 Windows PowerShell의 신규 설치·첫 설정, 이전 Git 카탈로그에서의 업데이트·장부 보존을 확인합니다. Git과 Codex CLI가 필요합니다.
4. 직원 PC의 실제 Codex 앱에서 설치·업데이트·다시 열기를 시험합니다. 한글 검증은 합성 문서의 열기·저장·재열기·PDF 전 페이지 확인과 내용 대조를 따로 기록합니다. 실행하지 않은 단계는 구분합니다.
