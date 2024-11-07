mod config;
mod dns;
mod server;

use server::Server;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(config::LOCALHOST_PORT).await?;
    println!("DNS forwarder listening on {}", socket.local_addr()?);

    let server = Server::new(socket, 1024);

    server.run().await?;

    Ok(())
}
