//! PV-028: normalise IPv4-mapped IPv6 addresses before private-network checks
//! so the result does not depend on how the platform resolver represents them.

use std::net::IpAddr;
use std::net::Ipv6Addr;

pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

pub fn is_private_or_local(ip: IpAddr) -> bool {
    match normalize(ip) {
        IpAddr::V4(v4) => {
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
                || v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]) // CGNAT 100.64/10
                || v4.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

pub fn parse(s: &str) -> IpAddr {
    s.parse::<Ipv6Addr>().map(IpAddr::V6).unwrap_or_else(|_| s.parse().expect("ip"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_addresses_are_normalised() {
        assert!(is_private_or_local(parse("::ffff:10.0.0.1")));
        assert!(is_private_or_local(parse("::ffff:127.0.0.1")));
        assert!(is_private_or_local(parse("::ffff:169.254.169.254")));
        assert!(!is_private_or_local(parse("::ffff:93.184.216.34")));
        assert!(is_private_or_local(parse("fd00::1")));
        assert!(!is_private_or_local(parse("2606:2800:220:1:248:1893:25c8:1946")));
    }

    #[tokio::test]
    async fn local_resolver_representation() {
        // Records how this platform's resolver represents localhost; Windows CI
        // repeats the same test.
        let addrs: Vec<_> = tokio::net::lookup_host("localhost:443").await.expect("resolve").collect();
        println!("localhost -> {addrs:?} (os = {})", std::env::consts::OS);
        assert!(addrs.iter().all(|a| is_private_or_local(a.ip())));
    }
}
