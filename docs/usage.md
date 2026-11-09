# 명령행 사용법

## 인원 실적·지출

입력 형식·집계·승인·보존 기준은 [인원·지출 관리](../skills/autosw-document/references/ledger.md)를 따릅니다. `ledger-preview`의 입력 JSON은 `change` 객체 자체입니다. `ledger-query`와 `ledger-report`는 파일을 저장하지 않고 결과를 출력합니다.

```powershell
node bin/autosw.mjs ledger-query --workspace "D:\autosw-work"
node bin/autosw.mjs ledger-preview --workspace "D:\autosw-work" --input "drafts/장부입력.json"
node bin/autosw.mjs ledger-apply --workspace "D:\autosw-work" --plan <작업ID> --confirm <확인값> --approved
node bin/autosw.mjs ledger-report --workspace "D:\autosw-work" --program <프로그램명> --from <시작일> --to <종료일>
```

`--approved`는 미리보기 내용을 실제 사용자에게 확인받은 뒤에만 지정합니다. 수정·삭제도 같은 미리보기와 승인 명령을 사용합니다. `--kind attendance` 또는 `--kind expense`로 한 종류만 조회할 수 있습니다. 조회하지 않은 항목은 0이 아닌 미조회로 표시됩니다. 반환된 `document`를 JSON 원고로 저장하면 특수문자를 유지하여 기존 `preview --kind create` 경로로 한글파일을 생성할 수 있습니다.

예시는 autosw 설치 폴더에서 실행합니다. Codex에서는 설치된 플러그인의 `scripts/autosw.ps1` 전체 경로로 호출하며, 아래의 `node bin/autosw.mjs`와 같은 하위 명령을 사용합니다. 이 진입점은 첫 설정에서 선택한 Node를 사용합니다. 업무 경로는 `--workspace`로 지정하며 그 안의 상대경로를 입력합니다. 명령의 오류와 반환 상태를 확인하고, 미지원·검증 대기를 성공으로 처리하지 않습니다.

## 환경·자료·지침 확인

```powershell
node bin/autosw.mjs doctor
node bin/autosw.mjs guide
node bin/autosw.mjs read --workspace "D:\autosw-work" --input "autosw.config.json"
node bin/autosw.mjs list --workspace "D:\autosw-work"
node bin/autosw.mjs inspect --workspace "D:\autosw-work" --input "templates/양식.hwpx"
```

`guide`는 선택 가능한 지침 목록을 제공합니다. `--topic`에는 목록에 표시된 실제 Markdown 경로를 사용합니다. 스킬 본문과 필요한 단계·문서 종류만 읽습니다.

작업 시작 시 `autosw.config.json`의 `institution`과 `writingPreferences`를 기관명·문체 기본값으로 읽습니다. 빈 값은 추측하지 않고 이번 요청과 지정 양식을 우선합니다.

양식이 HWP라면 `check-hangul`로 새 HWPX 사본과 PDF를 만든 뒤 그 파일을 읽습니다. `hwp-source-needs-review`는 원문 내용의 자동 대조가 완료되지 않은 상태이며 바로 `publish`할 수 없습니다. 사용자가 HWP 원문과 변환본을 대조한 뒤 변환 HWPX를 새 작성 절차의 입력으로 사용합니다. 변환 결과의 실제 형식과 상태를 확인하고 원본을 보존합니다.

## 새 HWPX 만들기

사용자가 확인한 원고를 UTF-8 Markdown 또는 지원하는 JSON으로 `drafts/`에 둡니다. 새 문서 생성기의 지원 요소는 [지원 범위](support.md)를 참고하세요.

```powershell
node bin/autosw.mjs preview --workspace "D:\autosw-work" --kind create --input "drafts/원고.md" --output "outputs/결과.hwpx"
```

명령이 돌려준 실제 미리보기의 본문·출력 위치를 확인합니다. 사용자 승인 범위가 확인되면 응답의 작업 ID와 확인값으로 실행합니다.

```powershell
node bin/autosw.mjs apply --workspace "D:\autosw-work" --plan <작업ID> --confirm <미리보기SHA256>
node bin/autosw.mjs verify --workspace "D:\autosw-work" --plan <작업ID>
```

`<작업ID>`와 `<미리보기SHA256>`는 응답의 값으로 바꿉니다. 확인값은 입력이 변하지 않았는지 대조하기 위한 것이며 사람의 승인을 대신하지 않습니다.

## 기존 양식 채우기

