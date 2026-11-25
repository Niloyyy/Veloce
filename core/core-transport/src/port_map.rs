use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const SSDP_MULTICAST_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
const NAT_PMP_PORT: u16 = 5351;
const DEFAULT_LEASE_SECS: u32 = 3600;
const MAX_HTTP_RESPONSE_BYTES: usize = 64 * 1024;

/// Router port-forwarding protocol used to obtain a public UDP mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortMapProtocol {
    UpnpIgd,
    NatPmp,
}

/// Active temporary UDP port mapping leased from the local home/office router.
#[derive(Debug, Clone)]
pub struct PortMappingLease {
    pub protocol: PortMapProtocol,
    pub external_addr: SocketAddr,
    pub internal_port: u16,
    pub external_port: u16,
    pub lifetime_secs: u32,
    upnp_control_url: Option<String>,
    upnp_service_type: Option<String>,
    natpmp_gateway: Option<Ipv4Addr>,
}

impl PortMappingLease {
    /// Best-effort release of the temporary router port mapping on shutdown.
    pub async fn release(&self) {
        match self.protocol {
            PortMapProtocol::UpnpIgd => {
                if let (Some(url), Some(service)) =
                    (self.upnp_control_url.as_deref(), self.upnp_service_type.as_deref())
                {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(500),
                        upnp_delete_port_mapping(url, service, self.external_port),
                    )
                    .await;
                }
            }
            PortMapProtocol::NatPmp => {
                if let Some(gw) = self.natpmp_gateway {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(400),
                        natpmp_request_mapping(gw, self.internal_port, 0, 0),
                    )
                    .await;
                }
            }
        }
    }
}

/// Returns true if an IPv4 address is publicly routable on the Internet (excluding RFC 1918
/// private, loopback, link-local, RFC 6598 CGNAT `100.64.0.0/10`, multicast, and reserved ranges).
pub fn is_public_routable_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let is_cgnat = o[0] == 100 && (o[1] & 0b1100_0000) == 64;
    let is_benchmark = o[0] == 198 && (o[1] == 18 || o[1] == 19);
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !is_cgnat
        && !is_benchmark
}

/// Attempts zero-cost automatic UDP port mapping on the local LAN router using both
/// UPnP IGD (SSDP + SOAP) and NAT-PMP (RFC 6886) in parallel, bounded to ~1.2 seconds.
pub async fn try_map_udp_port(
    internal_port: u16,
    local_lan_ipv4s: &[Ipv4Addr],
) -> Option<PortMappingLease> {
    if internal_port == 0 {
        return None;
    }

    let gateways = discover_gateway_candidates(local_lan_ipv4s);
    let upnp_fut = tokio::time::timeout(
        Duration::from_millis(1200),
        try_upnp_igd_map(internal_port),
    );
    let natpmp_fut = tokio::time::timeout(
        Duration::from_millis(800),
        try_nat_pmp_map(internal_port, &gateways),
    );

    let (upnp_res, natpmp_res) = tokio::join!(upnp_fut, natpmp_fut);
    if let Ok(Some(lease)) = upnp_res {
        return Some(lease);
    }
    if let Ok(Some(lease)) = natpmp_res {
        return Some(lease);
    }
    None
}

