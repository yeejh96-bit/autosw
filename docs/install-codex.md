# Codex 설치·첫 사용 안내

Windows PC에서 autosw를 처음 연결할 때 읽습니다. Codex 앱과 한글 프로그램은 먼저 준비하고 Codex에 로그인해 주세요. Chrome 또는 Edge도 필요합니다. npm·GitHub 계정을 만들거나 게시자용 보안키를 등록할 필요는 없습니다.

## 1. Node 준비

시작 메뉴에서 **Windows PowerShell**을 엽니다. 이미 Node가 있다면 `node --version`으로 확인하세요. **24.22.1 이상인 24.x**가 준비돼 있으면 2단계로 진행할 수 있습니다. 새 설치 권장 버전은 **24.22.2**입니다. 25·26 등 다른 계열은 현재 autosw 지원 범위 밖이므로 다른 프로그램에서 쓰는 Node를 임의로 교체하지 말고 설치 담당자에게 확인하세요.

다음 명령은 일반적인 **Windows x64 PC**용입니다. ARM64 PC는 [공식 다운로드](https://nodejs.org/en/blog/release/v24.22.2)에서 해당 설치 파일을 선택하세요. 아래 x64 명령을 그대로 실행하지 않습니다.

아래 블록을 한 번에 복사해 PowerShell에 붙여넣으세요. 공식 Node 설치 파일을 내려받아 무결성을 확인한 뒤 설치창을 엽니다. 설치창과 Windows 권한 확인은 직접 진행해 주세요. 설치 실패 시 autosw 설치로 넘어가지 않습니다.

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
        if ($nodeInstallResult.ExitCode -eq 3010) { Write-Host 'Node 설치 완료. PC를 다시 시작한 뒤 2단계를 진행하세요.' }
    } finally {
        if (Test-Path -LiteralPath $nodeInstaller) { Remove-Item -LiteralPath $nodeInstaller }
    }
}
```

설치 파일은 완료·취소 후 정리하며 PC를 자동으로 다시 시작하지 않습니다. **설치 완료 후 PowerShell과 Codex를 완전히 종료했다가 다시 여세요.** 이미 열려 있던 앱은 새 Node 경로를 찾지 못할 수 있습니다. 재부팅 안내가 나오면 재부팅한 뒤 진행하세요.

## 2. autosw 연결

새 PowerShell에서 다음을 실행해 준비 상태를 확인합니다.

```powershell
node --version
npm.cmd --version
```

Node가 지원 버전으로 표시되고 npm 버전도 나오면 다음 명령을 실행합니다.

```powershell
npx.cmd -y @yeejh96/autosw@0.1.0 setup
```

첫 실행에는 인터넷 연결이 필요합니다. 표시된 개인 작업 폴더와 연결 내용을 확인한 뒤 `Y`를 입력하세요. Codex용 MCP 설정과 작성 지침을 그 폴더에 설치합니다. 기존 설정이 있어 중단되면 덮어쓰지 말고 기존 프로젝트 연결을 확인하세요.

## 3. Codex에서 사용

설치 완료 화면에 표시된 **개인 작업 폴더를 Codex 프로젝트로 열고** 신뢰 여부를 확인합니다. 새 대화에 다음을 입력하세요.

> autosw 환경을 확인하고 작성 지침을 읽어줘.

연결이 확인되면 양식은 작업 폴더의 `templates`, 참고 원고는 `drafts`에 넣고 문서 작성·검토 또는 인원 실적·지출 관리를 요청합니다. AI가 필요한 정보를 확인하고 미리보기를 제시하면 내용을 확인한 뒤 저장·등록을 승인하세요. 생성 문서는 `outputs`에서 확인합니다.

처음에는 개인정보 없는 시험 자료로 확인하세요. 한글 문서는 제출 전에 내용과 전 페이지 배치를 확인합니다. 자동 연결 시험 통과와 직원의 실제 Codex 앱 사용 확인은 별개입니다.

## 설치가 막힐 때

- `node`나 `npx.cmd`를 찾을 수 없으면 PowerShell·Codex를 완전히 종료 후 다시 열고 2단계의 버전 확인부터 진행하세요.
- 다운로드·파일 확인·설치 권한 오류가 나면 오류 문구를 설치 담당자에게 전달하세요. 보안 설정이나 PowerShell 실행 정책을 바꾸지 않습니다.
- 연결 후 필요한 한글·브라우저가 없다고 나오면 해당 프로그램을 준비한 뒤 다시 환경 확인을 요청하세요.

## 버전 기준

이 안내의 Node 설치 파일·해시는 [24.22.2 공식 배포](https://nodejs.org/en/blog/release/v24.22.2)와 [공식 SHA256 목록](https://nodejs.org/dist/v24.22.2/SHASUMS256.txt)을 기준으로 합니다. 설치 명령은 확인한 버전으로 고정되어 있고 자동 업데이트하지 않습니다. 새 보안 패치가 나오면 호환성을 확인한 뒤 설치 파일 주소·해시·개발 기준을 함께 갱신합니다. [Node 보안 공지](https://nodejs.org/en/blog/vulnerability)를 배포 시점에 확인하세요.
