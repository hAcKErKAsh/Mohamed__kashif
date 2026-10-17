# --- web -------------------------------------------------------------------
FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- server ----------------------------------------------------------------
FROM rust:1-bookworm AS server
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs && cargo build --release && rm -rf src
COPY backend/ ./
RUN touch src/main.rs src/lib.rs && cargo build --release

# --- runtime ---------------------------------------------------------------
# rustls ships its own root store, so the runtime needs no extra packages.
FROM debian:bookworm-slim
COPY --from=server /app/target/release/lyceum /usr/local/bin/lyceum
COPY --from=web /web/dist /srv/web
ENV STATIC_DIR=/srv/web BIND=0.0.0.0:8080 RUST_LOG=lyceum=info
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD ["/bin/sh", "-c", "exec 3<>/dev/tcp/127.0.0.1/8080"]
CMD ["lyceum"]