먼저 `inspect`로 문단·표·셀의 실제 위치를 확인합니다. [양식 입력 JSON 명세](../skills/autosw-document/references/form-input.md)에 따라 입력값과 위치를 대응시킵니다. `sourceSha256`, `target`, `expected`는 이번 구조 확인 결과를 그대로 사용하고, `values`는 기존 문단 수에 맞춥니다. 위치 키를 추측하지 않습니다.

```powershell
node bin/autosw.mjs guide --topic "references/form-input.md"
```

요청 JSON은 `drafts/`에 새 파일로 저장한 뒤 `preview`의 `--request`로 넘깁니다. 입력을 결정하지 못한 항목, 보존할 영역, 지원하지 않는 위치는 미리보기 전에 구분합니다.

```powershell
node bin/autosw.mjs preview --workspace "D:\autosw-work" --kind fill --input "templates/양식.hwpx" --request "drafts/입력.json" --output "outputs/결과.hwpx"
```

미리보기 뒤 실행·대조는 새 문서와 같은 `apply`·`verify` 명령을 사용합니다. 양식·입력·출력 위치가 달라졌으면 새 미리보기를 만듭니다.

## 미리보기와 실제 한글 확인

```powershell
node bin/autosw.mjs render --workspace "D:\autosw-work" --input "outputs/결과.hwpx" --output "previews/미리보기.html"
node bin/autosw.mjs check-hangul --workspace "D:\autosw-work" --input "outputs/결과.hwpx" --output "outputs/검증본.hwpx" --pdf "previews/검증.pdf" --plan <작업ID>
```

`render`는 자체 조판입니다. 복잡한 기존 양식의 자체 미리보기는 지원 여부를 확인하고 `--engine layout`을 선택할 수 있습니다. 자체 화면은 실제 한글 출력과 동일하다고 보장하지 않습니다.

`check-hangul`의 성공은 한글 변환·재열기·PDF 출력 단계의 결과입니다. 그 뒤 PDF 모든 페이지를 확인하고 재저장본과 승인 본문의 내용·숫자·대상 밖 내용을 대조해야 합니다. 한글 보안창이 뜨면 사용자가 직접 허용해야 하며 거절·시간 초과는 확인 대기로 남깁니다.

HWP 형식의 완성본이 필요한 경우 HWPX 후보를 입력으로 다음 검증을 수행합니다. `--verification`은 HWP 재열기 후 내용을 대조할 새 HWPX 사본 경로이며 생략할 수 없습니다.

```powershell
node bin/autosw.mjs check-hangul --workspace "D:\autosw-work" --input "outputs/결과.hwpx" --format HWP --output "outputs/검증본.hwp" --verification "previews/대조용.hwpx" --pdf "previews/HWP검증.pdf" --plan <작업ID>
```

확장자만 바꾸지 않고 한글에서 실제 HWP로 저장·재열기합니다. 반환된 검증 상태와 PDF를 확인한 뒤 같은 형식의 새 완성본 경로로 `publish`합니다.

## 완성본 저장

한글 검증 응답의 보고서 ID와 확인값을 사용합니다. PDF 전 페이지 검토와 재저장본 내용 대조를 실제로 마친 뒤에만 `--reviewed`를 지정합니다. 이 표시는 검토 사실의 기록이며 PDF 검토를 자동으로 대신하지 않습니다.

```powershell
node bin/autosw.mjs publish --workspace "D:\autosw-work" --report <보고서ID> --confirm <보고서SHA256> --output "outputs/완성본.hwpx" --reviewed
```

같은 문서의 수정본으로 기존 완성본을 교체할 때는 도구가 연결한 대상에 한해 `--expected <기존파일SHA256>`를 추가합니다. 작업 시작 때 확인한 기존 파일이 바뀌었으면 중단하고 최신 내용을 확인합니다. 원본 양식·참고 문서·동명 다른 문서는 교체하지 않습니다.

HWP로 검증했다면 완성본 출력도 `.hwp`로 지정합니다. 기존 HWP의 자동 수정·동일 파일 교체는 원문 대조가 제한되므로 새 파일로 저장하고 기존 파일을 보존합니다.

처음 만들 때는 새 출력 이름을 사용합니다. 이미 있는 결과를 교체하려고 보호 기능을 우회하지 않습니다. [한글 출력 절차](../skills/autosw-document/references/hangul.md)에서 승인 범위와 실패 시 처리를 확인하세요. `publish`는 로컬 완성본 저장 명령이며 GitHub·npm 게시와 관계없습니다.
