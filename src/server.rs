use crate::config;
use crate::dns::cache::DnsCache;
use crate::dns::query::query_dns;
use futures::future::join_all;
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::serialize::binary::BinDecodable;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

pub struct Server {
    socket: Arc<UdpSocket>,
    cache: Arc<DnsCache>,
    buf_size: usize,
}

impl Server {
    pub fn new(socket: UdpSocket, buf_size: usize) -> Self {
        Self {
            socket: Arc::new(socket),
            cache: Arc::new(DnsCache::new()),
            buf_size,
        }
    }

    async fn handle_request(
        socket: Arc<UdpSocket>,
        cache: Arc<DnsCache>,
        buf: Vec<u8>,
        size: usize,
        peer: SocketAddr,
    ) -> std::io::Result<()> {
        println!("Handling request from {}", peer);

        if let Some(cached_response) = cache.get(&buf[..size]).await {
            println!("Cache hit!");
            socket.send_to(&cached_response, peer).await?;
            return Ok(());
        }

        match Message::from_bytes(&buf[..size]) {
            Ok(query) => match query.to_vec() {
                Ok(encoded_query) => {
                    let futures: Vec<_> = config::DNS_SERVERS
                        .iter()
                        .map(|&dns_server| {
                            let query_data = encoded_query.clone();
                            Box::pin(query_dns(dns_server, query_data))
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

                            cache
                                .set(
                                    buf[..size].to_vec(),
                                    response_data.clone(),
                                    Duration::from_secs(config::CACHE_TTL),
                                )
                                .await;

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

    pub async fn run(self) -> std::io::Result<()> {
        let cache_clone = Arc::clone(&self.cache);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                cache_clone.cleanup().await;
            }
        });

        loop {
            let mut buf = vec![0; self.buf_size];
            let (size, peer) = self.socket.recv_from(&mut buf).await?;
            let socket_clone = Arc::clone(&self.socket);
            let cache_clone = Arc::clone(&self.cache);

            tokio::spawn(async move {
                if let Err(e) =
                    Server::handle_request(socket_clone, cache_clone, buf, size, peer).await
                {
                    eprintln!("Error handling request: {}", e);
                }
            });
        }
    }
}
