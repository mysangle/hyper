use std::fmt;
use std::io::{self, Read, Write};

use super::connect::HttpConnector;
use futures::{Future, Poll};
use openssl::ssl::{SslConnectorBuilder, SslMethod};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio::reactor::Handle;
use tokio::net::TcpStream;
use tokio_openssl::{SslConnectorExt, SslStream};
use tokio_service::Service;
use Uri;

/// A connector for the `https` scheme.
#[derive(Clone)]
pub struct HttpsConnector {
    http: HttpConnector,
}

impl HttpsConnector {
    /// constructs a new HttpsConnector.
    pub fn new(threads: usize, handle: &Handle) -> HttpsConnector {
        HttpsConnector {
            http: HttpConnector::new(threads, handle),
        }
    }
}

impl fmt::Debug for HttpsConnector {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("HttpsConnector")
            .finish()
    }
}

impl Service for HttpsConnector {
    type Request = Uri;
    type Response = MaybeHttpsStream;
    type Error = io::Error;
    type Future = HttpsConnecting;

    fn call(&self, uri: Uri) -> Self::Future {
        let is_https = uri.scheme() == Some("https");
        let host = match uri.host() {
            Some(host) => host.to_owned(),
            None => return HttpsConnecting(
                Box::new(
                    ::futures::future::err(
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid url, missing host"
                        )
                    )
                )
            ),
        };
        let connecting = self.http.call(uri);

        HttpsConnecting(if is_https {
            Box::new(connecting.and_then(move |tcp| {
                SslConnectorBuilder::new(SslMethod::tls())
                    .map(|builder| builder.build())
                    .map(|connector| connector.connect_async(&host, tcp))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }).and_then(|maybe_tls| {
                maybe_tls.map(|tls| MaybeHttpsStream::Https(tls))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }))
        } else {
            Box::new(connecting.map(|tcp| MaybeHttpsStream::Http(tcp)))
        })
    }
}

/// a future representing work to connect to a URL, and a TLS handshake.
pub struct HttpsConnecting(Box<Future<Item=MaybeHttpsStream, Error=io::Error>>);

impl Future for HttpsConnecting {
    type Item = MaybeHttpsStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

impl fmt::Debug for HttpsConnecting {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad("HttpsConnecting")
    }
}

/// a stream that might be protected with TLS.
pub enum MaybeHttpsStream {
    /// a stream over plain text.
    Http(TcpStream),
    /// a stream protected with TLS.
    Https(SslStream<TcpStream>),
}

impl fmt::Debug for MaybeHttpsStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            MaybeHttpsStream::Http(..) => f.pad("Http(..)"),
            MaybeHttpsStream::Https(..) => f.pad("Https(..)"),
        }
    }
}

impl Read for MaybeHttpsStream {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            MaybeHttpsStream::Http(ref mut s) => s.read(buf),
            MaybeHttpsStream::Https(ref mut s) => s.read(buf),
        }
    }
}

impl Write for MaybeHttpsStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match *self {
            MaybeHttpsStream::Http(ref mut s) => s.write(buf),
            MaybeHttpsStream::Https(ref mut s) => s.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match *self {
            MaybeHttpsStream::Http(ref mut s) => s.flush(),
            MaybeHttpsStream::Https(ref mut s) => s.flush(),
        }
    }
}

impl AsyncRead for MaybeHttpsStream {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match *self {
            MaybeHttpsStream::Http(ref s) => s.prepare_uninitialized_buffer(buf),
            MaybeHttpsStream::Https(ref s) => s.prepare_uninitialized_buffer(buf),
        }
    }
}

impl AsyncWrite for MaybeHttpsStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match *self {
            MaybeHttpsStream::Http(ref mut s) => s.shutdown(),
            MaybeHttpsStream::Https(ref mut s) => s.shutdown(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use tokio::reactor::Core;
    use client::connect::Connect;
    use super::HttpsConnector;

    #[test]
    fn test_non_https_url() {
        let mut core = Core::new().unwrap();
        let url = "/foo/bar?baz".parse().unwrap();
        let connector = HttpsConnector::new(1, &core.handle());

        assert_eq!(core.run(connector.connect(url)).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
