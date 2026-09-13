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
    let [a, b, ..] = ip.octets();
    !(ip.is_private()            // 10/8, 172.16/12, 192.168/16
        || ip.is_loopback()      // 127/8
        || ip.is_link_local()    // 169.254/16, the cloud metadata service among it
        || ip.is_broadcast()     // 255.255.255.255
        || ip.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || ip.is_unspecified()   // 0.0.0.0, which means "this host" to a connect()
        || ip.is_multicast()
        || a == 0                    // 0/8, "this network"
        || (a == 100 && (64..128).contains(&b))  // 100.64/10 carrier-grade NAT
        || (a == 192 && b == 0)      // 192.0.0/24 IETF protocol assignments
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
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00   // fc00::/7 unique local
        || (segments[0] & 0xffc0) == 0xfe80   // fe80::/10 link local
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)) // 2001:db8::/32 documentation
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
