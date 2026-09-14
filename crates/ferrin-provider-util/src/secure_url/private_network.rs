//! Private, loopback, link-local, multicast and reserved address detection.

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::sync::LazyLock;

use ipnet::Ipv4Net;
use ipnet::Ipv6Net;

static IPV4_BLOCKED: LazyLock<Vec<Ipv4Net>> = LazyLock::new(|| {
    [
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.0.0.0/24",
        "192.0.2.0/24",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "240.0.0.0/4",
    ]
    .into_iter()
    .filter_map(|net| net.parse().ok())
    .collect()
});

static IPV6_BLOCKED: LazyLock<Vec<Ipv6Net>> = LazyLock::new(|| {
    [
        "::/128",
        "::1/128",
        "fc00::/7",
        "fe80::/10",
        "fec0::/10",
        "ff00::/8",
        "2001:db8::/32",
        "3fff::/20",
    ]
    .into_iter()
    .filter_map(|net| net.parse().ok())
    .collect()
});

/// Returns `true` when `ip` must not be contacted: loopback, unspecified,
/// private (RFC 1918 / RFC 4193), shared address space, link-local,
/// site-local, multicast, documentation, benchmarking and reserved ranges.
/// IPv4-mapped, IPv4-compatible, NAT64 and 6to4 IPv6 addresses are judged
/// by their embedded IPv4 address as well.
#[must_use]
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => {
            if IPV6_BLOCKED.iter().any(|net| net.contains(&v6)) {
                return true;
            }
            embedded_v4(v6).is_some_and(is_private_v4)
        }
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    IPV4_BLOCKED.iter().any(|net| net.contains(&ip))
}

fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ip.segments();
    let low = |index: usize| -> Ipv4Addr {
        let [a, b] = segments[index].to_be_bytes();
        let [c, d] = segments[index + 1].to_be_bytes();
        Ipv4Addr::new(a, b, c, d)
    };
    match segments {
        // ::ffff:a.b.c.d (IPv4-mapped)
        [0, 0, 0, 0, 0, 0xffff, _, _] => Some(low(6)),
        // ::a.b.c.d (IPv4-compatible, deprecated) except :: and ::1
        [0, 0, 0, 0, 0, 0, hi, lo] if hi != 0 || lo > 1 => Some(low(6)),
        // 64:ff9b::/96 and 64:ff9b:1::/48 (NAT64)
        [0x64, 0xff9b, 0, 0, 0, 0, _, _] | [0x64, 0xff9b, 1, _, _, _, _, _] => Some(low(6)),
        // 2002:a.b.c.d::/48 (6to4)
        [0x2002, _, _, _, _, _, _, _] => Some(low(1)),
        _ => None,
    }
}

/// Returns `true` for hostnames that always denote local machines:
/// `localhost`, `*.localhost`, `*.local`, and IP literals in blocked ranges.
#[must_use]
pub fn is_private_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(&host);
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }
    false
}
