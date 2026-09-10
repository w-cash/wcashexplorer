FROM rust:1.88-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home explorer
COPY --from=builder /build/target/release/wcashexplorer /usr/local/bin/wcashexplorer
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/wcashexplorer"]
