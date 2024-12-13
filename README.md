# dns-load-balancer

Forward DNS queries to DNS-servers. Sometimes I need to access a kubernetes-cluster
using a wireguard vpn-tunnel and access some services. So I usually let wireguard
define the DNS-server I am using since it also resolve to public DNS besides the
internal kubernetes-services.

Sometimes the vpn-tunnel becomes stale and I need to disconnect and reconnect. So
I wanted a DNS-forwarder that could talk to one or more public DNS-servers besides
the kubernetes DNS-server. I started out having Claude make a simpel DNS-forwarder.

During the development I wanted to use tokio for asynchronous tasks. Claude have
done the heavy lifting.

This DNS-forwarder will send out a request to each DNS-server defined in `config.rs`
and stop when it receives an answer or None if name resolution was not successful.

I noticed a delay when connecting to a kubernetes-service and it turned out the client
was issuing a DNS-request for type A (IPv4), AAAA (IPv6) and that introduced this
delay. The client in this case is `psql` to access a PostgreSQL database.

So a check is added that if the request is for a `cluster.local.` any request other
than a type A will be ignored.

Clone, build and run the project with

```
cargo build [--release]
sudo ./target/[debug|release]/dns_load_balancer run --config [CONFIG]
```

An example of a configuration is printed to the console with `./target/release/dns_load_balancer example`.
Save end edit the file `config.toml` to use your preferred DNS-servers.

Since it connects to port 53 you need priviliged access for it to start.

As an example here is my `config.toml`:

```
$ cat /usr/local/etc/dns-load-balancer/config.toml
[[servers]]
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
description = "Kubernetes DNS"
```

And when Wireguard VPN-tunnel is not connected to Kubernetes DNS:

```
$ host postgresql.invoice.svc.cluster.local
DNS resolution failed: Failed to resolve hostname: postgresql.invoice.svc.cluster.local.
Root cause: no record found for Query { name: Name("postgresql.invoice.svc.cluster.local."), query_type: AAAA, query_class: IN }
Error: Failed to resolve hostname: postgresql.invoice.svc.cluster.local.

Caused by:
    no record found for Query { name: Name("postgresql.invoice.svc.cluster.local."), query_type: AAAA, query_class: IN }
```

When connected:
```
$ host postgresql.invoice.svc.cluster.local
postgresql.invoice.svc.cluster.local has address 10.152.183.95
```

Had I configured the Kubernetes DNS as the only DNS-server, either in network-settings or in `config.toml` no nameresolution would take place.
By adding Cloudflare and Google nameresolution will ususally work and only fail if the Wireguard VPN is not connected.
