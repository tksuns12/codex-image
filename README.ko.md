# codex-image

[English](README.md)

`codex-image`는 설치된 Codex CLI에 이미지 생성을 맡기는 작은 CLI입니다. Codex의 내장 이미지 도구로 이미지를 생성한 뒤, 결과 파일을 지정한 출력 디렉터리로 복사하고 매니페스트를 작성합니다.

처음 사용하는 경우 이 문서를 순서대로 읽어 주세요: Codex 사전 요구 사항 확인 → `codex-image` 설치 → 생성 명령 1회 실행 → 출력 파일/stdout 확인.

이 도구는 자체 OpenAI OAuth 흐름을 구현하지 않습니다. URL로 설정하는 이미지 API 엔드포인트를 호출하지 않습니다. Codex 인증 파일을 읽거나 변경하지도 않습니다. 로그인과 이미지 생성 권한은 Codex가 직접 소유합니다.

## 사전 요구 사항: Codex CLI / Codex 확장

`codex-image generate`는 정상 동작하는 Codex 설치가 필요합니다.

- standalone Codex CLI는 현재 **macOS 전용**입니다.
- VS Code/Cursor Codex 확장으로 설치된 Codex도 지원하며 `codex-image generate`에 그대로 사용할 수 있습니다.

Codex 실행 파일은 다음 순서로 찾습니다.

1. `CODEX_IMAGE_CODEX_BIN`이 설정되어 있으면 그 값을 사용합니다.
2. `PATH`에 있는 `codex`를 사용합니다.
3. VS Code/Cursor 확장에서 흔히 쓰이는 Codex 설치 위치를 확인합니다.

Codex는 이미 로그인되어 있어야 하며, 내장 이미지 생성 도구를 사용할 수 있어야 합니다.

## 설치

권장 경로: 먼저 플랫폼에 맞는 릴리스 아티팩트 설치를 사용하세요.

### 릴리스 아티팩트로 설치

플랫폼에 맞는 설치 스크립트를 내려받아 실행하세요. 각 스크립트는 최신 GitHub Release 태그를 자동으로 확인하고, 다운로드한 아카이브를 `SHA256SUMS`로 검증한 뒤 바이너리를 설치하고 `codex-image --help`로 확인합니다.

#### Linux x86_64 / macOS x86_64 / macOS arm64

```bash
curl -fsSL https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh | sh
```

기본 설치 위치는 `${HOME}/.local/bin`입니다. `CODEX_IMAGE_INSTALL_DIR=/path/to/bin`으로 바꿀 수 있으며, 설치 디렉터리가 `PATH`에 포함되어 있어야 합니다.

#### Windows x86_64 PowerShell

```powershell
Invoke-RestMethod https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1 | Invoke-Expression
```

기본 설치 위치는 `$HOME\bin`입니다. 실행 전에 `$env:CODEX_IMAGE_INSTALL_DIR = "C:\path\to\bin"`으로 바꿀 수 있으며, 설치 디렉터리가 `PATH`에 포함되어 있어야 합니다.

### 소스에서 설치 (보조 경로)

로컬 개발/테스트처럼 현재 체크아웃을 의도적으로 설치할 때만 사용하세요.

```bash
cargo install --path . --force
codex-image --help
```

## 이미지와 매니페스트 생성

출력 디렉터리를 지정해 1회 생성을 실행합니다.

```bash
codex-image generate "도서관에서 책을 읽는 수채화풍 여우" --out ./out
```

이 단일 명령의 기대 결과:
- `./out` 아래 `image-0001.<format>` 이름의 이미지 파일 생성
- `./out` 아래 `manifest.json` 생성
- 기본 동작으로 사람이 읽기 쉬운 성공 메시지가 stdout에 출력

`manifest.json`은 항상 `--out` 디렉터리에 기록됩니다. 자동화에서 매니페스트를 stdout JSON으로 파싱해야 하면 `--output json`을 명시해 다시 실행하세요.

```bash
codex-image generate "도서관에서 책을 읽는 수채화풍 여우" --out ./out --output json
```

호출자가 파일 결과와 종료 코드만 확인하면 충분한 경우 `--quiet`로 성공 stdout 메시지를 숨길 수 있습니다.

## 첫 실행 후

첫 명령에서 `image-0001.<format>`과 `manifest.json`이 생성되었다면 quickstart는 완료입니다.
아래 내용은 에이전트 자동화, 스킬 유지보수, 바이너리 업데이트가 필요할 때만 참고하면 됩니다.

실행 흐름이 궁금하다면: `codex-image`는 `codex exec`를 호출해 Codex 내장 이미지 도구를 사용하고, 최종 JSON 응답을 읽어 출력 디렉터리로 결과를 복사합니다.

## 첫 실행 이후 참고 자료 (선택)

quickstart 이후 작업이 필요하면 아래를 사용하세요.

- 고급 운영 가이드(스킬 라이프사이클, 자동화 프롬프트, 업데이트 동작, 검증 자세): [docs/advanced-reference.md](docs/advanced-reference.md)
- 지원 도구/경로/근거의 기준 매트릭스: [docs/skill-paths.md](docs/skill-paths.md)
- 의도적으로 라이브 Codex 생성까지 확인하는 런북: [docs/uat-live-smoke.md](docs/uat-live-smoke.md)

빠른 명령 참고:

```bash
codex-image generate "도서관에서 책을 읽는 수채화풍 여우" --out ./out --quiet
codex-image generate --prompt-file ./prompt.txt --out ./out --timeout 120 --debug-diagnostics
codex-image skill install --tool codex --scope project --yes
codex-image skill update --tool codex --scope project --yes
codex-image update --dry-run --output json
codex-image update
codex-image update --version v1.2.3
```

자동화에서는 스킬 명령에 `--tool`, `--scope`를 명시하세요. 머신이 stdout을 파싱할 때는 `--output json`을 사용하고, 실제 교체 전에는 `codex-image update --dry-run --output json`으로 비변경 JSON 미리보기를 확인하세요.

상세 운영 절차(스킬 상호작용 설치, 보호 대상 처리, no-live/라이브 검증 등)는 [docs/advanced-reference.md](docs/advanced-reference.md)를 기준 문서로 사용하세요.
