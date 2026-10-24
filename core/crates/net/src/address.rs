//! Which IP addresses a connection may reach.
//!
//! The host resolves every name itself and connects only to the addresses it
//! allows (`plugin-api/wit/host.wit`, `http`): public addresses for every
//! destination, and for the origins of a Server Grant the local-network or
//! same-device addresses the user confirmed. Anything that is not plainly a
//! public unicast address counts as non-public, including the IPv6 forms
//! that embed an IPv4 address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `address` is a public unicast address.
///
/// Loopback, private, shared (CGNAT), link-local, unique-local, site-local,
/// multicast, broadcast, unspecified, documentation, benchmarking and
/// reserved ranges are not public. IPv4-mapped, IPv4-compatible, NAT64 and
/// 6to4 IPv6 addresses are judged by the IPv4 address they carry, and Teredo
/// addresses are refused, since they tunnel to an address the host cannot
/// check.
pub fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    let reserved = a == 0                              // "this network"
        || a == 10                                     // private
        || (a == 100 && (64..128).contains(&b))        // shared address space (CGNAT)
        || a == 127                                    // loopback
        || (a == 169 && b == 254)                      // link-local
        || (a == 172 && (16..32).contains(&b))         // private
        || (a == 192 && b == 0 && c == 0)              // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2)              // documentation
        || (a == 192 && b == 88 && c == 99)            // 6to4 relay anycast (deprecated)
        || (a == 192 && b == 168)                      // private
        || (a == 198 && (b == 18 || b == 19))          // benchmarking
        || (a == 198 && b == 51 && c == 100)           // documentation
        || (a == 203 && b == 0 && c == 113)            // documentation
        || a >= 224; // multicast, reserved, broadcast
    !reserved
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    if let Some(v4) = embedded_v4(address) {
        return is_public_v4(v4);
    }
    let first = segments[0];
    let reserved = address.is_unspecified()
        || address.is_loopback()
        || segments[..6] == [0, 0, 0, 0, 0, 0]          // IPv4-compatible, handled above when it has a v4 part
        || (first & 0xfe00) == 0xfc00                    // unique local
        || (first & 0xffc0) == 0xfe80                    // link-local
        || (first & 0xffc0) == 0xfec0                    // site-local (deprecated)
        || (first & 0xff00) == 0xff00                    // multicast
        || (first == 0x2001 && segments[1] == 0x0db8)   // documentation
        || (first == 0x3fff && segments[1] < 0x1000)    // documentation (RFC 9637)
        || (first == 0x2001 && segments[1] == 0)        // Teredo
        || (first == 0x2001 && (segments[1] & 0xfff0) == 0x0010) // ORCHIDv1
        || (first == 0x2001 && (segments[1] & 0xfff0) == 0x0020) // ORCHIDv2
        || (first == 0x0100 && segments[1..4] == [0, 0, 0]) // discard-only
        || (first == 0x0064 && segments[1] == 0xff9b && segments[2] == 1) // local-use NAT64
        || (first == 0x5f00); // segment routing SIDs
    // Only global unicast (2000::/3) is public.
    !reserved && (first & 0xe000) == 0x2000
}

/// The IPv4 address an IPv6 address stands for, if it is one of the forms
/// that carry one.
fn embedded_v4(address: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = address.segments();
    let tail = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
    // IPv4-mapped ::ffff:a.b.c.d
    if s[..6] == [0, 0, 0, 0, 0, 0xffff] {
        return Some(tail);
    }
    // IPv4-compatible ::a.b.c.d (deprecated), excluding :: and ::1.
    if s[..6] == [0, 0, 0, 0, 0, 0] && (s[6] != 0 || s[7] > 1) {
        return Some(tail);
    }
    // NAT64 well-known prefix 64:ff9b::/96.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(tail);
    }
    // 6to4 2002:aabb:ccdd::/48.
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    None
}

/// Puts an address in the form the host compares: IPv4-mapped IPv6
/// addresses become the IPv4 address they map.
pub(crate) fn canonical(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn public_addresses_are_public() {
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.0.1",
            "172.32.0.1",
            "192.169.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:0808:0808::1",
        ] {
            assert!(public(address), "{address} should be public");
        }
    }

    #[test]
    fn every_special_range_is_refused() {
        for address in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.255.254",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "3fff::1",
            "2001::1",
            "2001:10::1",
            "2001:20::1",
            "100::1",
            "64:ff9b:1::1",
            "5f00::1",
            "4000::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "::127.0.0.1",
            "::10.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:0001::1",
            "2002:c0a8:0101::1",
        ] {
            assert!(!public(address), "{address} must not be public");
        }
    }

    #[test]
    fn mapped_addresses_compare_as_ipv4() {
        let mapped: IpAddr = "::ffff:192.168.1.20".parse().unwrap();
        assert_eq!(canonical(mapped), "192.168.1.20".parse::<IpAddr>().unwrap());
        let v6: IpAddr = "fd00::1".parse().unwrap();
        assert_eq!(canonical(v6), v6);
    }
}
