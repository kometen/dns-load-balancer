use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::serialize::binary::BinDecodable;
use tokio::{io, net::UdpSocket};

use crate::config;

pub async fn query_dns(
    dns_server: &str,
    query_data: Vec<u8>,
) -> io::Result<(String, Option<Vec<u8>>)> {
    let upstream = UdpSocket::bind("0.0.0.0:0").await?;
    upstream.connect(dns_server).await?;

    let timeout = tokio::time::Duration::from_secs(config::DNS_TIMEOUT);
    upstream.send(&query_data).await?;

    let mut response_buf = vec![0; 1024];
    match tokio::time::timeout(timeout, upstream.recv(&mut response_buf)).await {
        Ok(Ok(size)) => {
            if let Ok(message) = Message::from_bytes(&response_buf[..size]) {
                if message.response_code() == ResponseCode::NoError && !message.answers().is_empty()
                {
                    println!("{} returned a positive response", dns_server);
                    Ok((dns_server.to_string(), Some(response_buf[..size].to_vec())))
                } else {
                    println!("{} returned no results", dns_server);
                    Ok((dns_server.to_string(), None))
                }
            } else {
                println!("{} returned invalid DNS message", dns_server);
                Ok((dns_server.to_string(), None))
            }
        }
        Ok(Err(e)) => {
            println!("{} query failed: {}", dns_server, e);
            Ok((dns_server.to_string(), None))
        }
        Err(_) => {
            println!("{} timed out", dns_server);
            Ok((dns_server.to_string(), None))
        }
    }
}
