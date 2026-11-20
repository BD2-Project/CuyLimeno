FROM rust:1.95-slim-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM python:3.12-slim-bookworm
WORKDIR /app
COPY --from=build /build/target/release/cuy-limeno-server /usr/local/bin/cuy-limeno-server
ENV CUYLIMENO_SERVER_ADDR=0.0.0.0:3001 \
    SOUPDB_HOST=engine \
    SOUPDB_PORT=55432 \
    SOUPDB_TIMEOUT_MS=10000 \
    SOUPDB_POOL_SIZE=4 \
    SOUPDB_INVERTED_STORAGE_ROOT=/app/data/plugins/inverted \
    CUYLIMENO_DATA_DIR=/app/data/cuylimeno
RUN mkdir -p /app/data/cuylimeno
EXPOSE 3001
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=6 \
    CMD ["python", "-c", "from urllib.request import urlopen; urlopen('http://127.0.0.1:3001/api/health', timeout=2).close()"]
CMD ["cuy-limeno-server"]
