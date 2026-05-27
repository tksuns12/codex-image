#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PHASE="guard"
install_root=""
out_dir=""
generate_stdout=""
generate_stderr=""

log_phase() {
  printf '[uat-live-smoke] phase=%s %s\n' "$PHASE" "$1"
}

redact_cli_stderr() {
  local stderr_path="$1"
  if [[ -f "$stderr_path" ]]; then
    while IFS= read -r line; do
      printf '[uat-live-smoke] cli-stderr-redacted %s\n' "$line" >&2
    done < "$stderr_path"
  fi
}

cleanup() {
  local rc=$?

  for file_path in "$generate_stdout" "$generate_stderr"; do
    if [[ -n "$file_path" && -f "$file_path" ]]; then
      rm -f "$file_path"
    fi
  done

  for dir_path in "$install_root" "$out_dir"; do
    if [[ -n "$dir_path" && -d "$dir_path" ]]; then
      rm -rf "$dir_path"
    fi
  done

  if [[ $rc -eq 0 ]]; then
    printf '[uat-live-smoke] phase=cleanup removed temp roots\n'
  else
    printf '[uat-live-smoke] ERROR phase=%s exit_code=%d\n' "$PHASE" "$rc" >&2
  fi

  exit "$rc"
}
trap cleanup EXIT

if [[ "${CODEX_IMAGE_RUN_LIVE:-}" != "1" ]]; then
  echo '[uat-live-smoke] ERROR phase=guard refusing live UAT; set CODEX_IMAGE_RUN_LIVE=1 to opt in' >&2
  exit 1
fi

PHASE="temp-filesystem"
install_root="$(mktemp -d -t codex-image-uat-install.XXXXXX)"
out_dir="$(mktemp -d -t codex-image-uat-output.XXXXXX)"
log_phase "created isolated install/output roots"

PHASE="cargo-install"
log_phase "running cargo install --path . --root <temp> --force"
cargo install --path "$REPO_ROOT" --root "$install_root" --force >/dev/null

binary="$install_root/bin/codex-image"
if [[ ! -x "$binary" ]]; then
  echo "[uat-live-smoke] ERROR phase=cargo-install installed binary missing at $binary" >&2
  exit 1
fi

PHASE="codex-check"
if [[ -n "${CODEX_IMAGE_CODEX_BIN:-}" ]]; then
  if [[ ! -x "$CODEX_IMAGE_CODEX_BIN" ]]; then
    echo '[uat-live-smoke] ERROR phase=codex-check CODEX_IMAGE_CODEX_BIN is not executable' >&2
    exit 1
  fi
elif ! command -v codex >/dev/null 2>&1; then
  log_phase "codex not on PATH; codex-image may still find an extension-bundled Codex executable"
else
  log_phase "codex found on PATH"
fi

PHASE="generate"
log_phase "running Codex-backed generate prompt and capturing manifest JSON"
generate_stdout="$(mktemp -t codex-image-uat-generate-out.XXXXXX)"
generate_stderr="$(mktemp -t codex-image-uat-generate-err.XXXXXX)"
if ! "$binary" generate "UAT smoke image from codex-image" --out "$out_dir" --output json >"$generate_stdout" 2>"$generate_stderr"; then
  redact_cli_stderr "$generate_stderr"
  echo '[uat-live-smoke] ERROR phase=generate generate command failed' >&2
  exit 1
fi

PHASE="manifest-validate"
image_count="$(python3 - "$generate_stdout" "$out_dir" <<'PY'
import json
import os
import sys

stdout_path, out_dir = sys.argv[1], sys.argv[2]
with open(stdout_path, 'r', encoding='utf-8') as fh:
    payload = json.load(fh)

manifest_path = payload.get('manifest_path')
if not isinstance(manifest_path, str) or not manifest_path.strip():
    raise SystemExit('manifest_path missing from generate stdout JSON')

if not os.path.isfile(manifest_path):
    raise SystemExit(f'manifest file missing at {manifest_path}')

expected_manifest_path = os.path.join(out_dir, 'manifest.json')
if os.path.realpath(manifest_path) != os.path.realpath(expected_manifest_path):
    raise SystemExit(
        f'manifest_path {manifest_path} does not match expected {expected_manifest_path}'
    )

with open(manifest_path, 'r', encoding='utf-8') as fh:
    manifest = json.load(fh)

images = manifest.get('images')
if not isinstance(images, list) or not images:
    raise SystemExit('manifest images must be a non-empty array')

for index, image in enumerate(images, start=1):
    image_path = image.get('path')
    if not isinstance(image_path, str) or not image_path.strip():
        raise SystemExit(f'manifest image[{index}] missing path')
    if not os.path.isfile(image_path):
        raise SystemExit(f'manifest image[{index}] file missing at {image_path}')

print(len(images))
PY
)"
log_phase "validated manifest and image artifacts (count=$image_count)"

PHASE="complete"
log_phase "live UAT smoke completed successfully"
