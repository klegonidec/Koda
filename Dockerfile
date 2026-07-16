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
COPY --from=builder /src/target/release/duo-bridge /usr/local/bin/duo-bridge
COPY migrations /app/migrations
RUN mkdir -p /data /workspaces && chown -R app:app /data /workspaces /app
USER app
ENV APP_BIND=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=20s --timeout=5s CMD /usr/bin/curl --fail http://127.0.0.1:8080/health/live || exit 1
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/duo-bridge"]
