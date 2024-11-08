mod config;
mod dns;
mod server;

use server::Server;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let socket_v4 = UdpSocket::bind(config::LOCALHOST_PORT_V4).await?;
    println!(
        "DNS forwarder listening on IPv4: {}",
        socket_v4.local_addr()?
    );

    let socket_v6 = UdpSocket::bind(config::LOCALHOST_PORT_V6).await?;
    println!(
        "DNS forwarder listening on IPv6: {}",
        socket_v6.local_addr()?
    );

    let server_v4 = Server::new(socket_v4, 1024);
    let server_v6 = Server::new(socket_v6, 1024);

    tokio::select! {
        result = server_v4.run() => {
            if let Err(e) = result {
                eprintln!("IPv4 server error: {}", e);
            }
        }

        result = server_v6.run() => {
            if let Err(e) = result {
                eprintln!("IPv6 server error: {}", e);
            }
        }
    }

    Ok(())
}
