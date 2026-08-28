# PC-98 FAT12 Patcher Tool

사용자가 가진 원본 PC-98 FAT12 HDM에서 정해진 파일을 다시 조립한 뒤 BPS를 적용하는 로컬 웹 패처입니다. 원본 HDM과 결과 HDM은 브라우저 안에서만 처리되며 서버 업로드 API가 없습니다. 작품별 배포물은 누구나 열어 볼 수 있는 표준 ZIP 하나입니다.

전체 디스크에 곧바로 BPS를 적용하면 FAT 파일 재배치까지 패치 데이터가 떠안을 수 있습니다. 이 도구는 먼저 파일명·크기·SHA-256·배치 순서가 고정된 기준 HDM을 결정론적으로 만들고, 레시피와 그 기준에 맞춘 BPS만 ZIP으로 배포합니다.

```text
배포 ZIP
├─ recipe.json ─┐
└─ patch.bps ───┼─ 원본 검사·FAT12 재조립·BPS 적용 ── 결과 HDM
사용자 원본 HDM ┘
```

## 로컬 실행

Node.js 22.13 이상이 필요합니다.

```sh
npm ci
npm run dev
```

기본 설정에서는 패치 ZIP을 먼저 끌어 놓거나 선택해 지원 원본 조건을 읽은 다음 원본 HDM을 같은 방식으로 넣습니다. 잘못된 HDM은 ZIP을 유지한 채 다시 고를 수 있고, ZIP을 지우거나 바꾸면 HDM 선택부터 다시 확인합니다. Rust 코어를 수정했다면 `wasm-pack`을 설치한 뒤 `npm run core:build`로 추적되는 WebAssembly 산출물을 갱신합니다.

S3와 CloudFront에 올릴 정적 산출물은 `npm run build`로 `dist/site/`에 만듭니다. 작품별 ZIP을 빌드에 포함하는 방법과 캐시·MIME 타입 설정은 [S3·CloudFront 배포 문서](docs/s3-cloudfront.md)를 따릅니다.

## 한 작품용 정적 사이트 만들기

저장소 밖에 있는 패치 ZIP을 빌드 명령에 넘깁니다.

```sh
npm run build:hosted -- /path/to/game-kr-patch.zip
```

이 명령은 내용 해시가 붙은 ZIP과 그 경로를 가리키는 `patcher.json`을 `dist/site/`에만 만듭니다. 추적되는 [public/patcher.json](public/patcher.json)은 범용 로컬 입력 모드인 `package_url: null`을 유지합니다. 작품 포함 사이트에서는 방문자가 원본 HDM만 고르며, 그 파일도 서버로 전송되지 않습니다.

## 패치 작성

웹과 같은 코어를 쓰는 작성 도구가 포함되어 있습니다.

```sh
cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- create \
  recipe.json source.hdm target.hdm game-kr-patch.zip
```

작성 도구는 기존 출력 파일을 덮어쓰지 않습니다. 작성 도구가 만드는 ZIP 루트에는 `recipe.json`과 `patch.bps`만 들어가며, 작성 입력의 레시피 문장을 그대로 보존합니다. 패처는 이 두 이름만 소비하고 다른 부가 문서를 압축 해제하지 않습니다. `baseline`과 `apply` 명령도 지원하며, 잘못된 원본·ZIP·레시피·BPS·출력 해시에서는 결과를 내지 않습니다.

```sh
cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- inspect game-kr-patch.zip

cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- apply \
  source.hdm game-kr-patch.zip output.hdm
```

`inspect`는 ZIP 안의 BPS 동작별 바이트 수를 보여 주므로 재배치가 리터럴 데이터로 새지 않는지도 감사할 수 있습니다. BPS 자체는 정규 기준 HDM에서 결과 HDM으로 가는 표준 BPS1이며, 원본 HDM에 직접 적용하는 파일은 아닙니다.

## 문서

- [레시피 계약](docs/recipe.md)
- [구조와 모듈 경계](docs/architecture.md)
- [현재 검증 상태](docs/status.md)
- [S3와 CloudFront 정적 배포](docs/s3-cloudfront.md)

원본·목표 HDM과 게임 파일은 저장소에 포함하지 않습니다. 현재 범위는 같은 크기의 raw FAT12 이미지, 루트 파일 재배치, MZ 실행 파일 뒤에 붙은 LHA 멤버 추출, BPS1 적용입니다.
