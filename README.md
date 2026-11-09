# autosw

사회복지 문서의 초안 작성부터 한글파일 생성·검증, 개인별 프로그램 인원 실적·지출 관리까지 이어 주는 로컬 도구입니다. AI용 작성 지침과 MCP 서버, 명령행 도구를 하나의 프로젝트로 제공합니다.

Windows용 초기판입니다. 소스는 [GitHub](https://github.com/yeejh96-bit/autosw), 실행 패키지는 [npm](https://www.npmjs.com/package/@yeejh96/autosw)에서 제공합니다. OpenAI 공식 플러그인 디렉터리 등록본은 아닙니다.

## 하는 일

- 계획서·결과보고서·회의록·복명서·설문지 등 문서의 필요한 정보를 확인하고 초안을 작성합니다.
- 지정한 HWPX 양식의 구조를 읽고 승인된 본문을 대응시킵니다.
- 기존 양식 채우기와 양식 없는 새 HWPX 생성을 구분합니다.
- 미리보기와 생성 내용을 대조하고, 설치된 한글로 저장·재열기·PDF 출력을 확인합니다.
- HWP가 필요하면 한글에서 실제 형식을 변환·재검증하여 새 HWP 완성본을 만듭니다.
- 문서 검토만 요청하면 누락·일관성·내부 계산을 점검하고 원본을 수정하지 않습니다.

- 프로그램·실시일·회기별 인원과 지출 항목·금액·사용일을 등록·정정·삭제하고 기간·프로그램별로 집계합니다.
- 집계 결과를 문서 원고로 연결합니다. 실제 자료 변경은 미리보기 확인·승인 뒤 실행합니다.

일정·업무일지·위키·자동 백업 기능은 포함하지 않습니다. 기존 운영 장부와 연결하거나 자료를 자동 이관하지 않습니다. 문서 작성 기준의 범위와 자동 편집 가능한 한글 구조의 범위는 다릅니다. 자세한 제한은 [지원 범위](docs/support.md)를 확인하세요.

## 필요한 환경

- Node.js 24.22.1 이상 25 미만과 npm (새 설치 권장·개발 기준: 24.22.2)
- Windows PowerShell 5.1 이상: 독립 구조 읽기와 한글 연동
- 설치된 한글 프로그램: HWP 입력 변환과 최종 한글 검증
- 설치된 Chrome 또는 Edge 등 지원 브라우저와 글꼴: 자체 미리보기
- 로컬 MCP를 연결할 수 있는 AI 앱: Codex 등. 계정·앱별 실제 연결은 직접 확인해야 합니다.

한글 프로그램과 글꼴은 포함하지 않습니다. 웹 ChatGPT에 저장소 주소만 붙여 넣어 직원 PC의 파일을 처리하는 방식은 아닙니다. 웹 연결은 별도의 지원 경로와 권한 확인이 필요합니다.

## 설치 방법 선택

**처음 쓰는 직원은 아래 설치 요청문을 Codex에 붙여넣으면 됩니다.** 직접 설치하려면 Node 준비 후 MCP 설치 명령을 실행하세요. npm·GitHub 가입, 게시자용 보안키, autosw 전용 API 키는 필요하지 않습니다. 사용하는 AI 앱의 계정과 이용 권한은 별도입니다.

| 현재 상황 | 진행할 경로 |
|---|---|
| 설치를 Codex에게 맡기고 싶음 | [복사해서 요청하기](#codex에-설치-요청하기) |
| Node가 없거나 설치 여부를 모름 | [Node 준비](#node-준비) → MCP 또는 플러그인 중 하나 선택 |
| Node가 있고 Codex 앱을 사용함 | [MCP 설치](#mcp-설치-권장) |
| GitHub에서 플러그인으로 추가하고 싶음 | [GitHub 플러그인 설치](#github-플러그인-설치) |
| 작업 폴더를 직접 정하거나 자동화된 터미널에서 설치함 | [폴더 지정 설치](#작업-폴더를-직접-지정할-때) |
| GitHub ZIP을 받았거나 소스를 수정하려고 함 | [소스 설치](#github-소스에서-설치) |
| AI 앱 없이 명령행만 쓰거나 다른 MCP 앱에 연결함 | [명령행과 수동 MCP](#명령행과-수동-mcp) |
| 이미 설치했음·설치 중 오류가 남 | [업데이트·제거](#업데이트와-제거), [문제 해결](#설치-문제-해결) |

### Codex에 설치 요청하기

Windows PC의 Codex 앱에서 아래 내용을 그대로 보내세요. 설치 파일의 권한 확인창은 직접 눌러야 합니다.

```text
https://github.com/yeejh96-bit/autosw 의 README를 읽고 이 PC에 autosw를 설치하고 Codex에 연결해줘.
Node와 npm 버전을 먼저 확인하고, Node가 없으면 README의 공식 Node 설치 명령부터 진행해줘.
다른 프로그램이 쓰는 Node나 기존 Codex 설정은 임의로 교체하지 마.
내 문서 아래 autosw-workspace를 개인 작업 폴더로 사용해줘.
비대화형 터미널에서는 setup --npm --workspace로 미리보기를 확인하고 --apply로 연결해줘.
기존 연결이 있으면 중복 설치하지 말고 현재 상태를 확인해줘.
끝나면 내가 직접 눌러야 할 창, 재시작 여부, Codex에서 열 폴더의 전체 경로를 알려줘.
```

### Node 준비

Windows PowerShell에서 아래 명령을 각각 확인합니다. **Node 24.22.1 이상인 24.x**와 npm이 표시되면 다음 절로 넘어갑니다. Node가 이미 다른 버전으로 설치되어 있으면 다른 프로그램에 영향을 줄 수 있으므로 임의로 교체하지 마세요. 이 안내의 새 설치 기준은 **24.22.2**입니다.

```powershell
node --version
npm.cmd --version
```

Node가 없다면 **Windows x64 PC**에서 다음 블록 전체를 실행하세요. 공식 MSI를 내려받아 SHA256을 대조한 뒤 설치창을 엽니다. 설치창과 Windows 권한 확인은 직접 진행합니다.

```powershell
& {
    $ErrorActionPreference = 'Stop'
    $nodeInstaller = Join-Path ([IO.Path]::GetTempPath()) ('autosw-node-' + [guid]::NewGuid().ToString('N') + '.msi')
    try {
        Invoke-WebRequest 'https://nodejs.org/dist/v24.22.2/node-v24.22.2-x64.msi' -OutFile $nodeInstaller -UseBasicParsing
        $expectedHash = '9231bf64d19bef391dd5ee8af65411d3799c3c44a842d2320849cc1da60b6b6d'
        if ((Get-FileHash -LiteralPath $nodeInstaller -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Node 설치 파일 확인에 실패했습니다. 설치를 중단합니다.' }
        $nodeInstallResult = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', ('"{0}"' -f $nodeInstaller), '/norestart') -Wait -PassThru
        if ($nodeInstallResult.ExitCode -notin @(0, 3010)) { throw "Node 설치가 완료되지 않았습니다. 종료 코드: $($nodeInstallResult.ExitCode)" }
        if ($nodeInstallResult.ExitCode -eq 3010) { Write-Host 'Node 설치 완료. PC를 다시 시작한 뒤 autosw 설치를 진행하세요.' }
    } finally {
        if (Test-Path -LiteralPath $nodeInstaller) { Remove-Item -LiteralPath $nodeInstaller }
    }
}
```

완료·취소 뒤 내려받은 설치 파일을 정리하며 PC를 자동 재부팅하지 않습니다. 실패하면 다음 설치 단계로 넘어가지 마세요. **성공 후 PowerShell과 Codex를 완전히 종료하고 다시 여세요.** 재부팅 안내가 나왔으면 재부팅한 뒤 버전을 다시 확인합니다. Node 설치와 autosw 연결은 이 재시작을 사이에 두고 진행합니다. `npx ... setup` 자체가 Node를 설치하는 것은 아닙니다.

ARM64는 [공식 배포 페이지](https://nodejs.org/en/blog/release/v24.22.2)에서 맞는 설치 파일을 선택하세요. ARM64에서 autosw 전체 기능은 검증하지 않았습니다. 관리자 권한이 막혀 있거나 다른 Node 계열을 유지해야 하면 설치 담당자에게 별도 실행 환경 구성을 요청하세요. 실행 정책·보안 설정을 풀어서 진행하지 않습니다.

설치 파일과 해시는 [공식 SHA256 목록](https://nodejs.org/dist/v24.22.2/SHASUMS256.txt)을 기준으로 고정했습니다. 보안 패치 검토 시 [Node 보안 공지](https://nodejs.org/en/blog/vulnerability)를 확인하고 주소·해시·지원 버전을 함께 갱신합니다. 설치된 Node를 자동 업데이트하지 않습니다.

### MCP 설치 (권장)

지원 버전의 Node가 준비된 새 PowerShell에서 실행합니다. 처음 내려받을 때 인터넷 연결이 필요합니다.

```powershell
npx.cmd -y @yeejh96/autosw@0.1.1 setup
```

표시된 개인 작업 폴더와 연결 내용을 확인하고 **Y**를 입력합니다. 기본 위치는 사용자 홈 아래 `Documents\autosw-workspace`입니다. Codex용 MCP 설정과 작성 지침을 그 폴더에 설치하며 전역 설정은 바꾸지 않습니다. 기존 설정이나 스킬이 있으면 보존하고 중단합니다.

설치 결과의 **개인 작업 폴더를 Codex 프로젝트로 열고 신뢰 여부를 확인**한 뒤 [첫 사용](#설치-후-첫-사용)으로 진행합니다. 설정의 실행 버전은 고정되므로 새 버전이 나와도 자동으로 바뀌지 않습니다.

### 작업 폴더를 직접 지정할 때

아래 경로를 원하는 개인 폴더로 바꿉니다. 첫 명령은 미리보기만 출력하며 폴더나 설정을 만들지 않습니다. Codex가 실행하는 비대화형 터미널에서도 이 경로를 사용합니다.

```powershell
$autoswWorkspace = Join-Path $env:USERPROFILE 'Documents\autosw-workspace'
npx.cmd -y @yeejh96/autosw@0.1.1 setup --npm --workspace "$autoswWorkspace"
```

표시된 내용으로 설치하려면 같은 PowerShell에서 실행합니다.

```powershell
npx.cmd -y @yeejh96/autosw@0.1.1 setup --npm --workspace "$autoswWorkspace" --apply
```

폴더 경로에는 따옴표를 유지하세요. 기존 프로젝트 설정이 있으면 아래 [기존 설치 업데이트](#업데이트와-제거)를 따릅니다. 서로 다른 직원의 자료는 각자의 PC·Windows 사용자·개인 작업 폴더로 분리합니다.

### GitHub 플러그인 설치

Kordoc처럼 GitHub 저장소를 플러그인 설치 원본으로 추가하는 경로입니다. 여기서는 **Codex용 명령**을 사용합니다. Kordoc README의 `/plugin ...` 명령은 Claude Code용이므로 Codex PowerShell에 그대로 붙여넣지 않습니다. [Codex 공식 플러그인 안내](https://developers.openai.com/plugins/build/plugins)

Node 준비를 마친 뒤 PowerShell에서 실행합니다. 이 경로는 **Codex CLI의 `plugin` 명령과 Git**이 필요합니다. `codex` 또는 `git` 명령이 없으면 위 MCP 설치를 사용하거나 아래 요청문을 Codex 앱에 보내세요.

카탈로그 등록·설치·활성 상태는 Windows의 Codex CLI 0.159.2 임시 프로필에서 확인했습니다. 직원의 앱 화면과 실제 한글 출력은 설치 후 별도로 확인합니다.

```powershell
npx.cmd -y @yeejh96/autosw@0.1.1 init
codex plugin marketplace add yeejh96-bit/autosw --ref main
codex plugin add autosw@autosw-marketplace
```

첫 명령은 사용자 홈의 `Documents\autosw-workspace`를 준비하고 실행 패키지를 캐시합니다. 다음 명령은 GitHub 카탈로그를 등록하고 플러그인을 설치합니다. 각 명령이 성공한 뒤 다음 명령으로 진행하세요. Codex 앱을 완전히 종료했다가 다시 열고 해당 개인 작업 폴더에서 새 대화를 시작합니다. 플러그인 목록의 autosw 활성 여부와 실제 도구 호출을 확인하세요.

```text
https://github.com/yeejh96-bit/autosw 의 README를 읽고 GitHub 플러그인 방식으로 설치해줘.
Node와 Codex의 플러그인 설치 명령 지원 여부를 확인하고 개인 작업 폴더를 먼저 준비해줘.
기존 autosw MCP 연결이 있으면 중복 연결하지 말고 전환할 내용을 보여줘.
끝나면 앱 재시작 방법과 개인 작업 폴더의 전체 경로를 알려줘.
```

플러그인은 **기본 개인 폴더만** 사용하며 다른 작업 폴더를 열어도 자동 전환하지 않습니다. 맞춤 경로·여러 업무 폴더는 위 MCP 설치를 사용하세요. 플러그인 경로에서는 `setup`을 추가로 실행하지 않습니다. 이미 MCP 방식으로 설치했다면 기존 연결을 확인하고 하나만 활성화하세요. npm 실행 패키지와 GitHub의 지침을 함께 쓰므로 처음 설치·캐시 삭제 후에는 인터넷이 필요합니다.

공식 공개 디렉터리 등록과 GitHub 카탈로그 배포는 별개입니다. autosw는 GitHub 카탈로그로 제공하며 OpenAI 공식 디렉터리 등록본이 아닙니다. Claude Code용 플러그인 카탈로그나 Cursor·VS Code 설치 버튼은 이번 배포에 포함하지 않습니다. 해당 앱에서는 수동 MCP 지원을 먼저 확인하세요.

### GitHub 소스에서 설치

소스 ZIP은 설치 실행 파일이 아닙니다. Node와 의존성을 별도로 준비해야 합니다. 다음 중 **하나**로 소스를 받습니다.

- Git이 있으면 새 설치 폴더를 만들 위치에서 아래 명령을 실행합니다. 기존 `autosw` 폴더가 있으면 다른 위치를 고르세요.
- Git이 없으면 [GitHub 저장소](https://github.com/yeejh96-bit/autosw)의 **Code → Download ZIP**으로 받고 압축을 풉니다. PowerShell에서 압축을 푼 폴더 중 `package.json`이 있는 폴더를 엽니다.

```powershell
git clone https://github.com/yeejh96-bit/autosw.git
Set-Location -LiteralPath '.\autosw'
```

그 폴더 안에서 설치합니다. 명령이 실패하면 다음 단계로 넘어가지 마세요.

```powershell
powershell.exe -NoProfile -File scripts/install.ps1
$autoswWorkspace = Join-Path $env:USERPROFILE 'Documents\autosw-workspace'
powershell.exe -NoProfile -File scripts/autosw.ps1 init --workspace "$autoswWorkspace"
powershell.exe -NoProfile -File scripts/autosw.ps1 doctor --workspace "$autoswWorkspace"
powershell.exe -NoProfile -File scripts/autosw.ps1 setup --workspace "$autoswWorkspace"
```

미리보기를 확인한 뒤 연결합니다.

```powershell
powershell.exe -NoProfile -File scripts/autosw.ps1 setup --workspace "$autoswWorkspace" --apply
```

업무 폴더는 소스 폴더 밖에 두고, 소스 폴더를 업무 폴더 안에 넣지도 마세요. 이 방식은 Node와 소스의 실제 경로를 연결하므로 설치 후 옮기면 설정을 다시 확인해야 합니다. 전용 진입점은 소스 안 `.local/node/node.exe`가 있으면 먼저 사용하고 없으면 설치된 Node를 확인합니다. Node를 자동 다운로드하지 않습니다. Node가 PATH에 준비된 개발자는 `npm.cmd ci`, `node bin/autosw.mjs ...`로 같은 명령을 실행할 수도 있습니다.

### 명령행과 수동 MCP

**명령행만 사용:** MCP나 플러그인 설치 없이 작업 폴더를 준비하고 진단·지침 조회를 실행할 수 있습니다.

```powershell
$autoswWorkspace = Join-Path $env:USERPROFILE 'Documents\autosw-workspace'
npx.cmd -y @yeejh96/autosw@0.1.1 init --workspace "$autoswWorkspace"
npx.cmd -y @yeejh96/autosw@0.1.1 doctor --workspace "$autoswWorkspace"
npx.cmd -y @yeejh96/autosw@0.1.1 guide
```

문서 생성·인원 및 지출 입력 명령은 [명령행 사용법](docs/usage.md)을 따릅니다. `publish`는 검토한 문서를 로컬 완성본으로 저장하는 명령입니다. GitHub나 npm에 업무 자료를 올리지 않습니다. JavaScript 라이브러리의 공개 API는 제공하지 않습니다.

**다른 AI 앱·기존 Codex 설정에 수동 연결:** 위 `init`으로 작업 폴더부터 준비한 뒤 앱이 지원하는 로컬 STDIO MCP 형식에 아래 값을 넣습니다. `AUTOSW_WORKSPACE`는 실제 전체 경로로 바꿉니다. JSON의 역슬래시는 두 번 씁니다.

```json
{
  "mcpServers": {
    "autosw": {
      "command": "cmd.exe",
      "args": ["/d", "/c", "npx", "--yes", "@yeejh96/autosw@0.1.1", "mcp"],
      "env": {"AUTOSW_WORKSPACE": "D:\\autosw-work"}
    }
  }
}
```

이 JSON은 공통 연결 예시이며 앱별 설정 파일 위치·형식은 다릅니다. **Codex의 `config.toml`에 JSON을 붙여넣지 마세요.** Codex의 기존 프로젝트 설정에는 아래 TOML에서 폴더 경로를 바꾸고 autosw 항목만 병합합니다. 같은 이름의 항목을 두 번 만들지 않고 다른 서버 설정은 유지합니다. `setup --npm`의 미리보기는 경로를 보여 주며 TOML 본문은 출력하지 않습니다.

```toml
[mcp_servers.autosw]
command = "cmd.exe"
args = ["/d", "/c", "npx", "--yes", "@yeejh96/autosw@0.1.1", "mcp"]
startup_timeout_sec = 120
tool_timeout_sec = 180

[mcp_servers.autosw.env]
AUTOSW_WORKSPACE = 'D:\autosw-work'
```

앱 재시작 후 환경 진단과 `guide` 조회가 실제 호출되는지 확인하세요. 다른 앱 자동 탐지·설정과 앱별 설치 버튼은 제공하지 않으며, 다른 앱의 실제 작동은 별도 확인이 필요합니다.

## 설치 후 첫 사용

Codex에서 **설치 결과에 나온 개인 작업 폴더**를 열고 새 대화에 붙여넣습니다.

```text
autosw 환경을 확인하고 작성 지침을 읽어줘.
MCP 도구를 실제로 호출해 연결 여부와 현재 작업 폴더를 확인해줘.
누락된 프로그램이나 설정이 있으면 알려줘.
```

연결 후에는 다음처럼 요청합니다. 파일은 먼저 개인 작업 폴더에 복사하고 실제 파일명을 알려주세요.

```text
프로그램 계획서 초안을 작성해줘. 필요한 정보부터 물어봐줘.
```

```text
drafts에 넣은 결과보고서를 검토해줘. 빠진 내용과 숫자 오류만 알려줘.
```

```text
확인한 초안으로 templates에 넣은 기관 양식을 사용해 한글파일을 만들어줘.
```

```text
이번 회기 인원과 지출을 등록하려고 해. 필요한 항목을 알려주고 저장 전에 내용을 보여줘.
```

초안과 미리보기를 확인하고 저장·등록을 요청합니다. 처음에는 개인정보 없는 시험 자료로 확인하세요. 프로그램의 진단 성공과 직원 PC의 실제 Codex 연결·한글 출력 확인은 별개입니다. 한글 제출 문서는 본문과 전 페이지 배치를 확인해야 합니다.

## 개인 작업 폴더

작업 폴더에는 다음 내용이 생성됩니다.

| 위치 | 용도 |
|---|---|
| `autosw.config.json` | 작업 폴더 설정 |
| `drafts/` | 사용자 원고와 승인된 입력 |
| `templates/` | 사용자가 제공한 기관 양식 |
| `outputs/` | 생성한 문서 |
| `previews/` | 요청한 미리보기·검증 PDF |
| `.autosw/` | 작업 상태·미리보기, 승인 저장 후 생성되는 개인 인원·지출 장부와 변경 이력 |
| `.codex/`·`.agents/skills/` | 사용자가 연결 설정을 적용한 경우의 프로젝트 전용 MCP·스킬 |

기관 양식과 실제 문서는 공개 저장소에 넣지 마세요. 업데이트는 프로그램을 교체하며, 별도 작업 폴더의 자료는 유지하는 방식입니다.

`autosw.config.json`의 `institution`에는 기관명, `writingPreferences`에는 문체 선호를 입력할 수 있습니다. AI는 새 작업에서 이 값을 읽고 기본값으로 사용하며, 이번 요청과 지정 양식이 우선합니다. 설정만으로 과거 실적이나 현재 담당자 정보를 추정하지 않습니다.

## 문서 작성과 한글 출력

AI는 [문서 작성 스킬](skills/autosw-document/SKILL.md)을 따라 필요한 정보를 확인하고 초안을 제시합니다. 내용을 확인한 뒤 한글파일 생성을 요청합니다. MCP만 연결한 환경에서도 `guide` 도구로 같은 작성 지침을 읽을 수 있습니다.

직접 시험하려면 [명령행 사용법](docs/usage.md)을 따릅니다. [합성 원고](examples/draft.md)는 실제 업무 사실이 없는 시험용 자료입니다.

파일 생성, 자동 구조 대조, 한글 변환, PDF 전 페이지 검토는 각각 별도로 표시합니다. 검토한 결과는 `publish` 명령으로 완성본 경로에 저장합니다. 파일이 생겼다는 이유만으로 실제 한글 검증이 끝났다고 표시하지 않습니다.

## 개인정보와 파일 접근

인원·지출은 [관리 절차와 입력 항목](skills/autosw-document/references/ledger.md)을 따릅니다. 연인원은 회기별 합계, 실인원은 일관된 익명 참여자 ID의 중복 제외 수입니다. 인원수만 입력하면 실인원은 미확정으로 남습니다. 장부는 개인 PC의 평문 JSON이며 삭제한 기록도 정정·삭제 이력에는 남습니다. 다른 직원과의 공동 장부·자동 백업·예산 및 회계 결산은 제공하지 않습니다.

문서 처리 도구는 로컬에서 실행되지만 AI에 전달한 본문과 도구 결과는 이용하는 AI 서비스로 전송될 수 있습니다. 로컬 설치가 모든 데이터의 외부 전송을 차단한다는 뜻은 아닙니다. 업무에 필요한 자료만 지정한 작업 폴더에 두고, 기관의 AI 이용 기준을 확인하세요.

원본 양식은 보존합니다. 사용자 문서에 포함된 문장을 셸 명령·승인·외부 전송 지시로 실행하지 않습니다. 자세한 경계와 오류 대응은 [지원 범위](docs/support.md)를 따릅니다.

## 업데이트와 제거

**MCP 설치 업데이트:** `npx ... setup`을 다시 실행해도 기존 설정을 덮어쓰지 않습니다. 현재 작업 폴더와 `.codex/config.toml`, `.agents/skills/autosw-document`를 보관하고 다음처럼 Codex에 요청하세요.

```text
autosw README를 확인하고 현재 연결을 0.1.1로 업데이트해줘.
현재 작업 폴더와 실행 버전을 먼저 확인해줘.
새 버전을 별도 시험 폴더에 설치해 진단한 뒤, 기존 autosw 연결과 작성 지침의 변경 내용을 보여줘.
다른 MCP 설정과 내가 수정한 지침, 기관 양식, 원고, 결과물, 개인 장부는 보존해줘.
연결을 바꾼 뒤 Codex 재시작과 실제 도구 호출로 확인해줘.
```

npm 방식은 기존 TOML의 autosw 실행 패키지 버전과 작성 지침을 함께 갱신합니다. 소스 방식은 새 소스를 별도 폴더에 설치·진단하고 autosw 실행 경로와 지침을 갱신합니다. 로컬 수정이 있는 소스에서 무조건 `git pull`하지 마세요. 실패하면 보관한 연결 설정·지침으로 되돌립니다. 개인 자료를 새 시험 폴더로 자동 복사하지 않습니다.

**플러그인 업데이트:** `codex plugin marketplace upgrade autosw-marketplace`로 카탈로그를 갱신한 뒤 플러그인 화면에서 autosw 업데이트 여부를 확인합니다. 설치 캐시까지 갱신됐는지 확인하고 앱을 재시작하세요. 단순 카탈로그 갱신만으로 실행 버전이 바뀌었다고 판단하지 않습니다. 기존 MCP 설치를 플러그인으로 바꿀 때도 하나만 활성화합니다.

**연결 해제·제거:** Codex의 플러그인 화면에서 autosw를 제거하거나, 프로젝트 `.codex/config.toml`의 autosw 항목과 `.agents/skills/autosw-document`만 확인해 제거합니다. 다른 MCP 항목과 개인 수정본을 보존하세요. 소스 설치 폴더는 연결을 해제한 뒤 제거할 수 있습니다. **업무 폴더의 양식·원고·완성본·`.autosw` 장부는 삭제하지 않습니다.** npx 방식은 전역 npm 설치가 아니므로 `npm uninstall -g`로 연결이 해제되지 않습니다.

## 설치 문제 해결

| 증상·환경 | 할 일 |
|---|---|
| `node`, `npx.cmd`를 찾을 수 없음 | Node 설치 완료 여부 확인 → PowerShell·Codex 완전 종료 후 재실행 → 버전 확인 |
| `npm.ps1`·`npx.ps1` 실행 정책 오류 | 안내의 `npm.cmd`·`npx.cmd` 사용. 실행 정책을 풀지 않음 |
| Node 버전 오류 | 지원 범위 24.22.1 이상 25 미만 확인. 다른 앱의 Node는 임의 교체하지 않음 |
| MSI 권한·해시·다운로드 오류 | 다음 단계로 넘어가지 않고 오류 문구를 설치 담당자에게 전달 |
| `setup`이 미리보기만 보여 줌 | 비대화형 터미널임. 폴더 지정 `setup --npm --workspace` 미리보기 확인 후 `--apply` 사용 |
| 기존 설정이 있다고 중단 | 해당 개인 폴더의 기존 연결 확인. 새 폴더를 선택하거나 autosw 부분만 검토해 갱신 |
| 작업 폴더가 없다는 오류 | CLI·플러그인은 `init` 먼저 실행. 수동 MCP의 실제 경로 확인 |
| 명령은 성공했는데 Codex 도구가 안 보임 | 설치 결과의 폴더를 프로젝트로 열었는지·신뢰 여부·앱 재시작 확인. MCP와 플러그인 중복 여부 확인 |
| `codex plugin`·`git` 명령이 없음 | MCP 설치 사용. 앱이 설치됐다고 별도 CLI 명령까지 항상 있는 것은 아님 |
| 첫 실행 시간 초과·npm 접속 실패 | 인터넷·기관 네트워크 정책 확인. `init` 또는 `doctor`를 터미널에서 실행해 내려받은 뒤 앱 재시작. 보안 정책 우회 금지 |
| 한글·브라우저 누락 | `doctor` 안내 확인. 초안·장부와 실제 한글 출력의 준비 상태를 구분 |
| 한글 확인창에서 멈춤 | 사용자가 허용 여부를 직접 결정. 일괄 자동 허용·다른 한글 프로세스 강제 종료 금지 |
| PC가 여러 대이거나 재설치함 | PC마다 환경·연결을 준비. 업무 자료는 자동 동기화되지 않으므로 사용자 승인 아래 별도 복원 |
| macOS·Linux·WSL·웹 ChatGPT·모바일 | 이 Windows 한글 연동 초기판의 직접 지원 경로가 아님. Windows 로컬 Codex 사용 |
| 완전한 폐쇄망·인터넷 없는 새 PC | 현재 온라인 npm·GitHub 설치 안내를 그대로 사용할 수 없음. 별도 오프라인 배포 묶음은 제공하지 않음 |

기능별 제한·오류 상태는 [지원 범위](docs/support.md)에 있습니다. 오류 공유 시 실제 문서·개인정보·토큰은 제외하세요.

## 개발과 배포 준비

설치 안내는 [Kordoc README](https://github.com/chrisryugj/kordoc#설치)의 MCP·플러그인·명령행 분리와 빠른 시작 구성을 참고했습니다. Kordoc의 다중 앱 자동 연결·지원 형식·플랫폼 범위를 autosw의 기능으로 간주하지 않습니다.

```powershell
npm run check
```

개발 기준은 [AGENTS.md](AGENTS.md), 구조와 출처는 [프로젝트 구조](docs/architecture.md), 공개 전 확인은 [배포 확인](docs/release.md)에 있습니다. 자동 시험 통과는 실제 직원 환경의 한글 출력 검증을 대신하지 않습니다.

자체 코드·지침은 [MIT License](LICENSE)를 따릅니다. 포함된 외부 코드·의존성·사용자 자료는 [NOTICE](NOTICE)와 각 고지를 확인하세요. 공개용 패키지의 npm 대상은 기본 공개 레지스트리로 고정하며, 실제 게시는 명시적으로 승인된 배포 작업에서만 수행합니다.
