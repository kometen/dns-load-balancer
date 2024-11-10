use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::serialize::binary::BinDecodable;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

lazy_static! {
    static ref TLS_CONNECTIONS: Mutex<HashMap<String, Arc<TlsConnector>>> =
        Mutex::new(HashMap::new());
}

async fn get_tls_connector(dns_server: &str) -> Arc<TlsConnector> {
    let mut connections: tokio::sync::MutexGuard<'_, HashMap<String, Arc<TlsConnector>>> =
        TLS_CONNECTIONS.lock().await;

    if let Some(connector) = connections.get(dns_server) {
        connector.clone()
    } else {
        let mut root_cert_store = RootCertStore::empty();
        root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();

        let connector = Arc::new(TlsConnector::from(Arc::new(config)));
        connections.insert(dns_server.to_string(), connector.clone());
        connector
    }
}
pub async fn query_dns_tls(
    dns_server: &str,
    query_data: Vec<u8>,
) -> io::Result<(String, Option<Vec<u8>>)> {
    let addr = format!("{}:853", dns_server);
    let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to resolve DNS server")
    })?;

    let connector = get_tls_connector(dns_server).await;

    // Connect using TLS
    let stream = TcpStream::connect(addr).await?;
    let domain = ServerName::try_from(dns_server.to_owned())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;

    let mut tls_stream = connector.connect(domain, stream).await?;

    // DNS over TLS requires a 2-byte prefix
    let length = (query_data.len() as u16).to_be_bytes();
    tls_stream.write_all(&length).await?;
    tls_stream.write_all(&query_data).await?;

    // Read response length
    let mut length_buf = [0u8; 2];
    tls_stream.read_exact(&mut length_buf).await?;
    let response_length = u16::from_be_bytes(length_buf) as usize;

    // Read response
    let mut response_buf = vec![0; response_length];
    tls_stream.read_exact(&mut response_buf).await?;

    if let Ok(message) = Message::from_bytes(&response_buf) {
        if message.response_code() == ResponseCode::NoError && !message.answers().is_empty() {
            return Ok((dns_server.to_string(), Some(response_buf)));
        }
    }

    Ok((dns_server.to_string(), None))
}

pub async fn query_dns(
    dns_server: &str,
    query_data: Vec<u8>,
) -> io::Result<(String, Option<Vec<u8>>)> {
    let addr = format!("{}:53", dns_server);
    let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Failed to resolve DNS server")
    })?;

    let upstream = UdpSocket::bind("0.0.0.0:0").await?;
    upstream.connect(addr).await?;

    let mut response_buf = vec![0; 1024];

    upstream.send(&query_data).await?;
    upstream.recv(&mut response_buf).await?;

    if let Ok(message) = Message::from_bytes(&response_buf) {
        if message.response_code() == ResponseCode::NoError && !message.answers().is_empty() {
            return Ok((dns_server.to_string(), Some(response_buf)));
        }
    }

    Ok((dns_server.to_string(), None))
}
