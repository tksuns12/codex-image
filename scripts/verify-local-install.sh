#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PHASE="init"
install_root=""
temp_home=""

log_phase() {
  printf '[verify-local-install] phase=%s %s\n' "$PHASE" "$1"
}

cleanup() {
  local rc=$?
  if [[ -n "$install_root" && -d "$install_root" ]]; then
    rm -rf "$install_root"
  fi
  if [[ -n "$temp_home" && -d "$temp_home" ]]; then
    rm -rf "$temp_home"
  fi

  if [[ $rc -eq 0 ]]; then
    printf '[verify-local-install] phase=cleanup removed temp roots\n'
  else
    printf '[verify-local-install] ERROR phase=%s exit_code=%d\n' "$PHASE" "$rc" >&2
  fi
  exit "$rc"
}
trap cleanup EXIT

PHASE="temp-filesystem"
install_root="$(mktemp -d -t codex-image-install.XXXXXX)"
temp_home="$(mktemp -d -t codex-image-home.XXXXXX)"
log_phase "created isolated install/home roots"

PHASE="cargo-install"
log_phase "running cargo install --path . --root <temp> --force"
cargo install --path "$REPO_ROOT" --root "$install_root" --force >/dev/null

PHASE="binary-help"
binary="$install_root/bin/codex-image"
if [[ ! -x "$binary" ]]; then
  echo "[verify-local-install] ERROR phase=$PHASE installed binary missing at temp root" >&2
  exit 1
fi
log_phase "executing codex-image --help"
HOME="$temp_home" "$binary" --help >/dev/null

PHASE="generate-help"
log_phase "executing codex-image generate --help"
generate_help="$(HOME="$temp_home" "$binary" generate --help)"
for marker in '--out' '--output' '--quiet' '--prompt-file' '--timeout' '--debug-diagnostics'; do
  if ! grep -q -- "$marker" <<<"$generate_help"; then
    echo "[verify-local-install] ERROR phase=$PHASE missing generate help marker: $marker" >&2
    exit 1
  fi
done

PHASE="removed-auth-commands"
log_phase "confirming removed auth lifecycle commands are unavailable"
if HOME="$temp_home" "$binary" status --json >/dev/null 2>&1; then
  echo '[verify-local-install] ERROR phase=removed-auth-commands status unexpectedly succeeded' >&2
  exit 1
fi
if HOME="$temp_home" "$binary" login --help >/dev/null 2>&1; then
  echo '[verify-local-install] ERROR phase=removed-auth-commands login unexpectedly succeeded' >&2
  exit 1
fi
if HOME="$temp_home" "$binary" logout --help >/dev/null 2>&1; then
  echo '[verify-local-install] ERROR phase=removed-auth-commands logout unexpectedly succeeded' >&2
  exit 1
fi

PHASE="complete"
log_phase "local install verification complete"
