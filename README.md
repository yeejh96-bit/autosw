# autosw

Windows용 Codex 플러그인입니다. 사회복지 문서의 초안·검토·한글파일 작성과 개인 인원 실적·지출 관리를 지원합니다. 소스와 설치용 카탈로그는 [GitHub](https://github.com/yeejh96-bit/autosw)에서 제공합니다.

## 빠른 설치

Windows x64에서 Codex 앱과 Git이 필요합니다. Codex 앱이 설치돼 있어도 PowerShell에서 `codex` 명령을 찾지 못할 수 있으므로, 처음 설치할 때는 아래 순서로 진행합니다.

**1. PowerShell에서 명령줄용 Codex를 설치합니다.** 이미 설치돼 있으면 업데이트합니다. [OpenAI 공식 설치 안내](https://learn.chatgpt.com/docs/codex/cli)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

**2. 설치가 끝나면 PowerShell을 닫았다가 다시 엽니다.** 새 창에서 아래 두 줄을 한 줄씩 실행합니다. 첫 번째 명령이 성공한 뒤 두 번째 명령을 실행합니다.

```powershell
codex plugin marketplace add yeejh96-bit/autosw --ref main
codex plugin add autosw@autosw-marketplace
```

**3. Codex 앱을 완전히 종료한 뒤 다시 열고** autosw의 **설정**을 실행합니다. 설정 버튼이 없으면 새 대화에서 **“autosw 처음 설정해줘”**라고 요청합니다.

첫 설정은 사용할 수 있는 Node를 확인합니다. 없거나 버전이 낮으면 공식 Node.js 배포에서 Windows x64 실행 환경을 받아 사용자 계정의 autosw 전용 위치에 준비합니다. 이어서 잠금 파일에 지정된 라이브러리를 내려받고 바탕화면에 `autosw-workspace`를 만듭니다. 처음에는 인터넷이 필요합니다. 앱이 실행 또는 다운로드 확인을 표시하면 안내에 따릅니다.

설정이 알려 준 **바탕화면의 업무 폴더**를 Codex 프로젝트로 열고 새 대화를 시작합니다. 사용 중인 autosw 폴더가 있다면 첫 설정에서 그 폴더의 전체 경로를 알려 주세요.

## 사용

- **계획서 작성** · “프로그램 계획서 초안을 작성해줘. 필요한 정보부터 물어봐줘.”
- **검토** · “drafts에 넣은 결과보고서에서 빠진 내용과 숫자 오류를 알려줘.”
- **한글 출력** · “확인한 초안으로 templates의 기관 양식을 채우고 한글파일을 만들어줘.”
- **인원·지출** · “이번 회기 인원과 교재비를 등록하고 싶어. 저장할 내용을 먼저 보여줘.”

| 업무 폴더 안 | 용도 |
|---|---|
| `templates/` | 기관 양식. 가능하면 HWPX로 준비합니다. |
| `drafts/` | 원고·검토 자료 |
| `outputs/` | 완성 문서 |
| `previews/` | 검증 PDF·미리보기 |
| `autosw.config.json` | 기관명과 문체 기본값 |
| `.autosw/` | 개인 장부와 작업 상태 |

폴더를 옮길 때는 작업을 끝내고 **업무 폴더 전체를 옮긴 뒤** 새 위치를 Codex 프로젝트로 엽니다. 새 작업은 그 프로젝트를 사용합니다. 이동 전에 만든 문서 미리보기나 검증 기록을 재개해야 한다면 새 위치에서 다시 만듭니다.

한글파일의 최종 저장·재열기·PDF 검증에는 설치된 한글 프로그램이 필요합니다. 인원·지출 등록이나 정정은 미리보기 확인과 승인 뒤 저장합니다. 익명 참여자 ID가 없는 회기는 실인원 집계가 미확정입니다. [지원 범위와 문제 해결](docs/support.md)

업무 폴더는 직원마다 분리해 사용합니다. AI에 전달한 본문과 명령 결과는 사용 중인 AI 서비스에서 처리할 수 있습니다. 실제 기관 자료와 개인정보는 공개 저장소에 올리지 않습니다.

## 업데이트와 제거

`codex plugin marketplace upgrade autosw-marketplace`로 GitHub 카탈로그를 갱신하고 Codex에서 플러그인 업데이트를 적용합니다. 필요한 경우 새 버전에서 첫 설정을 실행하고 기존 업무 폴더를 지정합니다. 플러그인 제거는 Codex의 플러그인 메뉴에서 진행합니다. 업무 폴더는 별도로 보관합니다.

이전 설치의 연결을 사용하던 경우, 새 플러그인의 업무 폴더로 기존 위치를 지정하세요. 이전 프로젝트의 autosw 연결과 이전 작성 지침을 정리할 때는 대상 항목을 확인한 뒤 제거합니다.

## 개발

Node는 실행과 개발에 사용합니다. 패키지 관리 도구는 외부 라이브러리를 설치하는 데 사용하고, autosw 자체는 GitHub 플러그인으로 배포합니다. 저장소 복사본에서 `npm ci --ignore-scripts`, `npm run check`, `npm run check:portable`를 실행할 수 있습니다. [명령행 사용법](docs/usage.md)과 [공개 전 확인](docs/release.md)을 참고하세요.

자체 코드·지침은 [MIT License](LICENSE)를 따릅니다. 포함된 외부 코드와 의존성은 [NOTICE](NOTICE)와 해당 고지를 따릅니다.