fn discover_gateway_candidates(local_lan_ipv4s: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    let mut gateways = Vec::new();

    // Linux default route lookup via /proc/net/route (zero FFI / zero unsafe).
    #[cfg(target_os = "linux")]
    if let Ok(contents) = std::fs::read_to_string("/proc/net/route") {
        for gw in parse_linux_proc_net_route_gateways(&contents) {
            if !gateways.contains(&gw) {
                gateways.push(gw);
            }
        }
    }

    // Cross-platform heuristic fallback: `.1` gateway for each local private LAN /24 subnet.
    for &ip in local_lan_ipv4s {
        if ip.is_private() {
            let o = ip.octets();
            let candidate = Ipv4Addr::new(o[0], o[1], o[2], 1);
            if candidate != ip && !gateways.contains(&candidate) {
                gateways.push(candidate);
            }
        }
    }
    gateways
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_proc_net_route_gateways(contents: &str) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for line in contents.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }
        // Default route has Destination == 00000000 and non-zero Gateway (little-endian hex on x86/ARM).
        if cols[1] != "00000000" {
            continue;
        }
        if let Ok(raw) = u32::from_str_radix(cols[2], 16) {
            let ip = Ipv4Addr::from(raw.to_ne_bytes());
            if !ip.is_unspecified() && !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

async fn try_nat_pmp_map(internal_port: u16, gateways: &[Ipv4Addr]) -> Option<PortMappingLease> {
    for &gw in gateways.iter().take(4) {
        let ext_ip = match natpmp_get_external_ip(gw).await {
            Some(ip) if is_public_routable_ipv4(ip) => ip,
            _ => continue,
        };
        if let Some((ext_port, lifetime)) =
            natpmp_request_mapping(gw, internal_port, internal_port, DEFAULT_LEASE_SECS).await
        {
            if ext_port > 0 {
                return Some(PortMappingLease {
                    protocol: PortMapProtocol::NatPmp,
                    external_addr: SocketAddr::new(IpAddr::V4(ext_ip), ext_port),
                    internal_port,
                    external_port: ext_port,
                    lifetime_secs: lifetime,
                    upnp_control_url: None,
                    upnp_service_type: None,
                    natpmp_gateway: Some(gw),
                });
            }
        }
    }
    None
}

async fn natpmp_get_external_ip(gateway: Ipv4Addr) -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
        .await
        .ok()?;
    let target = SocketAddr::new(IpAddr::V4(gateway), NAT_PMP_PORT);
    // RFC 6886 §3.2: Version=0, Opcode=0
    sock.send_to(&[0u8, 0u8], target).await.ok()?;
    let mut buf = [0u8; 32];
    let (n, from) = tokio::time::timeout(Duration::from_millis(300), sock.recv_from(&mut buf))
        .await
        .ok()?
        .ok()?;
    if from.ip() != IpAddr::V4(gateway) {
        return None;
    }
    parse_natpmp_external_ip_response(&buf[..n])
}

async fn natpmp_request_mapping(
    gateway: Ipv4Addr,
    internal_port: u16,
    requested_external_port: u16,
    lifetime_secs: u32,
) -> Option<(u16, u32)> {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
        .await
        .ok()?;
    let target = SocketAddr::new(IpAddr::V4(gateway), NAT_PMP_PORT);
    let req = encode_natpmp_udp_mapping_request(internal_port, requested_external_port, lifetime_secs);
    sock.send_to(&req, target).await.ok()?;
    let mut buf = [0u8; 32];
    let (n, from) = tokio::time::timeout(Duration::from_millis(350), sock.recv_from(&mut buf))
        .await
        .ok()?
        .ok()?;
    if from.ip() != IpAddr::V4(gateway) {
        return None;
    }
    parse_natpmp_udp_mapping_response(&buf[..n], internal_port)
}

fn encode_natpmp_udp_mapping_request(
    internal_port: u16,
    external_port: u16,
    lifetime_secs: u32,
) -> [u8; 12] {
    let mut pkt = [0u8; 12];
    pkt[0] = 0; // Version 0
    pkt[1] = 1; // Opcode 1 = Map UDP
    pkt[4..6].copy_from_slice(&internal_port.to_be_bytes());
    pkt[6..8].copy_from_slice(&external_port.to_be_bytes());
    pkt[8..12].copy_from_slice(&lifetime_secs.to_be_bytes());
    pkt
}

fn parse_natpmp_external_ip_response(buf: &[u8]) -> Option<Ipv4Addr> {
    if buf.len() < 12 || buf[0] != 0 || buf[1] != 128 {
        return None;
    }
    let result_code = u16::from_be_bytes([buf[2], buf[3]]);
    if result_code != 0 {
        return None;
    }
    Some(Ipv4Addr::new(buf[8], buf[9], buf[10], buf[11]))
}

fn parse_natpmp_udp_mapping_response(buf: &[u8], expected_internal_port: u16) -> Option<(u16, u32)> {
    if buf.len() < 16 || buf[0] != 0 || buf[1] != 129 {
        return None;
    }
    let result_code = u16::from_be_bytes([buf[2], buf[3]]);
    if result_code != 0 {
        return None;
    }
    let internal_port = u16::from_be_bytes([buf[8], buf[9]]);
    if internal_port != expected_internal_port {
        return None;
    }
    let external_port = u16::from_be_bytes([buf[10], buf[11]]);
    let lifetime = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
    Some((external_port, lifetime))
}

