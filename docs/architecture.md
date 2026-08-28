# 구조와 모듈 경계

브라우저 UI와 바이트 처리 코어를 분리하고, 네이티브 작성 도구와 웹 패처가 같은 Rust 함수를 호출합니다.

```text
app/page.tsx
├─ app/components/file-picker.tsx       입력 UI
├─ app/lib/patch-definition.ts          ZIP 배포 설정·표시용 레시피 요약
└─ app/lib/patch-core.ts                 WebAssembly 경계
   └─ wasm/                              추적되는 생성 패키지

patch-core/src/lib.rs
├─ recipe.rs                             입력 계약과 검증
├─ limits.rs                             파서와 브라우저 적용의 자원 한계
├─ source_files.rs                       원본 파일 선택
├─ lha_sfx.rs                            MZ 길이와 LHA 추출
├─ fat12.rs                              삭제·배치·구조 재검증
├─ bps.rs                                BPS1 작성·검사·적용
├─ patch_package.rs                      ZIP 작성·항목 검사·통합 적용
└─ pipeline.rs                           단계 순서와 해시 결합
```

`pipeline.rs`는 기준 HDM과 BPS 사이의 흐름만 조정합니다. `patch_package.rs`는 표준 ZIP의 두 고정 항목을 읽고 이 흐름에 연결하며, BPS 메타데이터를 패키지 명세로 확장하지 않습니다. FAT12, BPS, 설치 압축 파일, 배포 ZIP, 레시피 규칙은 각각의 모듈이 소유합니다. 테스트는 구현 파일 안에 넣지 않고 모두 `_tests.rs` 파일에 둡니다.

## 신뢰 경계

웹 화면의 JSON 파싱은 Rust 코어가 ZIP에서 검증해 꺼낸 레시피의 제목과 예상 크기를 보여주기 위한 편의 검사입니다. 실제 권한은 Rust 코어의 ZIP 항목 검사, 엄격한 레시피 파서와 해시 검증에만 있습니다. 화면에서 준비된 것으로 보이더라도 코어가 원본 전체 SHA-256을 확인하기 전에는 어떤 파일도 조립하지 않습니다.

ZIP의 무관한 항목은 압축 해제하지 않습니다. 필수 레시피와 BPS, 실제로 요청된 LHA 멤버만 크기 한도 안에서 읽습니다. 적용은 원본 전체 정체와 FAT12 형상, 입력 파일별 길이·SHA-256, 기준 HDM 전체 SHA-256, BPS 메타데이터·동작 스트림·CRC32, 결과 전체 SHA-256과 FAT 구조를 순서대로 확인합니다. 어느 단계든 실패하면 다운로드 결과를 만들지 않습니다. 구체적인 배포 조건은 [레시피 계약](recipe.md)이 맡습니다.

`public/patcher.json`과 패치 ZIP은 서버에서 내려받을 수 있지만 원본 HDM과 조립 결과는 `File`, `Uint8Array`, `Blob`으로 브라우저 메모리에만 존재합니다. 서버 API나 업로드 경로는 없습니다.

작성 도구 `pc98_patch_author`는 기존 경로를 덮어쓰지 않으며 임시 파일을 완전히 기록하고 동기화한 뒤 결과 이름으로 공개합니다.
