#!/usr/bin/env bash
# Builds `core-mobile-ffi` for Android (arm64-v8a by default; override via ANDROID_ABIS="arm64-v8a,x86_64")
# using `cargo-ndk` with 16 KB ELF page-size alignment (-Wl,-z,max-page-size=16384).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MOBILE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${MOBILE_ROOT}/.." && pwd)"
JNILIBS_DIR="${MOBILE_ROOT}/android/app/src/main/jniLibs"

PROFILE="${1:-release}"
ANDROID_ABIS="${ANDROID_ABIS:-arm64-v8a}"

CARGO_FLAGS=()
if [[ "${PROFILE}" == "release" ]]; then
  CARGO_FLAGS+=(--release)
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"

rm -rf "${JNILIBS_DIR}"
mkdir -p "${JNILIBS_DIR}"

NDK_TARGETS=()
IFS=',' read -ra ABI_ARRAY <<< "${ANDROID_ABIS}"
for abi in "${ABI_ARRAY[@]}"; do
  trimmed="$(echo "${abi}" | xargs)"
  if [[ -n "${trimmed}" ]]; then
    NDK_TARGETS+=(-t "${trimmed}")
  fi
done

echo "[veloce-mobile] Building core-mobile-ffi (${PROFILE}) for Android ABIs: ${ANDROID_ABIS}..."
(
  cd "${REPO_ROOT}"
  cargo ndk \
    "${NDK_TARGETS[@]}" \
    -o "${JNILIBS_DIR}" \
    build -p core-mobile-ffi "${CARGO_FLAGS[@]}"
)

ls -lh "${JNILIBS_DIR}"/*/*.so 2>/dev/null || true
echo "[veloce-mobile] Installed Android native libraries in ${JNILIBS_DIR}"
