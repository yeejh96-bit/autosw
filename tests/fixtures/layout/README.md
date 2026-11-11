# HWPX 배치 미리보기 대조 자료

`402f770c1814fdad09b2a530930caba6fa5d6817`의 고정 JavaScript 원본을 사용한 대조 시험 30개가 통과했을 때 입력과 결과를 보관했다. 묶음 420개에 합성 입력 272,615건이 들어 있다. 일부 거부 결과는 기존 시험에서 확인한 Rust 처리 한도를 담는다.

각 `.jsonl.gz`를 풀면 한 줄에 JSON 객체 하나가 나온다. 객체의 `command`와 `request`로 배치 연산을 호출하고 `status`와 `expected` 또는 `error`로 전체 결과를 대조한다. `fonts`는 임시 Windows 글꼴 폴더를 구성하는 합성 바이트다. 설치된 글꼴과 업무 문서는 사용하지 않는다.

`src/hwpx_layout/reference_tests.rs`에서 이 자료를 읽는다. 원본 소스의 버전·출처·라이선스는 `src/hwpx_layout/data/`에 남아 있다.
