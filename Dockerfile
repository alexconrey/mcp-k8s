FROM rust:1.89 AS builder

RUN rustup target add x86_64-unknown-linux-musl
RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release --target x86_64-unknown-linux-musl --bin mcp-k8s

FROM scratch

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/mcp-k8s /mcp-k8s
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

EXPOSE 8080
USER 65534

ENTRYPOINT ["/mcp-k8s", "--http"]
