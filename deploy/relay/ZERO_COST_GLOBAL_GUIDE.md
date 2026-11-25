# Zero-Cost Global P2P Transfers ($0/month)

Veloce does **not** require a paid server for global transfers. Use these three free options in order:

---

## Option 1: Built-In Zero-Config Direct P2P ($0 — Automatic)
Built directly into Veloce (`v0.1.2+`):
- **Dual-Stack IPv6 (`[::]:0`):** Most 4G/5G cellular carriers assign a globally routable IPv6 address (bypassing IPv4 CGNAT). Veloce advertises and races IPv6 + IPv4 candidates in parallel.
- **Automatic UPnP IGD & NAT-PMP:** When sharing from a PC or phone on home Wi-Fi, Veloce automatically requests a temporary UDP port mapping on your router so a phone on strict 4G/5G CGNAT can connect directly to your home router with zero relay.
- **RFC 5389 Public STUN:** Queries free Cloudflare & Google STUN servers on the shared QUIC UDP socket for standard Cone NAT direct connections.

---

## Option 2: Free Personal Mesh VPN ($0 — Zero VPS Needed)
If **both** devices sit behind IPv4-only symmetric cellular CGNAT (and neither is on a UPnP Wi-Fi router or global IPv6):
1. Install **Tailscale** (free personal plan, up to 100 devices) or **ZeroTier** (free plan) on your PC and phone.
2. Veloce automatically includes Tailscale (`100.64.0.0/10`) / ZeroTier / WireGuard addresses in `veloce1...` tickets and races them in parallel—no Settings changes required.

---

## Option 3: Self-Host `veloce-relay` Forever Free ($0/month on Oracle Cloud Always Free)
If you want arbitrary strangers' phones on double-CGNAT IPv4-only networks to transfer without installing Tailscale, run `veloce-relay` (~3 MB static binary, ~10 MB RAM, stateless UDP) on an **Oracle Cloud Always Free** VM (`VM.Standard.E2.1.Micro` or `VM.Standard.A1.Flex` — $0/month permanently, 10 TB/month free outbound data, public IPv4, full UDP support):

### 1. Open UDP Port `7878`
In Oracle Cloud Console -> **VCN -> Security Lists -> Ingress Rules**, add:
- **Source CIDR:** `0.0.0.0/0`
- **IP Protocol:** `UDP`
- **Destination Port Range:** `7878`

On the VM firewall (Ubuntu):
```bash
sudo iptables -I INPUT 6 -m state --state NEW -p udp --dport 7878 -j ACCEPT
sudo netfilter-persistent save
```

### 2. Run `veloce-relay` via systemd (No Docker Needed)
Download the prebuilt release binary from GitHub Releases (or build with Cargo):
```bash
curl -fsSL https://github.com/Niloyyy/Veloce/releases/latest/download/veloce-relay-linux-x64.tar.gz \
  | sudo tar -xz -C /usr/local/bin
sudo chmod +x /usr/local/bin/veloce-relay

sudo cp deploy/relay/veloce-relay.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now veloce-relay
sudo systemctl status veloce-relay
```
*(Or with Docker: `docker compose -f deploy/relay/docker-compose.yml up -d`.)*

### 3. Use Your Free Relay
- Enter `<your-vm-public-ip>:7878` (or `relay.yourdomain.com:7878`) in **Settings -> Relay Server** on Desktop or Mobile.
- **Optional (Bake Into CI Builds):** Set a GitHub repository variable named `VELOCE_DEFAULT_RELAY` (Settings -> Secrets and variables -> Actions -> **Variables**) to `<your-vm-public-ip>:7878`. Future CI/Release builds will use it automatically whenever a user leaves Relay empty in Settings.
