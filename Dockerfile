# --- build image
FROM rust:1.85 AS builder

RUN rustup target add x86_64-unknown-linux-musl

# 🔧 add missing native deps
RUN apt update && apt install -y \
    musl-tools \
    musl-dev \
    pkg-config \
    libssl-dev \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# 🔧 required for musl + openssl
ENV OPENSSL_STATIC=1
ENV OPENSSL_DIR=/usr

ENV USER=app
ENV UID=10001

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"

WORKDIR /app
COPY . .

# build
RUN cargo build --target x86_64-unknown-linux-musl --release


# --- final image
FROM scratch

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/wastebin ./

USER app:app
CMD ["/app/wastebin"]
