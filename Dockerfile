FROM rust
WORKDIR /usr/src/dns_load_balancer
COPY . .
RUN cargo install --path .

EXPOSE 53/udp

CMD ["dns_load_balancer"]
