use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinDecodable;
use tokio::io;
use tokio::net::UdpSocket;

const PUBLIC_DNS: &str = "1.1.1.1:53"; // Cloudflare
const KUBERNETES_DNS: &str = "10.152.183.10:53"; // kubernetes
const LOCALHOST_PORT: &str = "127.0.0.1:53";
const BUF_SIZE: usize = 1024;

struct Server {
    socket: UdpSocket,
    buf: Vec<u8>,
}

impl Server {
    async fn run(self) -> Result<(), io::Error> {
        let Server { socket, mut buf } = self;

        loop {
            let (size, peer) = socket.recv_from(&mut buf).await?;
            println!("Received query from {}", peer);

            // Create upstream socket
            let upstream = UdpSocket::bind("0.0.0.0:0").await?;
            upstream.connect(PUBLIC_DNS).await?;

            match Message::from_bytes(&buf[..size]) {
                Ok(query) => {
                    match query.to_vec() {
                        Ok(encoded_query) => {
                            // Query upstream DNS
                            upstream.send(&encoded_query).await?;

                            // Receive response from upstream DNS
                            let mut response_buf = vec![0; BUF_SIZE];
                            match upstream.recv(&mut response_buf).await {
                                Ok(response_size) => {
                                    socket.send_to(&response_buf[..response_size], peer).await?;
                                }
                                Err(e) => {
                                    eprintln!("Error receiving from upstream: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error encoding query: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error parsing DNS message: {}", e);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let socket = UdpSocket::bind(LOCALHOST_PORT).await?;
    println!("DNS forwarder listening on {}", socket.local_addr()?);

    let server = Server {
        socket,
        buf: vec![0; BUF_SIZE],
    };

    server.run().await?;

    Ok(())
}
