//! The TCP connector under every connection the client makes.
//!
//! It resolves the name itself, keeps only the addresses the destination
//! may use (public ones, or for a Server Grant origin what the user
//! confirmed), and checks the peer of the connection it got once more. Every
//! new connection resolves again, so a name rebound to a private address
//! gains nothing. IP literals in URLs take the same path.

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use http::Uri;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::address;
use crate::reach::Granted;

/// How long one TCP connection attempt may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A connection was refused because of where it would lead.
#[derive(Debug)]
pub(crate) struct Refused {
    pub(crate) origin: String,
    pub(crate) granted: bool,
    pub(crate) addresses: Vec<IpAddr>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} resolved to addresses that may not be reached",
            self.origin
        )
    }
}

impl std::error::Error for Refused {}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
pub(crate) struct GuardedConnector {
    granted: Granted,
}

impl GuardedConnector {
    pub(crate) fn new(granted: Granted) -> Self {
        Self { granted }
    }
}

impl tower_service::Service<Uri> for GuardedConnector {
    type Response = TokioIo<TcpStream>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let granted = self.granted.clone();
        Box::pin(async move { connect(&granted, &uri).await.map(TokioIo::new) })
    }
}

async fn connect(granted: &Granted, uri: &Uri) -> Result<TcpStream, BoxError> {
    let scheme = uri.scheme_str().unwrap_or("");
    let host = uri
        .host()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "URI has no host"))?;
    let port = uri.port_u16().unwrap_or(match scheme {
        "https" => 443,
        _ => 80,
    });
    let origin = granted
        .iter()
        .find(|origin| origin.host == host && origin.port == port && scheme_is(origin, scheme));
    connect_checked(host, port, origin, &format!("{scheme}://{host}:{port}")).await
}

/// Resolves `host`, keeps the addresses `origin` allows (public ones without
/// an origin), connects to the first that answers and checks the peer.
async fn connect_checked(
    host: &str,
    port: u16,
    origin: Option<&crate::GrantedOrigin>,
    label: &str,
) -> Result<TcpStream, BoxError> {
    let allows = |address: IpAddr| match origin {
        Some(origin) => origin.allows(address),
        None => address::is_public(address),
    };
    // `Uri::host` keeps IPv6 literals in brackets; the resolver wants them bare.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let resolved: Vec<SocketAddr> =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((bare, port)))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "name resolution timed out"))??
            .collect();
    let allowed: Vec<SocketAddr> = resolved
        .iter()
        .copied()
        .filter(|socket| allows(socket.ip()))
        .collect();
    if allowed.is_empty() {
        let mut addresses: Vec<IpAddr> = resolved.iter().map(|socket| socket.ip()).collect();
        addresses.dedup();
        return Err(Box::new(Refused {
            origin: label.to_owned(),
            granted: origin.is_some(),
            addresses,
        }));
    }
    let mut last_error = None;
    for socket in allowed {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(socket)).await {
            Ok(Ok(stream)) => {
                // The connection must lead where the check said.
                let peer = stream.peer_addr()?;
                if !allows(peer.ip()) {
                    return Err(Box::new(Refused {
                        origin: label.to_owned(),
                        granted: origin.is_some(),
                        addresses: vec![peer.ip()],
                    }));
                }
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Ok(Err(error)) => last_error = Some(error),
            Err(_) => {
                last_error = Some(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"));
            }
        }
    }
    Err(Box::new(last_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "no address to connect to")
    })))
}

fn scheme_is(origin: &crate::GrantedOrigin, scheme: &str) -> bool {
    matches!(
        (origin.scheme, scheme),
        (crate::Scheme::Http, "http") | (crate::Scheme::Https, "https")
    )
}

/// A raw TCP connection for the Plugin host's `socket` import: the name is
/// resolved here, only public addresses are used (or what `origin`
/// confirms), and the peer is checked once connected. Every new connection
/// resolves again.
pub(crate) async fn tcp_connect(
    host: &str,
    port: u16,
    origin: Option<&crate::GrantedOrigin>,
) -> Result<TcpStream, crate::Error> {
    let label = format!("tcp://{host}:{port}");
    connect_checked(host, port, origin, &label)
        .await
        .map_err(|error| match error.downcast::<Refused>() {
            Ok(refused) => crate::Error::AddressRefused {
                origin: refused.origin,
                granted: refused.granted,
                addresses: refused.addresses,
            },
            Err(error) => crate::Error::Network(error.to_string()),
        })
}
