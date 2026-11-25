# 공개 전 확인

autosw는 GitHub 저장소 하나를 Codex와 Claude Code 플러그인으로 함께 배포합니다. 공개 자체는 사용자가 승인한 범위에서 진행합니다.

1. Rust 1.89 이상(MSVC, `rust-toolchain.toml`이 고정) 및 Visual C++ x64 빌드 도구가 있는 PowerShell에서 아래를 실행합니다. `Cargo.toml`, `plugin.json`, `.claude-plugin/plugin.json`의 버전을 맞춥니다. 공개할 실행 파일은 `package.ps1`(`-Check` 없이)로 먼저 빌드합니다. `check.ps1`은 서식·clippy·시험·`package.ps1 -Check`를 차례로 실행하고 첫 실패에서 멈춥니다.

   ```powershell
   .\scripts\package.ps1
   .\scripts\check.ps1
   ```

   개발 중 자동 검사: Claude Code는 `.claude/settings.json`의 끝 검사(`scripts/stop-check.ps1`)로 Rust 코드·스크립트가 바뀐 경우 `check.ps1 -Quick`(서식·clippy)을 실행합니다. GitHub는 `main`의 push·pull request마다 `.github/workflows/check.yml`로 `check.ps1`을 실행합니다. GitHub 검사는 개발 PC와 같은 Rust 버전(현재 1.89.0)으로 고정되어 있어, 개발 PC의 Rust를 올리면 그 파일의 버전도 함께 올립니다. 새 PC에서는 저장소 폴더에서 `git config core.hooksPath scripts/git-hooks`를 한 번 실행해 push 전 전체 검사를 켭니다.

2. `runtime/`의 배포용 실행 파일·확인값·라이선스와 스킬을 함께 배포합니다. `target/`, 업무 폴더, 로그, 자격 증명, 개인 설정을 포함하지 않습니다.
3. Windows PowerShell에서 `./tests/plugin-install.ps1`을 실행합니다. 별도 Codex·Claude Code 설정과 합성 업무 폴더로 양쪽의 신규 설치·첫 설정·같은 업무 폴더 사용, Codex의 이전 카탈로그 업데이트·장부 보존을 확인합니다. Git, Codex CLI, Claude Code CLI가 필요합니다.
4. `claude plugin validate .`가 `Validation passed`(경고 포함 가능)로 끝나는지 확인합니다. 공식 목록용 아이콘 경고는 이 배포 방식에 해당하지 않습니다. 루트 `CLAUDE.md`가 프로젝트 지침으로 읽히지 않는다는 경고는 개발용 파일이라 정상입니다. 그 밖의 경고는 원인을 확인합니다.
5. 직원 PC의 실제 Codex 앱과 Claude Code에서 설치·업데이트·다시 열기를 시험합니다. 한글 검증은 합성 문서의 열기·저장·재열기·PDF 전 페이지 확인과 내용 대조, 합성 HWP의 `convert` 결과를 따로 기록합니다. 실행하지 않은 단계는 구분합니다.
