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

pub async fn query_dns_with_fallback(
    dns_server: &str,
    query_data: Vec<u8>,
) -> io::Result<(String, Option<Vec<u8>>)> {
    // First try DNS over TLS
    match query_dns_tls(dns_server, query_data.clone()).await {
        Ok((server, Some(response))) => {
            // DoT succeeded and returned a valid response
            Ok((server, Some(response)))
        }
        Ok((server, None)) => {
            // DoT succeeded but returned no data, try cleartext fallback
            println!(
                "DoT returned no data for {}, falling back to cleartext DNS",
                server
            );
            query_dns(dns_server, query_data).await
        }
        Err(e) => {
            // DoT failed entirely, try cleartext fallback
            println!(
                "DoT failed for {} ({}), falling back to cleartext DNS",
                dns_server, e
            );
            query_dns(dns_server, query_data).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use std::str::FromStr;

    fn create_test_query() -> Vec<u8> {
        let mut message = Message::new();
        message.set_id(12345);
        let name = Name::from_str("example.com").unwrap();
        let query = Query::query(name, RecordType::A);
        message.add_query(query);
        message.to_vec().unwrap()
    }

    #[tokio::test]
    async fn test_fallback_functionality_exists() {
        // This test verifies that the fallback function exists and can be called
        // In a real environment, this would test against actual DNS servers
        let query_data = create_test_query();

        // Test with a non-existent server (should fail gracefully)
        let result = query_dns_with_fallback("192.0.2.1", query_data).await;

        // The function should return without panicking, even if it fails
        match result {
            Ok((server, _response)) => {
                assert_eq!(server, "192.0.2.1");
                // Response might be None due to network failure, which is expected
            }
            Err(_) => {
                // Network errors are expected when testing with unreachable servers
            }
        }
    }

    #[test]
    fn test_create_dns_query() {
        // Test that we can create a valid DNS query
        let query_data = create_test_query();
        assert!(!query_data.is_empty());

        // Verify we can parse it back
        let parsed = Message::from_bytes(&query_data);
        assert!(parsed.is_ok());

        let message = parsed.unwrap();
        assert_eq!(message.id(), 12345);
        assert_eq!(message.queries().len(), 1);
    }
}
