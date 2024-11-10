mod config;
mod dns;
mod server;

use server::Server;
use tokio::{net::UdpSocket, signal};

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

    // Shutdown-channel
    let (shutdown_tx, shutdown_rx_v4) = tokio::sync::broadcast::channel(1);
    let shutdown_rx_v6 = shutdown_rx_v4.resubscribe();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server_v4.run(shutdown_rx_v4) => {
                    println!("IPv4 server stopped normally");
            }

            _ = server_v6.run(shutdown_rx_v6) => {
                    println!("IPv6 server stopped normally");
            }
        }
    });

    signal::ctrl_c().await?;
    println!("\nReceived Ctrl+C, shutting down ...");
    let _ = shutdown_tx.send(());

    tokio::select! {
        _ = server_handle => {
            println!("Servers shut down successfully");
        }

        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
            println!("Shutdown timed out after 5 seconds");
        }
    }

    println!("Server shutdown complete");

    Ok(())
}
