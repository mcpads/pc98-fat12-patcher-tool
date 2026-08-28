# 현재 검증 상태

기준일: 2026-08-28

## 확인됨

- Rust 단위 테스트 41개와 공개 네이티브 적합성 테스트 1개가 통과했습니다. 사용자 소유 원본이 필요한 통합 테스트 1개는 기본 실행에서 명시적으로 제외됩니다.
- 합성 FAT12 입력에서 기존 파일 BPS와 `empty` 입력 신규 파일 BPS를 함께 적용하고, SFN·FAT 체인·EOF 뒤 바이트를 포함한 목표 HDM을 재현했습니다.
- 공개 벡터 생성기를 별도 임시 디렉터리에서 다시 실행해 `source.hdm`, `plan.json`, `package.zip`, `target.hdm`, `manifest.json`이 저장소의 고정본과 바이트 단위로 같음을 확인했습니다.
- macOS 네이티브 코어와 새로 빌드한 WebAssembly가 같은 공개 벡터의 목표 SHA-256 `20b1ea38ef250f6719bad83fdef48a96793a5ac46fca4a4a17c734efceeb9d97`을 재현했습니다.
- 잘못된 구형·비ZIP 패키지는 웹 사용자에게 내부 JSON·ZIP 파서 상세를 노출하지 않고 패키지 오류로 반환하는 회귀 검사가 통과했습니다.
- `wasm32-unknown-unknown` 릴리스 빌드, TypeScript 검사, ESLint, Vinext 정적 프로덕션 빌드와 S3 업로드용 `dist/site/` 조립이 성공했습니다.
- 브라우저 코드는 사용자가 고른 패치 ZIP과 원본 HDM을 `arrayBuffer()`로 읽고 결과를 로컬 `Blob` 다운로드로 만들며, 업로드·작품 ZIP 자동 요청 경로가 없습니다.
- npm 의존성 감사 결과 알려진 취약점은 0건입니다.

## 아직 확인하지 않음

- Linux·Windows 네이티브 구현에서 공개 적합성 벡터가 같은 바이트를 내는지
- 현재 WebAssembly 산출물로 실제 브라우저에서 잘못된 ZIP의 오류 색상과 적용·다운로드까지 완료하는 상호작용
- 모바일 브라우저별 메모리 한계와 호환성
- 공개 호스팅 및 배포 URL
- 이번 정규 조립 규칙으로 다시 만든 작품별 실제 배포 패키지의 자체 적용과 산출물 감사
- 작품별 결과의 에뮬레이터 실행, 플레이 테스트, 사람 검수, 릴리스 승인

공개 벡터는 다른 플랫폼과 미래 구현의 차이를 탐지하는 기준입니다. 아직 실행하지 않은 플랫폼의 통과를 대신 증명하지 않으며, 정적 빌드나 벡터 재현은 작품별 런타임·사람 검수·릴리스 완료를 뜻하지 않습니다.

## 소스 주입 통합 검사

원본과 목표 경로를 저장소에 넣지 않고 다음처럼 실데이터 검사를 다시 실행할 수 있습니다.

```sh
PC98_PATCH_RECIPE=/path/to/recipe.json \
PC98_PATCH_SOURCE=/path/to/source.hdm \
PC98_PATCH_TARGET=/path/to/target.hdm \
cargo test --manifest-path patch-core/Cargo.toml \
  --test source_injected_tests -- --ignored --nocapture
```