async fn try_upnp_igd_map(internal_port: u16) -> Option<PortMappingLease> {
    let locations = discover_upnp_locations().await;
    for location in locations.into_iter().take(3) {
        let Some((host, port, path)) = parse_lan_http_url(&location) else {
            continue;
        };
        let Ok(xml) = http_get_text(host, port, &path).await else {
            continue;
        };
        let Some((service_type, control_url)) = extract_upnp_wan_control_url(&xml, host, port) else {
            continue;
        };
        let Some(local_ip) = local_ipv4_for_target(SocketAddr::new(IpAddr::V4(host), port)) else {
            continue;
        };

        // Try preferred matching external port first; fall back to permanent lease or alternate port if needed.
        let mut mapped_port = internal_port;
        let mut lease_secs = DEFAULT_LEASE_SECS;
        if upnp_add_port_mapping(
            &control_url,
            &service_type,
            mapped_port,
            internal_port,
            local_ip,
            lease_secs,
        )
        .await
        .is_err()
        {
            // Some consumer routers reject finite lease durations (SOAP error 725).
            lease_secs = 0;
            if upnp_add_port_mapping(
                &control_url,
                &service_type,
                mapped_port,
                internal_port,
                local_ip,
                lease_secs,
            )
            .await
            .is_err()
            {
                // Retry with an alternate high UDP port in case of port conflict (error 718).
                mapped_port = 40000 + (internal_port % 20000);
                lease_secs = DEFAULT_LEASE_SECS;
                if upnp_add_port_mapping(
                    &control_url,
                    &service_type,
                    mapped_port,
                    internal_port,
                    local_ip,
                    lease_secs,
                )
                .await
                .is_err()
                {
                    continue;
                }
            }
        }

        let Some(ext_ip) = upnp_get_external_ip(&control_url, &service_type).await else {
            continue;
        };
        if !is_public_routable_ipv4(ext_ip) {
            continue;
        }

        return Some(PortMappingLease {
            protocol: PortMapProtocol::UpnpIgd,
            external_addr: SocketAddr::new(IpAddr::V4(ext_ip), mapped_port),
            internal_port,
            external_port: mapped_port,
            lifetime_secs: lease_secs,
            upnp_control_url: Some(control_url),
            upnp_service_type: Some(service_type),
            natpmp_gateway: None,
        });
    }
    None
}

async fn discover_upnp_locations() -> Vec<String> {
    let Ok(sock) = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await
    else {
        return Vec::new();
    };

    const SEARCH_TARGETS: &[&str] = &[
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];

    for st in SEARCH_TARGETS {
        let req = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 1\r\n\
             ST: {st}\r\n\r\n"
        );
        let _ = sock.send_to(req.as_bytes(), SSDP_MULTICAST_ADDR).await;
    }

    let mut locations = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(450);
    let mut buf = [0u8; 2048];
    while tokio::time::Instant::now() < deadline && locations.len() < 4 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                if let Ok(text) = std::str::from_utf8(&buf[..n]) {
                    if let Some(loc) = parse_ssdp_location(text) {
                        if parse_lan_http_url(&loc).is_some() && !locations.contains(&loc) {
                            locations.push(loc);
                        }
                    }
                }
            }
            _ => break,
        }
    }
    locations
}

