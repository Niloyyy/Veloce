#!/usr/bin/env bash
# Kernel-level tc netem benchmark runner (requires root / CAP_NET_ADMIN).
# Creates an isolated veth pair with 50ms one-way delay + 0.5% loss per direction
# (100ms total RTT, ~1% round-trip packet loss) and benchmarks TCP vs QUIC-BBR.
#
# For non-root environments, simply run:
#   cargo run --release -p veloce-bench -- --size-mb 8 --rtt-ms 100 --loss-pct 1.0

set -euo pipefail

SIZE_MB="${1:-16}"
DELAY_EACH_MS="${2:-50}"
LOSS_EACH_PCT="${3:-0.5}"

if [[ "${EUID}" -ne 0 ]]; then
  echo "[veloce-bench] Not running as root — falling back to non-root userspace impairment benchmark."
  exec cargo run --release -p veloce-bench -- --size-mb "${SIZE_MB}" --rtt-ms "$((DELAY_EACH_MS * 2))" --loss-pct "$(awk "BEGIN { printf \"%.2f\", ${LOSS_EACH_PCT} * 2 }")"
fi

NS_A="veloce_ns_a"
NS_B="veloce_ns_b"

cleanup() {
  ip netns del "${NS_A}" 2>/dev/null || true
  ip netns del "${NS_B}" 2>/dev/null || true
}
trap cleanup EXIT

cargo build --release -p veloce-bench
BIN="$(pwd)/target/release/veloce-bench"

ip netns add "${NS_A}"
ip netns add "${NS_B}"
ip link add veth_a type veth peer name veth_b
ip link set veth_a netns "${NS_A}"
ip link set veth_b netns "${NS_B}"

ip netns exec "${NS_A}" ip addr add 10.200.1.1/24 dev veth_a
ip netns exec "${NS_B}" ip addr add 10.200.1.2/24 dev veth_b
ip netns exec "${NS_A}" ip link set veth_a up
ip netns exec "${NS_B}" ip link set veth_b up
ip netns exec "${NS_A}" ip link set lo up
ip netns exec "${NS_B}" ip link set lo up

ip netns exec "${NS_A}" tc qdisc add dev veth_a root netem delay "${DELAY_EACH_MS}ms" loss "${LOSS_EACH_PCT}%"
ip netns exec "${NS_B}" tc qdisc add dev veth_b root netem delay "${DELAY_EACH_MS}ms" loss "${LOSS_EACH_PCT}%"

echo "[veloce-bench] Running kernel tc netem benchmark (${DELAY_EACH_MS}ms x 2 RTT, ${LOSS_EACH_PCT}% x 2 loss)..."
ip netns exec "${NS_A}" "${BIN}" --direct-netem --size-mb "${SIZE_MB}" --rtt-ms "$((DELAY_EACH_MS * 2))" --loss-pct "$(awk "BEGIN { printf \"%.2f\", ${LOSS_EACH_PCT} * 2 }")"
