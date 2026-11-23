#!/usr/bin/env bash
# Builds `core-mobile-ffi` for Android arm64-v8a and x86_64 using `cargo-ndk`
# with 16 KB ELF page-size alignment (-Wl,-z,max-page-size=16384) for Android 15+ / Play Store.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MOBILE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${MOBILE_ROOT}/.." && pwd)"
JNILIBS_DIR="${MOBILE_ROOT}/android/app/src/main/jniLibs"

PROFILE="${1:-release}"
CARGO_FLAGS=()
if [[ "${PROFILE}" == "release" ]]; then
  CARGO_FLAGS+=(--release)
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"

mkdir -p "${JNILIBS_DIR}"

echo "[veloce-mobile] Building core-mobile-ffi (${PROFILE}) for Android arm64-v8a and x86_64..."
(
  cd "${REPO_ROOT}"
  cargo ndk \
    -t arm64-v8a \
    -t x86_64 \
    -o "${JNILIBS_DIR}" \
    build -p core-mobile-ffi "${CARGO_FLAGS[@]}"
)

echo "[veloce-mobile] Installed Android native libraries in ${JNILIBS_DIR}"
