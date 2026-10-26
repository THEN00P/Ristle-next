//! Where a request may go.
//!
//! A [`Reach`] is built by the caller from what the user consented to: the
//! host patterns of a Plugin manifest (`hosts`, `media-hosts` or
//! `image-hosts`), reached over HTTPS at public addresses only, and the
//! origins of a Provider Connection's Server Grant, which may use plain HTTP
//! and the local-network or same-device addresses the user confirmed
//! (`plugin-api/wit/host.wit`, `http`). The client checks every URL against
//! it before sending, on every redirect too, and the connector checks every
//! address it connects to.

use std::net::IpAddr;
use std::sync::Arc;

use url::Url;

use crate::Error;
use crate::address;

/// A host pattern from a manifest: `api.example.com` matches that host only,
/// `*.example.com` matches every subdomain of `example.com` but not
/// `example.com` itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostPattern {
    /// Lowercase. For a wildcard, the suffix including its leading dot.
    host: String,
    wildcard: bool,
}

impl HostPattern {
    /// Parses a manifest host pattern.
    ///
    /// Patterns are ASCII (internationalized names are written in their
    /// `xn--` form), have no scheme, port, path or trailing dot, and a
    /// wildcard covers at least two labels (`*.example.com`, never `*.com`
    /// or `*`).
    pub fn parse(pattern: &str) -> Result<Self, InvalidPattern> {
        let invalid = || InvalidPattern(pattern.to_owned());
        let lower = pattern.to_ascii_lowercase();
        let (wildcard, host) = match lower.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, lower.as_str()),
        };
        if host.is_empty()
            || host.len() > 253
            || !host.is_ascii()
            || host.ends_with('.')
            || host.starts_with('.')
            || host.contains("..")
        {
            return Err(invalid());
        }
        // Validate as a URL host so the comparison form matches what `url`
        // produces for request URLs.
        let parsed = url::Host::parse(host).map_err(|_| invalid())?;
        let normalized = parsed.to_string();
        let plain_name = |name: &str| {
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        };
        if normalized != host || matches!(&parsed, url::Host::Domain(name) if !plain_name(name)) {
            return Err(invalid());
        }
        if wildcard {
            if !matches!(parsed, url::Host::Domain(_)) || !host.contains('.') {
                return Err(invalid());
            }
            return Ok(Self {
                host: format!(".{host}"),
                wildcard: true,
            });
        }
        Ok(Self {
            host: normalized,
            wildcard: false,
        })
    }

    /// Whether `host` (as `url` formats it: lowercase, IPv6 in brackets)
    /// matches.
    pub fn matches(&self, host: &str) -> bool {
        if self.wildcard {
            host.len() > self.host.len() && host.ends_with(&self.host)
        } else {
            host == self.host
        }
    }

    /// Whether every host `other` matches is matched by this pattern too.
    pub fn covers(&self, other: &HostPattern) -> bool {
        match (self.wildcard, other.wildcard) {
            (false, false) => self.host == other.host,
            (false, true) => false,
            // `other.host` is `a.example.com` or `.a.example.com`.
            (true, _) => {
                other.host.ends_with(&self.host)
                    && (other.wildcard || other.host.len() > self.host.len())
            }
        }
    }

    /// Whether it is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        self.wildcard
    }
}

impl std::fmt::Display for HostPattern {
    /// The pattern as a manifest writes it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.wildcard {
            write!(f, "*{}", self.host)
        } else {
            f.write_str(&self.host)
        }
    }
}

/// A host pattern that is not valid.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid host pattern {0:?}")]
pub struct InvalidPattern(pub String);

/// The scheme of a granted origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// Plain HTTP, allowed only for Server Grant origins.
    Http,
    /// HTTPS.
    Https,
}

impl Scheme {
    fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// One origin of a Server Grant and what the user confirmed for it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GrantedOrigin {
    /// The scheme.
    pub scheme: Scheme,
    /// The host, as `url` formats it.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The server resolved to a public address when the user confirmed it,
    /// so any public address is fine (public addresses change routinely).
    pub public: bool,
    /// The local-network or same-device addresses the user confirmed.
    pub confirmed: Vec<IpAddr>,
}

