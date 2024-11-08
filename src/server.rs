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
        if let Ok(message) = Message::from_bytes(&buf[..size]) {
            for query in message.queries() {
                println!(
                    "Received DNS query: {} (type: {}) from {}",
                    query.name(),
                    query.query_type(),
                    peer
                );

                // Only handle A records for kubernetes-domains
                if query
                    .name()
                    .to_ascii()
                    .as_str()
                    .ends_with(config::KUBERNETES_DOMAIN)
                    || query
                        .name()
                        .to_ascii()
                        .as_str()
                        .ends_with(&format!("{}.", config::KUBERNETES_DOMAIN))
                {
                    if query.query_type().to_string() != "A" {
                        println!(
                            "Non-A record query for domain {}, sending empty response",
                            config::KUBERNETES_DOMAIN
                        );
                        let mut response = Message::new();
                        response.set_id(message.id());
                        response.set_message_type(MessageType::Response);
                        response.set_response_code(ResponseCode::NoError);

                        for q in message.queries() {
                            response.add_query(q.clone());
                        }

                        if let Ok(response_data) = response.to_vec() {
                            socket.send_to(&response_data, peer).await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        if let Some(cached_response) = cache.get(&buf[..size]).await {
            println!("Cached response sent to {}", peer);
            socket.send_to(&cached_response, peer).await?;
            return Ok(());
        }

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
        let original_query_for_error = original_query.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut tx_opt = Some(tx);

        for &dns_server in config::DNS_SERVERS {
            let query_data = encoded_query.clone();
            let original_query_cloned = original_query.clone();
            let tx = tx_opt.take();

            tokio::spawn(async move {
                if let Ok((server, Some(response))) = query_dns(dns_server, query_data).await {
                    if let Ok(response_message) = Message::from_bytes(&response) {
                        if !response_message.answers().is_empty() {
                            if let Some(updated_response) =
                                DnsCache::update_dns_id(&original_query_cloned, response)
                            {
                                if let Some(tx) = tx {
                                    let _ = tx.send((server, updated_response));
                                    return;
                                }
                            }
                        } else {
                            println!("Empty response from {}", server);
                        }
                    }
                }
            });

            if tx_opt.is_none() {
                break;
            }
        }

        match tokio::time::timeout(Duration::from_secs(config::DNS_TIMEOUT), rx).await {
            Ok(Ok((_, response_data))) => {
                cache
                    .set(
                        original_query,
                        response_data.clone(),
                        Duration::from_secs(config::CACHE_TTL),
                    )
                    .await;

                socket.send_to(&response_data, peer).await?;
            }

            _ => {
                let mut msg = Message::new();
                msg.set_response_code(ResponseCode::NXDomain);
                msg.set_message_type(MessageType::Response);

                if let Ok(query_message) = Message::from_bytes(&original_query_for_error) {
                    let query_id = query_message.id();
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
