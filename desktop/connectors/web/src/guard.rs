//! What `fetch_url` is allowed to reach (03 §5: "private and loopback address ranges blocked").
//!
//! Two checks, and they are separate on purpose. The *scheme* check refuses `file:`, `ftp:`,
//! `data:` and the rest outright: a fetch tool that reads `file:///etc/passwd` is the filesystem
//! connector with none of its scope. The *address* check refuses anything that resolves inside
//! this machine or this network — the router's admin page, a metadata service at 169.254.169.254,
//! a database on localhost. Both run again on every redirect hop, because a public URL that
//! redirects to `http://127.0.0.1:8080/` is the whole attack and checking only the first URL
//! would miss it.
//!
//! What this does not stop is DNS rebinding: the name is resolved here and resolved again by the
//! connection, and a record with a one-second TTL can differ between the two. Closing that means
//! connecting to the address that was checked rather than to the name, which reqwest does not
//! expose. It is written down rather than glossed over, and the bound that does hold — a hostile
//! *page* cannot redirect us inward — is the one this is here for.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Why a URL was refused. Each carries the sentence the model is given, because a refusal it
/// cannot act on costs a turn (§11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAUrl(String),
    Scheme(String),
    NoHost,
    Private { host: String, addr: IpAddr },
    Unresolvable { host: String, reason: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAUrl(raw) => write!(
                f,
                "`{raw}` is not a URL. Give a full address including the scheme, such as \
                 https://example.com/page."
            ),
            Self::Scheme(scheme) => write!(
                f,
                "this tool only fetches http and https URLs, and that one is `{scheme}`. A file \
                 on this machine is the filesystem connector's, not this one's."
            ),
            Self::NoHost => write!(f, "that URL has no host to fetch from."),
            Self::Private { host, addr } => write!(
                f,
                "{host} resolves to {addr}, which is on this machine or this private network. \
                 This tool reaches the public internet only, so nothing on the local network can \
                 be read through it."
            ),
            Self::Unresolvable { host, reason } => {
                write!(f, "{host} could not be resolved: {reason}")
            }
        }
    }
}

/// Parses a URL and checks its scheme, without touching the network.
///
/// Separate from the address check so that an obviously wrong URL is refused before a DNS
/// lookup, and so the parse is testable offline on its own.
pub fn parse(raw: &str) -> Result<Url, Refusal> {
    let url = Url::parse(raw.trim()).map_err(|_| Refusal::NotAUrl(raw.trim().to_owned()))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(Refusal::Scheme(other.to_owned())),
    }
    if url.host().is_none() {
        return Err(Refusal::NoHost);
    }
    Ok(url)
}

/// Resolves the host and refuses if *any* address it answers with is one we will not reach.
///
/// Any, not all: a name that answers with one public address and one loopback address is a
/// rebinding attempt, and which of the two the connection picks is not ours to decide.
pub async fn check_address(url: &Url) -> Result<(), Refusal> {
    let host = url.host().ok_or(Refusal::NoHost)?;
    let name = host.to_string();
    match host {
        Host::Ipv4(ip) => reachable(&name, IpAddr::V4(ip)),
        Host::Ipv6(ip) => reachable(&name, IpAddr::V6(ip)),
        Host::Domain(domain) => {
            // The port only matters because `lookup_host` wants a socket address; 80 stands in
            // for "whatever this URL's port is" when the URL does not say.
            let port = url.port_or_known_default().unwrap_or(80);
            let addrs = tokio::net::lookup_host((domain, port))
                .await
                .map_err(|err| Refusal::Unresolvable {
                    host: name.clone(),
                    reason: err.to_string(),
                })?;
            let mut any = false;
            for addr in addrs {
                any = true;
                reachable(&name, addr.ip())?;
            }
            if any {
                Ok(())
            } else {
                Err(Refusal::Unresolvable {
                    host: name,
                    reason: "the name has no addresses".to_owned(),
                })
            }
        }
    }
}

fn reachable(host: &str, addr: IpAddr) -> Result<(), Refusal> {
    if is_public(addr) {
        Ok(())
    } else {
        Err(Refusal::Private {
            host: host.to_owned(),
            addr,
        })
    }
}

/// Whether an address is out on the public internet.
///
/// Written out rather than taken from `IpAddr::is_global`, which is still unstable on the
/// toolchain this pins (1.93). The list is the one that matters for a fetch tool: this machine,
/// this LAN, the carrier-grade range a home router sits behind, and the link-local address
/// 169.254.169.254 that every cloud hangs its credential service on.
#[must_use]
pub fn is_public(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()            // 10/8, 172.16/12, 192.168/16
        || ip.is_loopback()      // 127/8
        || ip.is_link_local()    // 169.254/16, the cloud metadata service among it
        || ip.is_broadcast()     // 255.255.255.255
        || ip.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || ip.is_unspecified()   // 0.0.0.0, which means "this host" to a connect()
        || ip.is_multicast()
        || a == 0                    // 0/8, "this network"
        || (a == 100 && (64..128).contains(&b))  // 100.64/10 carrier-grade NAT
        || (a == 192 && b == 0 && c == 0)  // 192.0.0/24 IETF protocol assignments
        || (a == 198 && (18..20).contains(&b))   // 198.18/15 benchmarking
        || a >= 240) // 240/4 reserved, and 255.255.255.255 with it
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    // An IPv4 address wearing an IPv6 hat is still that IPv4 address, and ::ffff:127.0.0.1 is
    // the oldest way around a loopback check there is.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    // `to_ipv4` also matches the deprecated IPv4-compatible form ::a.b.c.d, which is not a
    // public address in its own right either.
    if ip.to_ipv4().is_some() {
        return false;
    }
    let segments = ip.segments();
    // Three transition formats carry an IPv4 address inside an IPv6 one. Left alone they are a
    // way back to 127.0.0.1 with none of the checks above ever looking at it: `2002:7f00:1::` is
    // 6to4 for 127.0.0.1, and `64:ff9b::a00:1` is NAT64 for 10.0.0.1. Whether this host actually
    // has a relay or a gateway to make the trip is not the question — an address whose meaning
    // is an address we refuse is one we refuse.
    if let Some(v4) = embedded_v4(segments)
        && !is_public_v4(v4)
    {
        return false;
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00   // fc00::/7 unique local
        || (segments[0] & 0xffc0) == 0xfe80   // fe80::/10 link local
        || (segments[0] == 0x2001 && segments[1] == 0x0db8) // 2001:db8::/32 documentation
        || segments[0] == 0x2002                            // 2002::/16 6to4
        || (segments[0] == 0x2001 && segments[1] == 0x0000)) // 2001::/32 Teredo
}

