# PC-98 FAT12 Patcher Tool

사용자가 가진 원본 PC-98 FAT12 HDM에 파일별 BPS를 적용하고 결과 디스크를 브라우저 안에서 조립하는 로컬 웹 패처입니다. 패치 ZIP과 원본·결과 HDM은 서버로 전송되지 않습니다.

작품별 배포물은 누구나 열 수 있는 ZIP 하나입니다.

```text
배포 ZIP
├─ recipe.json
└─ patches/
   ├─ GAME.COM.bps
   └─ DATA.BIN.bps

사용자 원본 HDM
  → 원본 크기·해시·FAT12 형상 검사
  → 선언된 논리 파일 또는 빈 입력 확정
  → 파일별 BPS 적용
  → 결과 HDM 결정론적 조립·해시 검사
```

전체 목표 HDM은 패키지 제작 입력이나 배포 payload가 아닙니다. 제작용 content HDM에서는 레시피가 선언한 논리 파일만 읽으므로 클러스터 여유 공간과 미할당 영역은 패치 ZIP에 들어오지 않습니다. 어떤 논리 파일을 배포 가능한 형태로 만들지는 패키지 제작자의 책임이며, 로컬 적용기는 저작권 소유권을 추정하거나 임의의 FAT 영역 정책을 강제하지 않습니다.

## 로컬 실행

Node.js 22.13 이상이 필요합니다.

```sh
npm ci
npm run dev
```

패치 ZIP을 먼저 끌어 놓거나 선택하면 지원 원본 조건을 읽고 HDM 입력을 활성화합니다. 잘못된 HDM은 ZIP을 유지한 채 다시 고를 수 있고, ZIP을 지우거나 바꾸면 HDM 선택도 초기화됩니다.

Rust 코어를 수정했다면 `wasm-pack`을 설치한 뒤 추적되는 WebAssembly 산출물을 갱신합니다.

```sh
npm run core:build
```

S3와 CloudFront에 올릴 정적 산출물은 `dist/site/`에 만듭니다.

```sh
npm run build
```

업로드 범위와 캐시·MIME 타입 설정은 [S3·CloudFront 배포 문서](docs/s3-cloudfront.md)를 따릅니다.

## 패치 작성

작성 입력인 `plan.json`은 원본 정체, 보존할 파일, 다시 놓을 파일과 각 파일의 `copy` 또는 `bps` 변환 방식을 선언합니다. `content.hdm`은 목표 논리 파일을 읽기 위한 제작자 로컬 입력일 뿐 배포물에 들어가지 않습니다.

```sh
cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- create \
  plan.json source.hdm content.hdm game-kr-patch.zip
```

작성 도구는 다음을 수행합니다.

1. 원본 HDM과 계획의 파일별 정체를 확인합니다.
2. content HDM에서 선언된 출력 파일만 읽습니다.
3. 변경 파일마다 BPS를 만들고 파일명·원본·목표 정체를 메타데이터에 결합합니다.
4. 사용자 원본을 바탕으로 정규 결과 HDM을 조립해 최종 SHA-256을 계산합니다.
5. 생성한 ZIP을 다시 열고 자체 적용한 결과가 정규 결과와 같은지 확인합니다.

기존 출력 경로는 덮어쓰지 않습니다. 검사와 로컬 적용은 다음 명령을 사용합니다.

```sh
cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- inspect game-kr-patch.zip

cargo run --release --manifest-path patch-core/Cargo.toml \
  --bin pc98_patch_author -- apply \
  source.hdm game-kr-patch.zip output.hdm
```

`inspect`는 각 파일 BPS의 크기와 동작별 바이트 수를 보여 줍니다. BPS 구현은 공개 [retro-patch-utility](https://github.com/mcpads/retro-patch-utility)의 고정 Git 커밋을 사용합니다.

## 문서

- [패치 패키지 프로토콜](docs/protocol.md)
- [공개 적합성 벡터](conformance/manifest.json)
- [구조와 모듈 경계](docs/architecture.md)
- [현재 검증 상태](docs/status.md)
- [S3와 CloudFront 정적 배포](docs/s3-cloudfront.md)

원본·content·결과 HDM, 게임 파일, 저장 데이터와 내부 검수 자료는 저장소에 포함하지 않습니다. 현재 범위는 같은 크기의 raw FAT12 이미지, 루트 파일 재배치, 빈 입력에서 신규 파일 생성, MZ 실행 파일 뒤 LHA 멤버 추출과 파일별 BPS1 적용입니다.
