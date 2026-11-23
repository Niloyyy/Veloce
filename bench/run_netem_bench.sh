#!/usr/bin/env bash
# Kernel-level tc netem benchmark runner (requires root / CAP_NET_ADMIN).
# Creates an isolated Linux network namespace with netem applied to its loopback
# interface (`50ms` delay + `0.5%` loss per traversal = `100ms` RTT + `~1%` loss)
# without modifying any host network interfaces, and benchmarks TCP vs QUIC-BBR.
#
# Usage:
#   sudo ./bench/run_netem_bench.sh [SIZE_MB] [DELAY_EACH_MS] [LOSS_EACH_PCT]
#   sudo ./bench/run_netem_bench.sh 64 50 0.5

set -euo pipefail

SIZE_MB="${1:-64}"
DELAY_EACH_MS="${2:-50}"
LOSS_EACH_PCT="${3:-0.5}"

if [[ "${EUID}" -ne 0 ]]; then
  echo "[veloce-bench] Not running as root — falling back to non-root userspace impairment benchmark."
  exec cargo run --release -p veloce-bench -- --wan-size-mb "${SIZE_MB}" --rtt-ms "$((DELAY_EACH_MS * 2))" --loss-pct "$(awk "BEGIN { printf \"%.2f\", ${LOSS_EACH_PCT} * 2 }")"
fi

NS="veloce_netem_ns"

cleanup() {
  ip netns del "${NS}" 2>/dev/null || true
}
trap cleanup EXIT

cargo build --release -p veloce-bench
BIN="$(pwd)/target/release/veloce-bench"

ip netns del "${NS}" 2>/dev/null || true
ip netns add "${NS}"
ip netns exec "${NS}" ip link set lo up
ip netns exec "${NS}" tc qdisc add dev lo root netem delay "${DELAY_EACH_MS}ms" loss "${LOSS_EACH_PCT}%"

echo "[veloce-bench] Running kernel tc netem benchmark (${SIZE_MB} MiB, ${DELAY_EACH_MS}ms x 2 RTT, ${LOSS_EACH_PCT}% x 2 loss)..."
ip netns exec "${NS}" "${BIN}" --direct-netem --wan-size-mb "${SIZE_MB}" --rtt-ms "$((DELAY_EACH_MS * 2))" --loss-pct "$(awk "BEGIN { printf \"%.2f\", ${LOSS_EACH_PCT} * 2 }")"
