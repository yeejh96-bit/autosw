# HWPX 배치 미리보기 코드

autosw에 포함해 직접 관리하는 HWPX → SVG 구현이다. Kordoc npm 패키지나 MCP를 호출하지 않는다. `engine/hwpx-render-layout.mjs`가 이 폴더의 `render/svg-render.mjs`를 호출한다.

기존 마이시스템에서 사용하던 렌더 진입점의 의존 파일을 분리했다. 최초 Kordoc 원본 버전·커밋·파일별 해시·변경 내용은 `PROVENANCE.json`에 있다. 타입 표기를 제거하고 상대 모듈 확장자를 `.mjs`로 변경한 코드이며, 별도 빌드나 원격 갱신 없이 이 파일들을 직접 관리한다. 원본 파일의 해시는 현재 편집본의 해시와 구분한다.

ZIP와 XML 처리는 범용 `jszip`·`@xmldom/xmldom` 의존성을 사용한다. CLI·MCP·OCR·한글 COM 경로는 이 폴더에 포함하지 않는다. 입력 안전 검사와 출력 SVG 검사는 autosw 호출자가 수행한다.

`LICENSE`는 원저작자의 MIT 고지다. `NOTICE`는 원본 프로젝트의 고지 전체로, 이 폴더에 없는 기능에 대한 고지도 포함한다. 이식된 `hwpx/equation.mjs`의 파생 코드에 해당하는 Apache 2.0 고지는 `THIRD_PARTY/hml-equation-parser.LICENSE`에 보존했다. 파일 내부의 출처 주석도 유지한다.

이식 코드의 출처를 자체 창작으로 표시하지 않는다. 자체 미리보기는 한글 출력과 구분하며, 실제 문서 전체 쪽·본문·표·그림과 경고를 확인한다.
