use crate::FINGER_PORT;
use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::str::FromStr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

#[cfg(all(unix, feature = "unix-socket"))]
mod unix {
    pub use std::path::PathBuf;
    pub use tokio::net::unix::*;
    pub use tokio::net::{UnixListener, UnixStream};
}

#[derive(Clone, Debug)]
pub enum AnySocketAddr {
    Tcp(SocketAddr),

    #[cfg(all(unix, feature = "unix-socket"))]
    Unix(unix::PathBuf),
}

impl From<SocketAddr> for AnySocketAddr {
    fn from(value: SocketAddr) -> Self {
        Self::Tcp(value)
    }
}

impl TryFrom<&OsStr> for AnySocketAddr {
    type Error = AddrParseError;

    fn try_from(value: &OsStr) -> Result<Self, Self::Error> {
        #[cfg(all(unix, feature = "unix-socket"))]
        {
            use std::os::unix::ffi::OsStrExt;

            let bytes = value.as_bytes();
            if bytes.starts_with(b"/") || bytes.starts_with(b"./") || bytes.starts_with(b"../") {
                return Ok(Self::Unix(unix::PathBuf::from(value)));
            }
        }

        let str = value
            .to_str()
            .ok_or_else(|| SocketAddr::from_str("").unwrap_err())?;

        let socket_addr = SocketAddr::from_str(str)
            .or_else(|_| IpAddr::from_str(str).map(|addr| SocketAddr::new(addr, FINGER_PORT)));

        socket_addr.map(Self::Tcp)
    }
}

impl Display for AnySocketAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(addr) => Display::fmt(&addr, f),
            #[cfg(all(unix, feature = "unix-socket"))]
            Self::Unix(path) => Display::fmt(&path.display(), f),
        }
    }
}

/// A TCP or Unix listener (abstracted away)
pub enum AnyListener {
    Tcp(TcpListener),

    #[cfg(all(unix, feature = "unix-socket"))]
    Unix(unix::UnixListener),
}

impl From<TcpListener> for AnyListener {
    fn from(value: TcpListener) -> Self {
        Self::Tcp(value)
    }
}

impl AnyListener {
    pub async fn bind(addr: impl Borrow<AnySocketAddr>) -> std::io::Result<Self> {
        match addr.borrow() {
            AnySocketAddr::Tcp(addr) => TcpListener::bind(addr).await.map(Self::Tcp),
            #[cfg(all(unix, feature = "unix-socket"))]
            AnySocketAddr::Unix(path) => unix::UnixListener::bind(path).map(Self::Unix),
        }
    }

    pub async fn accept(&self) -> std::io::Result<AnySocket> {
        match self {
            Self::Tcp(listener) => listener
                .accept()
                .await
                .map(|(sock, addr)| AnySocket::Tcp(sock, addr)),

            #[cfg(all(unix, feature = "unix-socket"))]
            Self::Unix(listener) => listener
                .accept()
                .await
                .map(|(sock, _)| AnySocket::Unix(sock)),
        }
    }
}

pub enum AnySocket {
    Tcp(TcpStream, SocketAddr),

    #[cfg(all(unix, feature = "unix-socket"))]
    Unix(unix::UnixStream),
}

impl AnySocket {
    pub fn peer_display(&self) -> impl Display + Sync + Send + 'static {
        enum PeerDisplay {
            Tcp(SocketAddr),
            Unix,
        }

        impl Display for PeerDisplay {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Tcp(addr) => write!(f, "{addr}"),
                    Self::Unix => write!(f, "unix"),
                }
            }
        }

        match self {
            AnySocket::Tcp(_, addr) => PeerDisplay::Tcp(*addr),
            AnySocket::Unix(_) => PeerDisplay::Unix,
        }
    }

    pub fn split(&mut self) -> AnySplitSocket {
        match self {
            AnySocket::Tcp(sock, _) => AnySplitSocket::Tcp(sock.split()),
            #[cfg(all(unix, feature = "unix-socket"))]
            AnySocket::Unix(sock) => AnySplitSocket::Unix(sock.split()),
        }
    }
}

pub enum AnySplitSocket<'a> {
    Tcp((ReadHalf<'a>, WriteHalf<'a>)),

    #[cfg(all(unix, feature = "unix-socket"))]
    Unix((unix::ReadHalf<'a>, unix::WriteHalf<'a>)),
}

impl<'a> AnySplitSocket<'a> {
    pub fn as_parts(
        &mut self,
    ) -> (
        &mut (dyn AsyncRead + Send + Unpin),
        &mut (dyn AsyncWrite + Send + Unpin),
    ) {
        match self {
            Self::Tcp((r, w)) => (r, w),
            #[cfg(all(unix, feature = "unix-socket"))]
            Self::Unix((r, w)) => (r, w),
        }
    }
}
