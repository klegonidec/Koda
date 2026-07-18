FROM node:22-bookworm-slim AS frontend
WORKDIR /frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --ignore-scripts
COPY frontend ./
RUN npm run build

FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY Cargo.toml rust-toolchain.toml ./
RUN cargo generate-lockfile
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git openssh-client tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin app
WORKDIR /app
COPY --from=builder /src/target/release/koda /usr/local/bin/koda
COPY --from=builder /src/target/release/koda-harness /usr/local/bin/koda-harness
COPY --from=builder /src/target/release/koda-egress /usr/local/bin/koda-egress
COPY migrations /app/migrations
COPY --from=frontend /frontend/dist /app/static
RUN mkdir -p /data /workspaces && chown -R app:app /data /workspaces /app
USER app
ENV APP_BIND=0.0.0.0:8080
ENV KODA_STATIC_DIR=/app/static
EXPOSE 8080
HEALTHCHECK --interval=20s --timeout=5s CMD /usr/bin/curl --fail http://127.0.0.1:8080/health/live || exit 1
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/koda"]
