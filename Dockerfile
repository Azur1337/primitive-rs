# syntax=docker/dockerfile:1

# ---- Build stage ----
FROM rust:1-slim AS build
WORKDIR /app

# Copy the whole source (a lockfile is created during the release build).
COPY . .

# Build only the web binary with the `web` feature enabled.
RUN cargo build --release --features web --bin primitive-web

# ---- Runtime stage ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/primitive-web /usr/local/bin/primitive-web

# Bind on all interfaces inside the container; port is configurable.
ENV ADDR=0.0.0.0
ENV PORT=6123
EXPOSE 6123

CMD ["/usr/local/bin/primitive-web"]
