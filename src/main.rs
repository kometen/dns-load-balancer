mod config;
mod dns;
mod server;

use anyhow::{Ok, Result};
use clap::{Parser, Subcommand};
use config::Config;
use nix::unistd::{getuid, setuid, Uid};
use server::Server;
use tokio::{net::UdpSocket, signal};

// Command line arguments with clap.
#[derive(Parser)]
#[clap(version = env!("CARGO_PKG_VERSION"), author = "Claus Guttesen")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long, group = "mode")]
        config: String,
    },
    Example,
}

fn drop_privileges() -> Result<()> {
    if Uid::effective().is_root() {
        setuid(getuid())?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config } => {
            let dns_servers = Config::load(&config)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
                .servers;

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

            drop_privileges()?;

            let server_v4 = Server::new(socket_v4, 1024, dns_servers.clone());
            let server_v6 = Server::new(socket_v6, 1024, dns_servers);

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
        }
        Commands::Example => {
            println!(
                "{}",
                r#"[[servers]]
address = "1.1.1.1"
use_tls = true
description = "Cloudflare DNS"

[[servers]]
address = "8.8.8.8"
use_tls = true
description = "Google DNS"

[[servers]]
address = "10.152.183.10"
use_tls = false
description = "Kubernetes DNS""#
            );
            return Ok(());
        }
    }
    Ok(())
}
