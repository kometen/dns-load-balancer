use crate::config;
use crate::dns::cache::DnsCache;
use crate::dns::query::query_dns;
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
        if let Some(cached_response) = cache.get(&buf[..size]).await {
            println!("Cached response sent to {}", peer);
            socket.send_to(&cached_response, peer).await?;
            return Ok(());
        }

        println!("Cache miss, querying DNS servers for {}", peer);

        match Message::from_bytes(&buf[..size]) {
            Ok(query) => match query.to_vec() {
                Ok(encoded_query) => {
                    Self::handle_dns_queries(
                        socket,
                        cache,
                        encoded_query,
                        buf[..size].to_vec(),
                        peer,
                    )
                    .await?;
                }
                Err(e) => eprintln!("Error encoding query: {}", e),
            },
            Err(e) => eprintln!("Error parsing DNS message: {}", e),
        }
        Ok(())
    }

    async fn handle_dns_queries(
        socket: Arc<UdpSocket>,
        cache: Arc<DnsCache>,
        encoded_query: Vec<u8>,
        original_query: Vec<u8>,
        peer: SocketAddr,
    ) -> std::io::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut tx_opt = Some(tx);

        for &dns_server in config::DNS_SERVERS {
            let query_data = encoded_query.clone();
            let original_query_cloned = original_query.clone();
            let tx = tx_opt.take();

            tokio::spawn(async move {
                if let Ok((server, Some(response))) = query_dns(dns_server, query_data).await {
                    // Update DNS id before sending
                    if let Some(updated_response) =
                        DnsCache::update_dns_id(&original_query_cloned, response)
                    {
                        if let Ok(response_message) = Message::from_bytes(&updated_response) {
                            if let Ok(query_message) = Message::from_bytes(&original_query_cloned) {
                                if response_message.id() == query_message.id() {
                                    if let Some(tx) = tx {
                                        println!(
                                            "Sending response with ID {}",
                                            response_message.id()
                                        );
                                        let _ = tx.send((server, updated_response));
                                        return;
                                    }
                                } else {
                                    println!(
                                        "ID mismatch after update: expected {} got {}",
                                        query_message.id(),
                                        response_message.id()
                                    );
                                }
                            }
                        }
                    }
                    println!("Failed to update response ID from {}", server);
                }
            });

            if tx_opt.is_none() {
                break;
            }
        }

        match tokio::time::timeout(Duration::from_secs(config::DNS_TIMEOUT), rx).await {
            Ok(Ok((server, response_data))) => {
                println!("First valid response from {}", server);

                // Last validation before caching
                if let (Ok(query_message), Ok(response_message)) = (
                    Message::from_bytes(&original_query),
                    Message::from_bytes(&response_data),
                ) {
                    if query_message.id() != response_message.id() {
                        println!(
                            "Final ID check failed: expected {} got {}",
                            query_message.id(),
                            response_message.id()
                        );
                    }
                }

                cache
                    .set(
                        original_query,
                        response_data.clone(),
                        Duration::from_secs(config::CACHE_TTL),
                    )
                    .await;

                socket.send_to(&response_data, peer).await?;
                println!("Response sent to {}", peer);
            }

            _ => {
                println!("No valid response received, sending NXDOMAIN to {}", peer);
                let mut msg = Message::new();
                msg.set_response_code(ResponseCode::NXDomain);
                msg.set_message_type(MessageType::Response);

                // Set correct id from the original query
                if let Ok(query_message) = Message::from_bytes(&original_query) {
                    let query_id = query_message.id();
                    println!("Setting NXDOMAIN response id to {}", query_id);
                    msg.set_id(query_id);
                }

                if let Ok(response_data) = msg.to_vec() {
                    socket.send_to(&response_data, peer).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn run(self) -> std::io::Result<()> {
        let cache_clone = Arc::clone(&self.cache);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                cache_clone.dump_cache().await;
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
