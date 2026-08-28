# S3와 CloudFront에 정적 배포하기

이 문서는 패처 운영자가 서버 코드 없이 정적 사이트를 빌드해 Amazon S3와 CloudFront에 배포하는 절차를 설명합니다. 배포 대상은 `dist/site/`뿐이며, 원본 HDM·결과 HDM·작성에 사용한 목표 HDM은 포함하지 않습니다.

## 정적 사이트 빌드

의존성을 설치하고 범용 패처를 빌드합니다.

```sh
npm ci
npm run build
```

결과는 `dist/site/`에 생성됩니다. 이 모드의 `patcher.json`은 `package_url: null`을 유지하므로 방문자가 패치 ZIP과 원본 HDM을 차례로 넣습니다.

한 작품의 패치 ZIP을 사이트가 미리 제공하게 하려면 저장소 밖의 ZIP 경로를 빌드 명령에 넘깁니다.

```sh
npm run build:hosted -- /path/to/game-kr-patch.zip
```

이 명령은 입력 ZIP을 `dist/site/patch/package-<SHA-256 앞 12자리>.zip`으로 복사하고, 산출물 안의 `patcher.json`만 그 파일을 가리키도록 바꿉니다. `public/patcher.json`이나 입력 ZIP은 수정하거나 Git에 추가하지 않습니다.

