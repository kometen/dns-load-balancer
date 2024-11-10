pub struct DnsServer {
    pub address: &'static str,
    pub use_tls: bool,
}

pub const DNS_SERVERS: &[DnsServer] = &[
    DnsServer {
        address: "10.152.183.10", // kubernetes
        use_tls: false,
    },
    DnsServer {
        address: "1.1.1.1", // Cloudflare
        use_tls: true,
    },
    DnsServer {
        address: "8.8.8.8", // Google
        use_tls: true,
    },
];

pub const LOCALHOST_PORT_V4: &str = "127.0.0.1:53";
pub const LOCALHOST_PORT_V6: &str = "[::1]:53";
pub const CACHE_TTL: u64 = 300; // 5 minutes
pub const DNS_TIMEOUT: u64 = 3; // seconds
pub const KUBERNETES_DOMAIN: &str = "cluster.local.";
