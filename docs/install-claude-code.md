# Claude Code 설치·첫 사용 안내

1. Windows x64 PC에 Claude Code(데스크톱 앱 또는 명령줄)와 Git을 준비합니다.
2. Claude Code 대화창에 `/plugin marketplace add yeejh96-bit/autosw`를 입력한 뒤 `/plugin install autosw@autosw-marketplace`를 입력합니다. PowerShell에서는 `claude plugin marketplace add yeejh96-bit/autosw`와 `claude plugin install autosw@autosw-marketplace`를 차례로 실행합니다.
3. 새 대화에서 “autosw 처음 설정해줘”라고 요청하거나 `/autosw:autosw-setup`을 입력합니다. 처음 설정은 함께 설치된 실행 파일을 확인하고 바탕화면 업무 폴더를 준비합니다. 기존 autosw 업무 폴더가 있으면 그 위치를 지정합니다. PowerShell 실행 확인이 나오면 내용을 확인하고 허용합니다.
4. 설치가 알려 준 업무 폴더를 Claude Code의 작업 폴더로 열고 새 대화에서 문서 작업을 시작합니다. 문서 작업은 `/autosw:autosw-document`를 입력하지 않아도 요청 내용에 맞춰 자동으로 선택됩니다.

플러그인은 `~/.claude/plugins` 아래에 설치되고, 프로젝트의 양식·장부는 업무 폴더에 따로 남습니다. Codex에서 쓰던 업무 폴더도 그대로 열어 사용할 수 있습니다. 폴더를 통째로 옮겼다면 새 위치를 Claude Code에서 엽니다.
