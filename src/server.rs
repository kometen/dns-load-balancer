use crate::config::{ServerConfig, CACHE_TTL, DNS_TIMEOUT, KUBERNETES_DOMAIN};
use crate::dns::cache::DnsCache;
use crate::dns::query::{query_dns, query_dns_with_fallback};
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
    dns_servers: Arc<Vec<ServerConfig>>,
}

impl Server {
    pub fn new(socket: UdpSocket, buf_size: usize, dns_servers: Vec<ServerConfig>) -> Self {
        Self {
            socket: Arc::new(socket),
            cache: Arc::new(DnsCache::new()),
            buf_size,
            dns_servers: Arc::new(dns_servers),
        }
    }

    async fn handle_request(
        socket: Arc<UdpSocket>,
        cache: Arc<DnsCache>,
        buf: Vec<u8>,
        size: usize,
        peer: SocketAddr,
        dns_servers: Arc<Vec<ServerConfig>>,
    ) -> std::io::Result<()> {
        if let Ok(message) = Message::from_bytes(&buf[..size]) {
            for query in message.queries() {
                // Only handle A records for kubernetes-domains
                if query
                    .name()
                    .to_ascii()
                    .as_str()
                    .ends_with(KUBERNETES_DOMAIN)
                    || query
                        .name()
                        .to_ascii()
                        .as_str()
                        .ends_with(&format!("{}.", KUBERNETES_DOMAIN))
                {
                    // Non-A record query for kubernetes-domain, send empty response,
                    if query.query_type().to_string() != "A" {
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
                        dns_servers,
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
        dns_servers: Arc<Vec<ServerConfig>>,
    ) -> std::io::Result<()> {
        let timeout = tokio::time::Duration::from_secs(DNS_TIMEOUT);
        let original_query_for_error = original_query.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel(dns_servers.len());

        for dns_server in dns_servers.iter() {
            let query_data = encoded_query.clone();
            let original_query_cloned = original_query.clone();
            let tx = tx.clone();
            let dns_server = dns_server.clone();

            tokio::spawn(async move {
                let result = if dns_server.use_tls {
                    match tokio::time::timeout(
                        timeout,
                        query_dns_with_fallback(&dns_server.address, query_data),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Ok((dns_server.address.to_string(), None)),
                    }
                } else {
                    match tokio::time::timeout(timeout, query_dns(&dns_server.address, query_data))
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => Ok((dns_server.address.to_string(), None)),
                    }
                };

                if let Ok((server, Some(response))) = result {
                    if let Ok(response_message) = Message::from_bytes(&response) {
                        if !response_message.answers().is_empty() {
                            if let Some(updated_response) =
                                DnsCache::update_dns_id(&original_query_cloned, response.to_vec())
                            {
                                let _ = tx.send((server, updated_response)).await;
                            }
                        } else {
                            println!("Empty response from {}", server);
                        }
                    }
                }
            });
        }

        match tokio::time::timeout(Duration::from_secs(DNS_TIMEOUT), rx.recv()).await {
            Ok(Some((_, response_data))) => {
                cache
                    .set(
                        original_query,
                        response_data.clone(),
                        Duration::from_secs(CACHE_TTL),
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

    pub async fn run(
        self,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> std::io::Result<()> {
        let cache_clone = Arc::clone(&self.cache);

        let mut shutdown_cleanup = shutdown.resubscribe();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        cache_clone.cleanup().await;
                    }

                    _ = shutdown_cleanup.recv() => {
                        println!("Cleanup task shutting down");
                        break;
                    }
                }
            }
        });

        loop {
            let mut buf = vec![0; self.buf_size];

            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    match result {
                        Ok((size, peer)) => {
                            let socket_clone = Arc::clone(&self.socket);
                            let cache_clone = Arc::clone(&self.cache);
                            let dns_servers = Arc::clone(&self.dns_servers);
                            let mut shutdown_handler = shutdown.resubscribe();

                            tokio::spawn(async move {
                                tokio::select! {
                                    _ = Server::handle_request(socket_clone, cache_clone, buf, size, peer, dns_servers) => {}
                                        _ = shutdown_handler.recv() => {
                                            println!("Request handler shutting down");
                                        }
                                }
                            });
                        }
                        Err(e) => eprintln!("Error receiving: {}", e),
                    }
                }
                _ = shutdown.recv() => {
                    println!("Main server loop shutting down");
                    break;
                }
            }
        }

        println!("Server shutdown complete");
        Ok(())
    }
}
