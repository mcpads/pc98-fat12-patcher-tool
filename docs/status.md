# 현재 검증 상태

기준일: 2026-08-28

## 확인됨

- Rust 단위·통합 테스트 39개 통과. 별도 원본이 필요한 테스트 1개는 기본 실행에서 명시적으로 제외됨.
- `wasm32-unknown-unknown` 릴리스 빌드와 `wasm-bindgen` 웹 산출물 생성 성공.
- 공개 WebAssembly의 Rust 소스 위치를 일반 경로로 치환해 빌드 사용자 이름과 로컬 절대 경로가 남지 않음을 확인.
- `output: 'export'` 정적 내보내기와 S3 업로드용 `dist/site/` 조립 검사 성공.
- 작품 포함 정적 사이트를 로컬 HTTP로 제공해 `/`, `patcher.json`, 내용 해시 ZIP과 WebAssembly의 200 응답을 확인. 제공 전후 파일 목록과 해시가 같고, 내려받은 ZIP·WebAssembly가 빌드 산출물과 SHA-256 단위로 일치함.
- 브라우저 코드는 정적 설정·작품 ZIP·WebAssembly를 받는 `GET`만 사용하며, 사용자가 고른 패치 ZIP·원본 HDM과 결과 HDM을 서버로 전송하는 API나 요청 경로가 없음을 확인.
- TypeScript, ESLint, Vinext 프로덕션 빌드, Rustfmt, 모든 타깃 Clippy 경고 거부 검사 통과.
- 전체 npm 의존성 감사 0건.
- 단일 배포 ZIP의 고정 루트 이름, 결정론적 재생성, 부가 문서 비추출, 레시피와 BPS 결합, 잘못된 원본 거부를 최소 fixture로 확인.
- ZIP·레시피·BPS·LHA·FAT12의 크기·항목·작업량 한도와 BPS 복사 범위 이탈 거부를 회귀 검사로 확인. 무관한 ZIP 부가 파일은 압축 해제하지 않고 계속 허용함.
- 프로덕션 화면에서 패치 ZIP을 먼저 읽고 지원 원본 조건을 표시한 뒤에만 HDM 입력이 활성화되는 흐름, ZIP·HDM 드롭 영역, 입력 초기 문구와 적용 버튼 문구, 브라우저 경고·오류가 없음을 확인.
- 실제 브라우저에서 사용자가 ZIP·HDM 드래그앤드롭 입력 동작을 확인.
- 저장소 밖의 사용자 소유 실데이터로 기준 HDM 재조립과 네이티브·WebAssembly 패치 적용 결과가 목표와 바이트 단위로 일치함을 확인.
- 작성 도구의 `inspect`로 BPS 동작별 바이트 수와 원본 파생 데이터 포함 범위를 감사할 수 있음을 실데이터 패키지로 확인.

실데이터 검증에 사용한 원본·목표 HDM, 레시피, BPS, ZIP과 식별 해시는 공개 저장소에 포함하지 않습니다.

## 아직 확인하지 않음

- 실제 브라우저에서 파일 선택 버튼을 눌러 내려받기까지 완료하는 상호작용 검증
- 모바일 브라우저별 메모리 한계와 호환성
- 공개 호스팅 및 배포 URL
- 작품별 결과의 에뮬레이터 실행, 플레이 테스트, 사람 검수, 릴리스 승인

정적 빌드나 WebAssembly의 바이트 재현은 작품별 런타임·사람 검수·릴리스 완료를 뜻하지 않습니다.

## 소스 주입 통합 검사

원본과 목표 경로를 저장소에 넣지 않고 다음처럼 실데이터 검사를 다시 실행할 수 있습니다.

```sh
PC98_PATCH_RECIPE=/path/to/recipe.json \
PC98_PATCH_SOURCE=/path/to/source.hdm \
PC98_PATCH_TARGET=/path/to/target.hdm \
cargo test --manifest-path patch-core/Cargo.toml \
  --test source_injected_tests -- --ignored --nocapture
```
