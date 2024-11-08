FROM rust

RUN useradd -u 1010 -m dns_load_balancer

WORKDIR /usr/src/dns_load_balancer
COPY . .

RUN cargo install --path .

CMD ["dns_load_balancer"]