fn parse_ssdp_location(response: &str) -> Option<String> {
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.len() > 9 && trimmed[..9].eq_ignore_ascii_case("location:") {
            let val = trimmed[9..].trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Parses an `http://<lan-ipv4>[:port]/path` URL, rejecting non-LAN hosts as an SSRF guard.
fn parse_lan_http_url(url: &str) -> Option<(Ipv4Addr, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host_str, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (authority, 80),
    };
    let ip: Ipv4Addr = host_str.parse().ok()?;
    if !ip.is_private() && !ip.is_link_local() {
        return None;
    }
    Some((ip, port, path.to_string()))
}

fn local_ipv4_for_target(target: SocketAddr) -> Option<Ipv4Addr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(target).ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

fn extract_upnp_wan_control_url(
    xml: &str,
    default_host: Ipv4Addr,
    default_port: u16,
) -> Option<(String, String)> {
    const PREFERRED_SERVICES: &[&str] = &[
        "urn:schemas-upnp-org:service:WANIPConnection:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];

    let url_base = extract_xml_tag(xml, "URLBase").map(|s| s.trim().trim_end_matches('/').to_string());

    for &service_type in PREFERRED_SERVICES {
        let mut search_from = 0usize;
        while let Some(rel_idx) = xml[search_from..].find(service_type) {
            let abs_idx = search_from + rel_idx;
            let service_end = xml[abs_idx..]
                .find("</service>")
                .or_else(|| xml[abs_idx..].find("</Service>"))
                .map(|e| abs_idx + e)
                .unwrap_or(xml.len());
            let service_slice = &xml[abs_idx..service_end];
            if let Some(raw_ctrl) = extract_xml_tag(service_slice, "controlURL") {
                let ctrl = raw_ctrl.trim();
                let resolved = if ctrl.starts_with("http://") {
                    ctrl.to_string()
                } else if let Some(ref base) = url_base {
                    if ctrl.starts_with('/') {
                        format!("{base}{ctrl}")
                    } else {
                        format!("{base}/{ctrl}")
                    }
                } else if ctrl.starts_with('/') {
                    format!("http://{default_host}:{default_port}{ctrl}")
                } else {
                    format!("http://{default_host}:{default_port}/{ctrl}")
                };
                if parse_lan_http_url(&resolved).is_some() {
                    return Some((service_type.to_string(), resolved));
                }
            }
            search_from = abs_idx + service_type.len();
        }
    }
    None
}

fn extract_xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let lower = xml.to_ascii_lowercase();
    let open = format!("<{}", tag.to_ascii_lowercase());
    let close = format!("</{}>", tag.to_ascii_lowercase());

    let mut start = 0usize;
    while let Some(pos) = lower[start..].find(&open) {
        let open_pos = start + pos;
        let after_name = open_pos + open.len();
        let next_ch = lower.as_bytes().get(after_name).copied();
        if !matches!(next_ch, Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')) {
            start = after_name;
            continue;
        }
        let gt = lower[after_name..].find('>')? + after_name;
        let body_start = gt + 1;
        let body_end = lower[body_start..].find(&close)? + body_start;
        return Some(&xml[body_start..body_end]);
    }
    None
}

async fn http_get_text(host: Ipv4Addr, port: u16, path: &str) -> std::io::Result<String> {
    let mut stream = tokio::time::timeout(
        Duration::from_millis(500),
        TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "UPnP HTTP GET timeout"))??;

    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nUser-Agent: Veloce/1.0\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let body = read_bounded_http_body(&mut stream).await?;
    Ok(body)
}

async fn http_post_soap(
    control_url: &str,
    service_type: &str,
    action: &str,
    body_xml: &str,
) -> std::io::Result<String> {
    let (host, port, path) = parse_lan_http_url(control_url)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid UPnP URL"))?;
    let mut stream = tokio::time::timeout(
        Duration::from_millis(500),
        TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "UPnP SOAP timeout"))??;

    let req = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: text/xml; charset=\"utf-8\"\r\n\
         Content-Length: {}\r\n\
         SOAPACTION: \"{service_type}#{action}\"\r\n\
         Connection: close\r\n\r\n\
         {body_xml}",
        body_xml.len()
    );
    stream.write_all(req.as_bytes()).await?;
    read_bounded_http_body(&mut stream).await
}

async fn read_bounded_http_body(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut raw = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(600);
    while raw.len() < MAX_HTTP_RESPONSE_BYTES {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                let take = n.min(MAX_HTTP_RESPONSE_BYTES - raw.len());
                raw.extend_from_slice(&chunk[..take]);
            }
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let first_line = text.lines().next().unwrap_or("");
    if !first_line.contains(" 200 ") {
        return Err(std::io::Error::other(format!(
            "UPnP HTTP error: {first_line}"
        )));
    }
    let body = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map(|(_, b)| b)
        .unwrap_or(&text);
    Ok(body.to_string())
}

async fn upnp_add_port_mapping(
    control_url: &str,
    service_type: &str,
    external_port: u16,
    internal_port: u16,
    local_ip: Ipv4Addr,
    lease_secs: u32,
) -> std::io::Result<()> {
    let soap = format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:AddPortMapping xmlns:u=\"{service_type}\">\
         <NewRemoteHost></NewRemoteHost>\
         <NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol>\
         <NewInternalPort>{internal_port}</NewInternalPort>\
         <NewInternalClient>{local_ip}</NewInternalClient>\
         <NewEnabled>1</NewEnabled>\
         <NewPortMappingDescription>Veloce P2P</NewPortMappingDescription>\
         <NewLeaseDuration>{lease_secs}</NewLeaseDuration>\
         </u:AddPortMapping></s:Body></s:Envelope>"
    );
    http_post_soap(control_url, service_type, "AddPortMapping", &soap).await?;
    Ok(())
}

