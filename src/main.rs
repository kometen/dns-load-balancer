use futures::future::select_all;
use hickory_proto::op::Message;
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
    async fn query_dns(dns_server: &str, query_data: Vec<u8>) -> io::Result<(String, Vec<u8>)> {
        let upstream = UdpSocket::bind("0.0.0.0:0").await?;
        upstream.connect(dns_server).await?;
        upstream.send(&query_data).await?;

        let mut response_buf = vec![0; 1024];
        let size = upstream.recv(&mut response_buf).await?;
        Ok((dns_server.to_string(), response_buf[..size].to_vec()))
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

                    let (result, _index, _remaining) = select_all(futures).await;
                    match result {
                        Ok((server, response_data)) => {
                            println!("First response from {}", server);
                            socket.send_to(&response_data, peer).await?;
                            println!("Response sent to {}", peer);
                        }
                        Err(e) => {
                            eprintln!("All DNS queries failed: {}", e);
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