impl GrantedOrigin {
    /// The origin of `url` with nothing confirmed yet.
    pub fn of(url: &Url) -> Result<Self, Error> {
        let scheme = match url.scheme() {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            other => return Err(Error::NotAllowed(format!("scheme {other}"))),
        };
        let host = url
            .host_str()
            .ok_or_else(|| Error::NotAllowed("URL has no host".to_owned()))?
            .to_owned();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| Error::NotAllowed("URL has no port".to_owned()))?;
        Ok(Self {
            scheme,
            host,
            port,
            public: false,
            confirmed: Vec::new(),
        })
    }

    /// Whether this origin may use `address`.
    pub fn allows(&self, address: IpAddr) -> bool {
        let address = address::canonical(address);
        (self.public && address::is_public(address))
            || self
                .confirmed
                .iter()
                .any(|confirmed| address::canonical(*confirmed) == address)
    }

    fn is(&self, scheme: &str, host: &str, port: u16) -> bool {
        self.scheme.as_str() == scheme && self.host == host && self.port == port
    }
}

/// The destinations one request may reach.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Reach {
    hosts: Vec<HostPattern>,
    granted: Vec<GrantedOrigin>,
}

impl Reach {
    /// Reaches nothing.
    pub fn none() -> Self {
        Self::default()
    }

    /// HTTPS to hosts matching `patterns`, at public addresses.
    pub fn hosts(patterns: Vec<HostPattern>) -> Self {
        Self {
            hosts: patterns,
            granted: Vec::new(),
        }
    }

    /// Adds the origins of a Server Grant.
    pub fn with_granted(mut self, origins: Vec<GrantedOrigin>) -> Self {
        self.granted.extend(origins);
        self
    }

    /// The granted origins, which decide which addresses the connector
    /// accepts.
    pub(crate) fn granted(&self) -> &[GrantedOrigin] {
        &self.granted
    }

    /// Checks `url` before anything is sent to it.
    pub(crate) fn check(&self, url: &Url) -> Result<(), Error> {
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::NotAllowed("URL carries credentials".to_owned()));
        }
        let host = url
            .host_str()
            .ok_or_else(|| Error::NotAllowed("URL has no host".to_owned()))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| Error::NotAllowed(format!("scheme {}", url.scheme())))?;
        if self
            .granted
            .iter()
            .any(|origin| origin.is(url.scheme(), host, port))
        {
            return Ok(());
        }
        if url.scheme() != "https" {
            return Err(Error::NotAllowed(format!(
                "{} is not HTTPS",
                origin_text(url)
            )));
        }
        if self.hosts.iter().any(|pattern| pattern.matches(host)) {
            return Ok(());
        }
        Err(Error::NotAllowed(format!("{host} is not a declared host")))
    }
}

/// `scheme://host:port`, for messages. Never includes a path or query, which
/// may carry tokens.
pub(crate) fn origin_text(url: &Url) -> String {
    match (url.host_str(), url.port_or_known_default()) {
        (Some(host), Some(port)) => format!("{}://{host}:{port}", url.scheme()),
        _ => url.scheme().to_owned(),
    }
}

