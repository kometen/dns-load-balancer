use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinDecodable;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io;
use tokio::net::UdpSocket;

const PUBLIC_DNS: &str = "1.1.1.1:53"; // Cloudflare
const KUBERNETES_DNS: &str = "10.152.183.10:53"; // kubernetes
const LOCALHOST_PORT: &str = "127.0.0.1:53";

struct Server {
    socket: Arc<UdpSocket>,
    buf_size: usize,
}

impl Server {
    async fn handle_request(
        socket: Arc<UdpSocket>,
        buf: Vec<u8>,
        size: usize,
        peer: SocketAddr,
    ) -> io::Result<()> {
        println!("Received query from {}", peer);

        let upstream = UdpSocket::bind("0.0.0.0:0").await?;
        upstream.connect(PUBLIC_DNS).await?;

        match Message::from_bytes(&buf[..size]) {
            Ok(query) => match query.to_vec() {
                Ok(encoded_query) => {
                    upstream.send(&encoded_query).await?;

                    let mut response_buf = vec![0; 1024];
                    match upstream.recv(&mut response_buf).await {
                        Ok(response_size) => {
                            socket.send_to(&response_buf[..response_size], peer).await?;
                            println!("Response sent to {}", peer);
                        }
                        Err(e) => {
                            eprintln!("Error receiving from upstream: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error encoding query: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Error parsing DNS message: {}", e);
            }
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