정적 내보내기는 [Next.js의 `output: 'export'`](https://nextjs.org/docs/app/guides/static-exports) 방식을 사용합니다. `dist/server/`는 빌드 도중 정적 HTML을 만들기 위한 내부 산출물이므로 S3에 올리지 않습니다.

## 로컬 확인

빌드한 정적 사이트를 전용 미리보기 서버로 엽니다.

```sh
npm run preview:static -- --host 127.0.0.1 --port 4173
```

브라우저에서 `http://127.0.0.1:4173/`을 열어 다음을 확인합니다.

- 범용 빌드는 패치 ZIP 입력부터 시작하는가
- 작품 포함 빌드는 패치 제목과 지원 원본 조건을 읽는가
- 원본 HDM을 적용한 결과가 레시피의 출력 SHA-256과 일치하는가
- 내려받은 HDM의 파일명과 크기가 레시피와 일치하는가

빌드 성공이나 첫 화면 표시는 실제 패치 적용 확인을 대신하지 않습니다.

## S3와 CloudFront 구성

패처 전용 S3 버킷이나 전용 접두사를 사용합니다. 버킷의 공개 접근은 차단하고 [CloudFront Origin Access Control(OAC)](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-restricting-access-to-s3.html)로만 읽게 구성합니다. S3 Object Ownership은 `Bucket owner enforced`, OAC 서명 동작은 `Sign requests`로 둡니다. CloudFront 원본은 S3 웹사이트 주소가 아니라 일반 S3 버킷 원본을 사용합니다.

CloudFront 배포에는 다음 값을 설정합니다.

- [Default root object](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/DefaultRootObject.html): 앞에 `/`를 붙이지 않은 `index.html`
- 허용 메서드: `GET`, `HEAD`
- Viewer protocol policy: HTTP를 HTTPS로 리디렉션
- 압축: 활성화
- 기본 동작: 캐시 비활성화 또는 최소 TTL 0인 정책
- `/_next/static/*`: 장기 캐시 정책
- `/patch/*`: 내용 해시 파일명을 쓰는 작품 포함 빌드에서 장기 캐시 정책

현재 정적 빌드는 CloudFront 배포의 루트 경로에 올리는 것을 전제로 합니다. 하위 경로에 배치하려면 자산 경로 설정을 별도로 바꾸고 다시 빌드해야 합니다.

## S3 업로드

이 절차는 사이트 운영자가 정적 배포물을 올리는 작업이며, 패처 방문자가 게임 파일을 올리는 흐름과는 무관합니다. 다음 예시는 전용 버킷의 루트에 올립니다. 버킷의 다른 용도 파일을 지울 수 있으므로 기본 절차에는 `--delete`를 넣지 않습니다.

```sh
PATCHER_BUCKET='your-patcher-bucket'

aws s3 sync dist/site/ "s3://${PATCHER_BUCKET}/" \
  --cache-control 'no-cache'

aws s3 cp dist/site/_next/ "s3://${PATCHER_BUCKET}/_next/" \
  --recursive \
  --cache-control 'public,max-age=31536000,immutable'

aws s3 cp dist/site/index.html "s3://${PATCHER_BUCKET}/index.html" \
  --content-type 'text/html; charset=utf-8' \
  --cache-control 'no-cache'

aws s3 cp dist/site/patcher.json "s3://${PATCHER_BUCKET}/patcher.json" \
  --content-type 'application/json' \
  --cache-control 'no-store'

aws s3 cp dist/site/_next/static/media/ "s3://${PATCHER_BUCKET}/_next/static/media/" \
  --recursive --exclude '*' --include '*.wasm' \
  --content-type 'application/wasm' \
  --cache-control 'public,max-age=31536000,immutable'
```

작품 포함 빌드라면 ZIP의 MIME 타입도 명시합니다.

```sh
aws s3 cp dist/site/patch/ "s3://${PATCHER_BUCKET}/patch/" \
  --recursive --exclude '*' --include '*.zip' \
  --content-type 'application/zip' \
  --cache-control 'public,max-age=31536000,immutable'
```

`index.rsc`, `404.html`과 그 밖의 루트 파일은 첫 번째 `sync`에서 `no-cache`로 올라갑니다. AWS CLI나 업로드 도구가 기존 객체의 메타데이터를 유지했다면 S3 콘솔에서 `index.html`, `patcher.json`, WebAssembly와 ZIP의 `Content-Type`과 `Cache-Control`을 다시 확인합니다.

## CloudFront 갱신과 확인

기본 동작이 캐시를 사용한다면 배포 직후 루트 문서와 설정만 [무효화](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/Invalidation_Requests.html)합니다.

```sh
PATCHER_DISTRIBUTION='your-cloudfront-distribution-id'

aws cloudfront create-invalidation \
  --distribution-id "${PATCHER_DISTRIBUTION}" \
  --paths '/' '/index.html' '/patcher.json' '/404.html' '/index.rsc'
```

배포 URL에서 다음 응답을 확인합니다.

```sh
curl -I 'https://patcher.example.com/'
curl -I 'https://patcher.example.com/patcher.json'
```

개발자 도구의 네트워크 화면에서는 WebAssembly 응답이 `application/wasm`인지 확인합니다. 작품 포함 빌드의 패치 ZIP은 내용 해시가 파일명에 들어가므로 새 패치를 올릴 때 새 객체가 생깁니다. 새 `patcher.json`이 배포된 것을 확인한 뒤 예전 ZIP과 오래된 해시 자산을 나중에 정리합니다.

## 사용자가 받는 동작

범용 빌드에서는 방문자가 브라우저 파일 선택기로 로컬 패치 ZIP과 원본 HDM을 차례로 엽니다. 작품 포함 빌드에서는 브라우저가 정적 패치 ZIP을 `GET`으로 내려받으므로 방문자는 지원 원본 HDM만 고릅니다. 두 모드 모두 원본 검사, FAT12 재조립, BPS 적용과 결과 검증은 브라우저 메모리에서 실행되며 결과 HDM은 로컬 다운로드로만 만들어집니다.

패처에는 원본·결과 HDM을 받는 서버 API, 요청 본문 전송, 계정이나 서버 저장소가 없습니다. 방문자 브라우저의 네트워크 요청은 HTML·JavaScript·WebAssembly·`patcher.json`과 선택적인 공개 패치 ZIP을 받는 정적 `GET`뿐입니다. 따라서 정적 호스트의 접근 로그에도 사용자가 고른 로컬 파일의 이름이나 내용이 전달되지 않습니다.

S3에는 `dist/site/`의 정적 자산과 선택한 패치 ZIP만 올라가야 합니다. 원본 HDM, 결과 HDM, 저장 데이터와 내부 검수 캡처는 업로드하지 않습니다.