/// The IPv4 address an IPv6 transition format carries, if it is one of them.
fn embedded_v4(s: [u16; 8]) -> Option<Ipv4Addr> {
    let v4 = |hi: u16, lo: u16| Ipv4Addr::from(((u32::from(hi)) << 16) | u32::from(lo));
    match s {
        // 6to4: 2002:<v4>::/48.
        [0x2002, a, b, ..] => Some(v4(a, b)),
        // Teredo: 2001:0:<server v4>:… — the server is the part worth checking.
        [0x2001, 0x0000, a, b, ..] => Some(v4(a, b)),
        // NAT64 well-known prefix 64:ff9b::/96, and the local prefix 64:ff9b:1::/48.
        [0x0064, 0xff9b, 0, 0, 0, 0, a, b] => Some(v4(a, b)),
        [0x0064, 0xff9b, 0x0001, _, _, _, a, b] => Some(v4(a, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_http_and_https_are_fetched() {
        assert!(parse("https://example.com/a").is_ok());
        assert!(parse("http://example.com").is_ok());
        assert_eq!(
            parse("file:///etc/passwd"),
            Err(Refusal::Scheme("file".to_owned()))
        );
        assert_eq!(
            parse("data:text/html,<b>x"),
            Err(Refusal::Scheme("data".to_owned()))
        );
        assert!(matches!(parse("example.com"), Err(Refusal::NotAUrl(_))));
    }

    #[test]
    fn this_machine_and_this_network_are_not_public() {
        for local in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "10.0.0.1",
            "172.16.5.4",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public(ip(local)), "{local} should not be reachable");
        }
    }

    #[test]
    fn an_ipv4_address_hidden_inside_an_ipv6_one_is_still_that_address() {
        // Three transition formats carry an IPv4 address. None of the plain IPv6 checks looks
        // inside them, so without this each is a way to name 127.0.0.1 and be allowed.
        for hidden in [
            "2002:7f00:0001::",      // 6to4 for 127.0.0.1
            "2002:0a00:0001::",      // 6to4 for 10.0.0.1
            "2002:a9fe:a9fe::",      // 6to4 for 169.254.169.254
            "64:ff9b::7f00:1",       // NAT64 well-known prefix for 127.0.0.1
            "64:ff9b::a00:1",        // NAT64 for 10.0.0.1
            "64:ff9b:1::c0a8:1",     // NAT64 local prefix for 192.168.0.1
            "2001:0:0:0:0:0:7f00:1", // Teredo whose server is 127.0.0.1
        ] {
            assert!(!is_public(ip(hidden)), "{hidden} should not be reachable");
        }
        // The prefixes are refused whatever they carry: 6to4 and Teredo are not addresses to
        // fetch from in their own right.
        for prefix in ["2002:0808:0808::", "2001:0:4136:e378:8000:63bf:3fff:fdd2"] {
            assert!(!is_public(ip(prefix)), "{prefix} should not be reachable");
        }
    }

    #[test]
    fn the_ietf_protocol_block_is_a_slash_24_and_not_a_slash_16() {
        // 192.0.0.0/24 is reserved; 192.0.1.0 and up are ordinary public addresses, and
        // refusing them would be this tool quietly failing on real hosts.
        assert!(!is_public(ip("192.0.0.1")));
        assert!(!is_public(ip("192.0.0.255")));
        assert!(is_public(ip("192.0.1.1")));
        assert!(is_public(ip("192.0.128.7")));
        // 192.0.2.0/24 is documentation and stays refused, by the other rule.
        assert!(!is_public(ip("192.0.2.5")));
    }

    #[test]
    fn the_public_internet_is_public() {
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.0.1",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001:800::200e",
        ] {
            assert!(is_public(ip(public)), "{public} should be reachable");
        }
    }

    #[tokio::test]
    async fn an_address_literal_needs_no_dns_to_be_refused() {
        let url = parse("http://127.0.0.1:8080/admin").unwrap();
        let err = check_address(&url).await.unwrap_err();
        assert!(matches!(err, Refusal::Private { .. }));
        assert!(err.to_string().contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn the_metadata_service_is_refused_by_name_or_by_number() {
        let url = parse("http://169.254.169.254/latest/meta-data/").unwrap();
        assert!(matches!(
            check_address(&url).await,
            Err(Refusal::Private { .. })
        ));
    }
}
