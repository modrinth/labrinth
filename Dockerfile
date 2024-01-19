FROM rust:1.75.0 as builder
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /usr/src/labrinth
# Download and compile deps
COPY . .
ARG SQLX_OFFLINE=true
RUN cargo build --release --features jemalloc

# Final Stage
FROM ubuntu:latest

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

RUN update-ca-certificates

COPY --from=build /usr/src/labrinth/target/release/labrinth /labrinth/labrinth
COPY --from=builder /usr/src/labrinth/migrations/* /labrinth/migrations/
COPY --from=builder /usr/src/labrinth/assets /labrinth/assets
WORKDIR /labrinth

CMD /labrinth/labrinth