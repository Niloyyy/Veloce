import { Globe, Radio, Wifi } from "lucide-react";
import type { ConnectionType } from "../types/veloce";

interface ConnectionBadgeProps {
  type: ConnectionType;
}

export function ConnectionBadge({ type }: ConnectionBadgeProps) {
  if (type === "LAN") {
    return (
      <span className="conn-badge conn-lan" title="Direct Local Network transfer over QUIC (BBR)">
        <Wifi size={13} />
        <span>LAN</span>
      </span>
    );
  }
  if (type === "Direct-P2P") {
    return (
      <span
        className="conn-badge conn-p2p"
        title="Direct hole-punched UDP P2P connection across WAN"
      >
        <Globe size={13} />
        <span>Direct P2P (UDP)</span>
      </span>
    );
  }
  return (
    <span
      className="conn-badge conn-relayed"
      title="End-to-end encrypted relay fallback (upgrading to Direct P2P when possible)"
    >
      <Radio size={13} />
      <span>Relayed</span>
    </span>
  );
}
