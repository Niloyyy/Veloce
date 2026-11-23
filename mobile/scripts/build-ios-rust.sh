#!/usr/bin/env bash
# Builds `core-mobile-ffi` static library for iOS (aarch64-apple-ios) and packages
# `VeloceCore.xcframework` for linking into the Flutter iOS Runner.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MOBILE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${MOBILE_ROOT}/.." && pwd)"
OUT_DIR="${MOBILE_ROOT}/ios/Frameworks"

mkdir -p "${OUT_DIR}"

echo "[veloce-mobile] Building core-mobile-ffi (release) for aarch64-apple-ios..."
(
  cd "${REPO_ROOT}"
  cargo build -p core-mobile-ffi --release --target aarch64-apple-ios
)

LIB_DEVICE="${REPO_ROOT}/target/aarch64-apple-ios/release/libcore_mobile_ffi.a"
rm -rf "${OUT_DIR}/VeloceCore.xcframework"

if command -v xcodebuild >/dev/null 2>&1; then
  xcodebuild -create-xcframework \
    -library "${LIB_DEVICE}" \
    -output "${OUT_DIR}/VeloceCore.xcframework"
  echo "[veloce-mobile] Built ${OUT_DIR}/VeloceCore.xcframework"
else
  cp "${LIB_DEVICE}" "${OUT_DIR}/libcore_mobile_ffi.a"
  echo "[veloce-mobile] Copied static library to ${OUT_DIR}/libcore_mobile_ffi.a"
fi
