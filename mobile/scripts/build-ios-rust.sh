#!/usr/bin/env bash
# Builds `core-mobile-ffi` static library for iOS (aarch64-apple-ios) and packages
# `VeloceCore.xcframework` with C module headers for linking into the Flutter iOS Runner.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MOBILE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${MOBILE_ROOT}/.." && pwd)"
OUT_DIR="${MOBILE_ROOT}/ios/Frameworks"
HEADERS_DIR="${OUT_DIR}/.headers"

mkdir -p "${OUT_DIR}" "${HEADERS_DIR}"

cat > "${HEADERS_DIR}/veloce_core_ffi.h" <<'EOF'
#ifndef VELOCE_CORE_FFI_H
#define VELOCE_CORE_FFI_H

#include <stdint.h>

uint32_t veloce_core_ios_anchor(void);

#endif
EOF

cat > "${HEADERS_DIR}/module.modulemap" <<'EOF'
module VeloceCore {
    header "veloce_core_ffi.h"
    export *
}
EOF

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
    -headers "${HEADERS_DIR}" \
    -output "${OUT_DIR}/VeloceCore.xcframework"
  rm -rf "${HEADERS_DIR}"
  echo "[veloce-mobile] Built ${OUT_DIR}/VeloceCore.xcframework"
else
  cp "${LIB_DEVICE}" "${OUT_DIR}/libcore_mobile_ffi.a"
  echo "[veloce-mobile] Copied static library to ${OUT_DIR}/libcore_mobile_ffi.a"
fi
