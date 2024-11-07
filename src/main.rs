use futures::future::join_all;
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::serialize::binary::BinDecodable;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io;
use tokio::net::UdpSocket;

const DNS_SERVERS: &[&str] = &[
    "10.152.183.10:53", // kubernetes
    "1.1.1.1:53",       // Cloudflare
    "8.8.8.8:53",       // Google
];

const LOCALHOST_PORT: &str = "127.0.0.1:53";

struct Server {
    socket: Arc<UdpSocket>,
    buf_size: usize,
}

impl Server {
    async fn query_dns(
        dns_server: &str,
        query_data: Vec<u8>,
    ) -> io::Result<(String, Option<Vec<u8>>)> {
        let upstream = UdpSocket::bind("0.0.0.0:0").await?;
        upstream.connect(dns_server).await?;

        let timeout = tokio::time::Duration::from_secs(3);
        upstream.send(&query_data).await?;

        let mut response_buf = vec![0; 1024];
        match tokio::time::timeout(timeout, upstream.recv(&mut response_buf)).await {
            Ok(Ok(size)) => {
                if let Ok(message) = Message::from_bytes(&response_buf[..size]) {
                    if message.response_code() == ResponseCode::NoError
                        && !message.answers().is_empty()
                    {
                        println!("{} returned a positive response", dns_server);
                        Ok((dns_server.to_string(), Some(response_buf[..size].to_vec())))
                    } else {
                        println!("{} returned no results (NXDOMAIN or empty", dns_server);
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

    async fn handle_request(
        socket: Arc<UdpSocket>,
        buf: Vec<u8>,
        size: usize,
        peer: SocketAddr,
    ) -> io::Result<()> {
        println!("Handling request from {}", peer);

        match Message::from_bytes(&buf[..size]) {
            Ok(query) => match query.to_vec() {
                Ok(encoded_query) => {
                    let futures: Vec<_> = DNS_SERVERS
                        .iter()
                        .map(|&dns_server| {
                            let query_data = encoded_query.clone();
                            Box::pin(Server::query_dns(dns_server, query_data))
                        })
                        .collect();

                    let results = join_all(futures).await;
                    let first_positive_response = results
                        .into_iter()
                        .filter_map(|result| result.ok())
                        .find_map(|(server, maybe_response)| {
                            maybe_response.map(|response| (server, response))
                        });

                    match first_positive_response {
                        Some((server, response_data)) => {
                            println!("First response from {}", server);
                            socket.send_to(&response_data, peer).await?;
                            println!("Response sent to {}", peer);
                        }
                        None => {
                            println!(
                                "No positive response received, sending last NXDOMAIN response"
                            );
                            let mut msg = Message::new();
                            msg.set_response_code(ResponseCode::NXDomain);
                            msg.set_message_type(MessageType::Response);
                            if let Ok(response_data) = msg.to_vec() {
                                socket.send_to(&response_data, peer).await?;
                            }
                        }
                    }
                }
                Err(e) => eprintln!("Error encoding query: {}", e),
            },
            Err(e) => eprintln!("Error parsing DNS message: {}", e),
        }

        Ok(())
    }

    async fn run(self) -> Result<(), io::Error> {
        let Server { socket, buf_size } = self;

        loop {
            let mut buf = vec![0; buf_size];
            let (size, peer) = socket.recv_from(&mut buf).await?;
            let socket_clone = Arc::clone(&socket);

            tokio::spawn(async move {
                if let Err(e) = Server::handle_request(socket_clone, buf, size, peer).await {
                    eprintln!("Error handling request: {}", e);
                }
            });
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let socket = UdpSocket::bind(LOCALHOST_PORT).await?;
    println!("DNS forwarder listening on {}", socket.local_addr()?);

    let server = Server {
        socket: Arc::new(socket),
        buf_size: 1024,
    };

    server.run().await?;

    Ok(())
}
