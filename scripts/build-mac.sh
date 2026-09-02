#!/usr/bin/env bash
set -euo pipefail

# scripts/build-mac.sh - Build and package SwitchAI Account Manager on macOS

# Toolchain & Environment Discovery (Apple Silicon + Intel + Rustup + NVM/FNM)
if [[ -x "/opt/homebrew/bin/brew" ]]; then
  eval "$(/opt/homebrew/bin/brew shellenv)"
elif [[ -x "/usr/local/bin/brew" ]]; then
  eval "$(/usr/local/bin/brew shellenv)"
fi

export PATH="/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"

if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

if [[ -d "${HOME}/.nvm" && -s "${HOME}/.nvm/nvm.sh" ]]; then
  export NVM_DIR="${HOME}/.nvm"
  # shellcheck source=/dev/null
  [ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"
elif command -v fnm &>/dev/null; then
  eval "$(fnm env)"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST_DIR="${REPO_ROOT}/dist/release"

TARGET="universal-apple-darwin"
BUNDLE_FLAG=""

# Parse CLI arguments safely
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      if [[ -z "${2:-}" ]]; then
        echo "Error: --target requires a target architecture argument" >&2
        exit 1
      fi
      TARGET="$2"
      shift 2
      ;;
    --arm64|--aarch64)
      TARGET="aarch64-apple-darwin"
      shift
      ;;
    --x64|--x86_64)
      TARGET="x86_64-apple-darwin"
      shift
      ;;
    --universal)
      TARGET="universal-apple-darwin"
      shift
      ;;
    --no-bundle)
      BUNDLE_FLAG="--no-bundle"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

echo "==> [build-mac] Target Architecture: ${TARGET}"

# Ensure Rust targets are installed
if [[ "${TARGET}" == "universal-apple-darwin" ]]; then
  echo "==> [build-mac] Checking toolchain targets for Universal Binary..."
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
else
  echo "==> [build-mac] Checking toolchain target for ${TARGET}..."
  rustup target add "${TARGET}"
fi

# Verify version parity
echo "==> [build-mac] Verifying version consistency..."
node "${REPO_ROOT}/scripts/verify-versions.mjs"

# Build Frontend
echo "==> [build-mac] Building frontend..."
npm --prefix "${REPO_ROOT}/ui" run build

# Build Tauri Application
echo "==> [build-mac] Compiling Tauri bundle for ${TARGET}..."
cd "${REPO_ROOT}"
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  unset APPLE_SIGNING_IDENTITY
fi
npx tauri build --target "${TARGET}" ${BUNDLE_FLAG}

# Prepare Artifacts Directory and clean stale artifacts
mkdir -p "${DIST_DIR}"

if [[ -z "${BUNDLE_FLAG}" ]]; then
  BUNDLE_OUT="${REPO_ROOT}/src-tauri/target/${TARGET}/release/bundle"

  # Clean old artifacts
  rm -f "${DIST_DIR}/"*.dmg "${DIST_DIR}/SHA256SUMS-mac.txt"

  # Copy DMG artifacts if present as SwitchAI_macos.dmg
  if [[ -d "${BUNDLE_OUT}/dmg" ]]; then
    echo "==> [build-mac] Copying DMG artifact as SwitchAI_macos.dmg..."
    shopt -s nullglob
    for dmg_path in "${BUNDLE_OUT}/dmg/"*.dmg; do
      cp -f "${dmg_path}" "${DIST_DIR}/SwitchAI_macos.dmg"
    done
    shopt -u nullglob
  fi
fi

# Generate Checksums safely
echo "==> [build-mac] Generating SHA256 checksums..."
cd "${DIST_DIR}"
rm -f SHA256SUMS-mac.txt
shopt -s nullglob
artifacts=( *.dmg )
shopt -u nullglob

if [[ ${#artifacts[@]} -gt 0 ]]; then
  shasum -a 256 "${artifacts[@]}" > SHA256SUMS-mac.txt
fi

echo "==> [build-mac] Build complete. Artifacts placed in ${DIST_DIR}"
