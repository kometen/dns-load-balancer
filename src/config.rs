pub const DNS_SERVERS: &[&str] = &[
    "10.152.183.10:53", // kubernetes
    "1.1.1.1:53",       // Cloudflare
    "8.8.8.8:53",       // Google
];

pub const LOCALHOST_PORT_V4: &str = "127.0.0.1:53";
pub const LOCALHOST_PORT_V6: &str = "[::1]:53";
pub const CACHE_TTL: u64 = 300; // 5 minutes
pub const DNS_TIMEOUT: u64 = 3; // seconds
