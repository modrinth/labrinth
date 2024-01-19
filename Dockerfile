FROM rust:1.75.0 as builder
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /usr/src/labrinth
# Download and compile deps
COPY . .
ARG SQLX_OFFLINE=true
RUN cargo install --features dhat --path .

# Final Stage
FROM ubuntu:latest

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

RUN update-ca-certificates

COPY --from=builder  /usr/local/cargo/bin/labrinth /labrinth/labrinth
COPY --from=builder /usr/src/labrinth/migrations/* /labrinth/migrations/
COPY --from=builder /usr/src/labrinth/assets /labrinth/assets
WORKDIR /labrinth

CMD /labrinth/labrinth