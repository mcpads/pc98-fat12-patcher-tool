# 구조와 모듈 경계

브라우저 UI와 바이트 처리 코어를 분리하고, 네이티브 작성 도구와 웹 적용기가 같은 Rust 적용 함수를 호출합니다.

```text
app/page.tsx
├─ app/components/file-picker.tsx       로컬 드롭·선택 UI
├─ app/lib/patch-definition.ts          표시용 레시피 요약
└─ app/lib/patch-core.ts                 WebAssembly 경계
   └─ wasm/                              추적되는 생성 패키지

patch-core/src/lib.rs
├─ recipe.rs                             제작 계획·배포 레시피·엄격 검증
├─ fat_name.rs                           ASCII·원시 FAT/LHA 이름 계약
├─ limits.rs                             파서와 브라우저 자원 한계
├─ source_files.rs                       원본 논리 파일 선택
├─ lha_sfx.rs                            MZ 길이와 LHA 멤버 추출
├─ fat12.rs                              루트 삭제·순서 배치·구조 재검증
├─ file_patch.rs                         파일 BPS 메타데이터 결합
├─ patch_package.rs                      ZIP 작성·항목 집합 검사
└─ pipeline.rs                           제작·적용 단계 조정

retro-patch-utility
└─ bps/                                  BPS1 생성·검사·적용
```

`pipeline.rs`는 파일 수집, 변환과 HDM 조립 순서만 조정합니다. ZIP 파싱은 `patch_package.rs`, BPS 자체는 공개 유틸리티, 작품별 BPS 결합은 `file_patch.rs`, FAT12 소비는 `fat12.rs`가 맡습니다. 테스트는 구현 파일 안에 넣지 않고 모두 `_tests.rs`에 둡니다.

외부 BPS 라이브러리는 브랜치 최신값이 아니라 `patch-core/Cargo.toml`의 정확한 Git 커밋 SHA로 고정합니다. 따라서 패처 빌드는 감사한 BPS 구현을 재사용하면서 저장소 경계도 유지합니다.

## 제작 경계

작성 도구는 세 입력을 받습니다.

```text
plan.json + 정확한 source.hdm + 제작자 로컬 content.hdm
```

`content.hdm`에서는 계획에 선언된 결과 루트 파일만 읽습니다. 그 파일들로 파일별 BPS를 만든 뒤 결과 HDM은 `source.hdm`에서 새로 조립합니다. content 이미지의 다른 파일, 디렉터리, FAT 배치, 클러스터 여유 공간과 미할당 영역은 읽은 결과나 ZIP 바이트에 영향을 주지 않습니다.

작성 도구는 생성한 ZIP을 다시 검사하고 source HDM에 자체 적용합니다. 자체 적용 결과가 제작 단계의 정규 결과와 바이트 단위로 다르면 출력하지 않습니다. 기존 출력 경로도 덮어쓰지 않습니다.

## 적용 신뢰 경계

웹의 JSON 파싱은 Rust 코어가 ZIP에서 검증해 꺼낸 제목과 예상 크기를 보여 주기 위한 편의 검사입니다. 실제 권한은 Rust 코어의 다음 검사에만 있습니다.

- ZIP 전체 크기·항목 수·정확한 의미 항목 집합
- 엄격한 레시피와 고정 format 식별자
- 원본 전체 크기·SHA-256·FAT12 형상
- 원본 논리 파일별 길이·SHA-256과 LHA CRC
- 파일 BPS 메타데이터·동작 스트림·CRC32·결과 파일 SHA-256
- 조립 결과의 FAT 미러·루트 파일 집합·전체 SHA-256

ZIP의 무관한 항목은 압축 해제하지 않습니다. 어느 검사든 실패하면 결과 Blob이나 다운로드 링크를 만들지 않습니다.

서버에서 받는 것은 정적 HTML·JavaScript·WebAssembly뿐입니다. 사용자가 고른 패치 ZIP과 원본 HDM, 조립 결과는 브라우저의 `File`, `Uint8Array`, `Blob`에만 존재하며 서버 API나 업로드 경로가 없습니다.

구체적인 입력 계약과 책임 분리는 [패치 패키지 프로토콜](protocol.md)이 맡습니다.

플랫폼과 빌드 시점에 따른 바이트 차이는 합성 입력·패치 ZIP·기대 HDM을 함께 고정한 [ASCII 호환 벡터](../conformance/manifest.json)와 [원시 SFN 벡터](../conformance/raw-sfn/manifest.json)로 검사합니다. 네이티브와 WebAssembly는 같은 두 벡터를 소비하며 구현 내부 구조가 아니라 패키지와 결과 바이트의 일치를 판정합니다.
