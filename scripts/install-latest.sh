#!/usr/bin/env sh
set -eu
set -f

REPO="${CODEX_IMAGE_REPO:-tksuns12/codex-image}"
INSTALL_DIR="${CODEX_IMAGE_INSTALL_DIR:-$HOME/.local/bin}"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "codex-image installer requires '$1' on PATH" >&2
    exit 1
  fi
}

select_checksum_verifier() {
  if command -v sha256sum >/dev/null 2>&1; then
    HASH_TOOL="sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    HASH_TOOL="shasum"
  else
    echo "codex-image installer requires 'sha256sum' or 'shasum' on PATH to verify SHA256SUMS" >&2
    exit 1
  fi
}

is_sha256() {
  case "${1:-}" in
    ""|*[!0123456789abcdefABCDEF]*) return 1 ;;
    *) [ "${#1}" -eq 64 ] ;;
  esac
}

extract_expected_sha256() {
  CHECKSUM_FILE="$1"
  CHECKSUM_MATCHES=0
  EXPECTED_SHA256=""

  while IFS= read -r line || [ -n "$line" ]; do
    if [ -z "$line" ]; then
      continue
    fi

    checksum="$(printf '%s\n' "$line" | awk 'NF == 2 { print $1; exit 0 } { exit 1 }')" || {
      echo "malformed SHA256SUMS entry for ${ASSET}" >&2
      exit 1
    }
    filename="$(printf '%s\n' "$line" | awk 'NF == 2 { print $2; exit 0 } { exit 1 }')" || {
      echo "malformed SHA256SUMS entry for ${ASSET}" >&2
      exit 1
    }
    case "$filename" in
      \*) filename="${filename#\*}" ;;
    esac

    if [ -z "$filename" ] || ! is_sha256 "$checksum"; then
      echo "malformed SHA256SUMS entry for ${ASSET}" >&2
      exit 1
    fi

    if [ "$filename" = "$ASSET" ]; then
      CHECKSUM_MATCHES=$((CHECKSUM_MATCHES + 1))
      EXPECTED_SHA256="$checksum"
    fi
  done < "$CHECKSUM_FILE"

  if [ "$CHECKSUM_MATCHES" -eq 0 ]; then
    echo "SHA256SUMS does not contain checksum for ${ASSET}" >&2
    exit 1
  fi

  if [ "$CHECKSUM_MATCHES" -gt 1 ]; then
    echo "SHA256SUMS contains duplicate checksum entries for ${ASSET}" >&2
    exit 1
  fi
}

verify_archive_checksum() {
  (
    cd "$TMPDIR"
    if [ "$HASH_TOOL" = "sha256sum" ]; then
      printf '%s  %s\n' "$EXPECTED_SHA256" "$ASSET" | sha256sum -c -
    else
      printf '%s  %s\n' "$EXPECTED_SHA256" "$ASSET" | shasum -a 256 -c -
    fi
  ) || {
    echo "checksum mismatch for ${ASSET}" >&2
    exit 1
  }
}

need_command curl
need_command sed
need_command awk
need_command tar
need_command uname
need_command mktemp
need_command install
select_checksum_verifier

VERSION="$(
  curl -fsSL "$API_URL" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' |
    sed -n '1p'
)"

if [ -z "$VERSION" ]; then
  echo "could not resolve latest codex-image release from ${API_URL}" >&2
  exit 1
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  Darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
  Darwin-arm64|Darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
  *)
    echo "unsupported platform: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

ASSET="codex-image-${VERSION}-${TARGET}.tar.gz"
ARCHIVE_ROOT="codex-image-${VERSION}-${TARGET}"
TMPDIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT HUP INT TERM

curl -fL "https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS" -o "${TMPDIR}/SHA256SUMS"
curl -fL "https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}" -o "${TMPDIR}/${ASSET}"
extract_expected_sha256 "${TMPDIR}/SHA256SUMS"
verify_archive_checksum "${TMPDIR}/${ASSET}" "${TMPDIR}/SHA256SUMS"
tar -xzf "${TMPDIR}/${ASSET}" -C "$TMPDIR"
mkdir -p "$INSTALL_DIR"
install -m 0755 "${TMPDIR}/${ARCHIVE_ROOT}/codex-image" "${INSTALL_DIR}/codex-image"

echo "installed codex-image ${VERSION} to ${INSTALL_DIR}/codex-image"
echo "make sure ${INSTALL_DIR} is on your PATH"
"${INSTALL_DIR}/codex-image" --help >/dev/null