/// Two URLs share an origin.
pub(crate) fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// The granted origins of a request, shared by every hyper client built for
/// that set.
pub(crate) type Granted = Arc<[GrantedOrigin]>;

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn reach(patterns: &[&str]) -> Reach {
        Reach::hosts(
            patterns
                .iter()
                .map(|pattern| HostPattern::parse(pattern).unwrap())
                .collect(),
        )
    }

    #[test]
    fn patterns_match_hosts_and_subdomains() {
        let exact = HostPattern::parse("API.Example.com").unwrap();
        assert!(exact.matches("api.example.com"));
        assert!(!exact.matches("x.api.example.com"));
        let wildcard = HostPattern::parse("*.sndcdn.com").unwrap();
        assert!(wildcard.matches("i1.sndcdn.com"));
        assert!(wildcard.matches("a.b.sndcdn.com"));
        assert!(!wildcard.matches("sndcdn.com"));
        assert!(!wildcard.matches("evilsndcdn.com"));
    }

    #[test]
    fn patterns_cover_the_patterns_they_include() {
        let p = |text: &str| HostPattern::parse(text).unwrap();
        assert!(p("*.example.com").covers(&p("a.example.com")));
        assert!(p("*.example.com").covers(&p("*.a.example.com")));
        assert!(p("*.example.com").covers(&p("*.example.com")));
        assert!(!p("*.example.com").covers(&p("example.com")));
        assert!(!p("*.example.com").covers(&p("aexample.com")));
        assert!(!p("a.example.com").covers(&p("*.a.example.com")));
        assert!(p("a.example.com").covers(&p("A.example.com")));
        assert!(!p("a.example.com").covers(&p("b.example.com")));
        assert_eq!(p("*.Example.com").to_string(), "*.example.com");
        assert_eq!(p("api.example.com").to_string(), "api.example.com");
    }

    #[test]
    fn malformed_patterns_are_refused() {
        for pattern in [
            "",
            "*",
            "*.com",
            "*.",
            "example.com.",
            ".example.com",
            "a..b",
            "https://example.com",
            "example.com:443",
            "example.com/path",
            "exämple.com",
            "*.1.2.3.4",
            "ex ample.com",
        ] {
            assert!(
                HostPattern::parse(pattern).is_err(),
                "{pattern:?} must be refused"
            );
        }
        assert!(HostPattern::parse("xn--exmple-cua.com").is_ok());
        assert!(HostPattern::parse("1.2.3.4").is_ok());
    }

    #[test]
    fn manifest_hosts_need_https() {
        let reach = reach(&["api.example.com"]);
        assert!(reach.check(&url("https://api.example.com/x?y")).is_ok());
        assert!(reach.check(&url("https://api.example.com:8443/x")).is_ok());
        assert!(matches!(
            reach.check(&url("http://api.example.com/x")),
            Err(Error::NotAllowed(_))
        ));
        assert!(matches!(
            reach.check(&url("https://other.example.com/x")),
            Err(Error::NotAllowed(_))
        ));
        assert!(matches!(
            reach.check(&url("ftp://api.example.com/x")),
            Err(Error::NotAllowed(_))
        ));
    }

    #[test]
    fn urls_with_credentials_are_refused() {
        let reach = reach(&["api.example.com"]);
        assert!(reach.check(&url("https://user@api.example.com/")).is_err());
        assert!(reach.check(&url("https://u:p@api.example.com/")).is_err());
    }

    #[test]
    fn granted_origins_allow_plain_http_on_their_exact_origin() {
        let origin = GrantedOrigin::of(&url("http://nas.local:8096/")).unwrap();
        let reach = Reach::none().with_granted(vec![origin]);
        assert!(reach.check(&url("http://nas.local:8096/Items")).is_ok());
        assert!(reach.check(&url("http://nas.local:8097/Items")).is_err());
        assert!(reach.check(&url("https://nas.local:8096/Items")).is_err());
        assert!(reach.check(&url("http://other.local:8096/")).is_err());
    }

    #[test]
    fn granted_addresses_are_exact_unless_public() {
        let mut origin = GrantedOrigin::of(&url("http://nas.local:8096/")).unwrap();
        origin.confirmed = vec!["192.168.1.20".parse().unwrap()];
        assert!(origin.allows("192.168.1.20".parse().unwrap()));
        assert!(origin.allows("::ffff:192.168.1.20".parse().unwrap()));
        assert!(!origin.allows("192.168.1.21".parse().unwrap()));
        assert!(!origin.allows("8.8.8.8".parse().unwrap()));
        origin.public = true;
        assert!(origin.allows("8.8.8.8".parse().unwrap()));
        assert!(!origin.allows("10.0.0.1".parse().unwrap()));
    }
}