async fn upnp_delete_port_mapping(
    control_url: &str,
    service_type: &str,
    external_port: u16,
) -> std::io::Result<()> {
    let soap = format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:DeletePortMapping xmlns:u=\"{service_type}\">\
         <NewRemoteHost></NewRemoteHost>\
         <NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol>\
         </u:DeletePortMapping></s:Body></s:Envelope>"
    );
    http_post_soap(control_url, service_type, "DeletePortMapping", &soap).await?;
    Ok(())
}

async fn upnp_get_external_ip(control_url: &str, service_type: &str) -> Option<Ipv4Addr> {
    let soap = format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:GetExternalIPAddress xmlns:u=\"{service_type}\"/>\
         </s:Body></s:Envelope>"
    );
    let body = http_post_soap(control_url, service_type, "GetExternalIPAddress", &soap)
        .await
        .ok()?;
    let ip_str = extract_xml_tag(&body, "NewExternalIPAddress")?.trim();
    ip_str.parse::<Ipv4Addr>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssdp_and_upnp_xml_parsers_handle_standard_igd_payloads() {
        let ssdp = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nLOCATION: http://192.168.1.1:5000/rootDesc.xml\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
        let loc = parse_ssdp_location(ssdp).unwrap();
        assert_eq!(loc, "http://192.168.1.1:5000/rootDesc.xml");
        assert_eq!(
            parse_lan_http_url(&loc),
            Some((Ipv4Addr::new(192, 168, 1, 1), 5000, "/rootDesc.xml".into()))
        );
        // Public host must be rejected (SSRF guard).
        assert!(parse_lan_http_url("http://8.8.8.8:80/evil.xml").is_none());

        let xml = r#"
        <root><device><serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
            <controlURL>/ctl/IPConn</controlURL>
          </service>
        </serviceList></device></root>"#;
        let (stype, ctrl) =
            extract_upnp_wan_control_url(xml, Ipv4Addr::new(192, 168, 1, 1), 5000).unwrap();
        assert_eq!(stype, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(ctrl, "http://192.168.1.1:5000/ctl/IPConn");

        let soap_resp = "<s:Envelope><s:Body><u:GetExternalIPAddressResponse><NewExternalIPAddress>203.0.113.42</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>";
        assert_eq!(
            extract_xml_tag(soap_resp, "NewExternalIPAddress"),
            Some("203.0.113.42")
        );
    }

    #[test]
    fn natpmp_packet_encode_decode_and_public_ip_filter() {
        let req = encode_natpmp_udp_mapping_request(51234, 51234, 3600);
        assert_eq!(req[0], 0);
        assert_eq!(req[1], 1);
        assert_eq!(u16::from_be_bytes([req[4], req[5]]), 51234);

        let ip_resp = [0, 128, 0, 0, 0, 0, 1, 0, 198, 51, 100, 9];
        assert_eq!(
            parse_natpmp_external_ip_response(&ip_resp),
            Some(Ipv4Addr::new(198, 51, 100, 9))
        );

        let mut map_resp = [0u8; 16];
        map_resp[0] = 0;
        map_resp[1] = 129;
        map_resp[8..10].copy_from_slice(&51234u16.to_be_bytes());
        map_resp[10..12].copy_from_slice(&51234u16.to_be_bytes());
        map_resp[12..16].copy_from_slice(&3600u32.to_be_bytes());
        assert_eq!(
            parse_natpmp_udp_mapping_response(&map_resp, 51234),
            Some((51234, 3600))
        );

        assert!(is_public_routable_ipv4(Ipv4Addr::new(1, 1, 1, 1)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(100, 64, 12, 34))); // Carrier CGNAT
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(100, 127, 255, 1))); // Carrier CGNAT

        let proc_route = "Iface\tDestination\tGateway\tFlags\n\
                          wlan0\t00000000\t0101A8C0\t0003\n\
                          wlan0\t0001A8C0\t00000000\t0001\n";
        assert_eq!(
            parse_linux_proc_net_route_gateways(proc_route),
            vec![Ipv4Addr::new(192, 168, 1, 1)]
        );
    }
}
