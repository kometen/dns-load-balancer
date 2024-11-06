use hickory_proto::op::{Message, MessageType};
use hickory_proto::serialize::binary::BinDecodable;
use std::net::{SocketAddr, UdpSocket};

const PUBLIC_DNS: &str = "1.1.1.1:53"; // Cloudflare
const KUBERNETES_DNS: &str = "10.152.183.10:53"; // kubernetes
const LOCALHOST_PORT: &str = "127.0.0.1:53";

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(LOCALHOST_PORT)?;
    println!("DNS forwarder listening on {}", LOCALHOST_PORT);

    let mut buf = vec![0; 512]; // Standard DNS message size

    loop {
        match socket.recv_from(&mut buf) {
            Ok((size, src)) => {
                println!("Received query from {}", src);

                // Parse incoming DNS query
                if let Ok(dns_message) = Message::from_bytes(&buf[..size]) {
                    handle_dns_query(&socket, dns_message, src)?;
                }
            }
            Err(e) => eprintln!("Error receiving data: {}", e),
        }
    }
}

fn handle_dns_query(socket: &UdpSocket, query: Message, src: SocketAddr) -> std::io::Result<()> {
    // Only process DNS queries
    if query.message_type() != MessageType::Query {
        return Ok(());
    }

    // Create upstream socket
    let upstream = UdpSocket::bind("0.0.0.0:0")?;

    // Forward query to both DNS servers and use first response
    let encoded_query = query.to_vec()?;

    // Try kubernetes DNS first
    upstream.send_to(&encoded_query, KUBERNETES_DNS)?;
    let mut response_buf = vec![0; 512];
    upstream.set_read_timeout(Some(std::time::Duration::from_secs(1)))?;

    match upstream.recv_from(&mut response_buf) {
        Ok((size, _)) => {
            // Forward the response back to the client
            socket.send_to(&response_buf[..size], src)?;
        }
        Err(_) => {
            // If kubernetes DNS fails, try public DNS
            upstream.send_to(&encoded_query, PUBLIC_DNS)?;
            if let Ok((size, _)) = upstream.recv_from(&mut response_buf) {
                socket.send_to(&response_buf[..size], src)?;
            }
        }
    }

    Ok(())
}
