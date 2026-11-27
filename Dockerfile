# OpenDirectory Management Server with the web Admin Center.
#
#   docker build -t opendirectory .
#   docker run -d -p 8443:8443 -v opendirectory-data:/var/lib/opendirectory opendirectory
#
# Then open https://<host>:8443 and create your domain. See docs/DOCKER.md.

# ---- web Admin Center (React)
FROM node:22-bookworm-slim AS ui
WORKDIR /src/apps/admin-center
COPY apps/admin-center/package.json apps/admin-center/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY apps/admin-center/ ./
RUN npx tsc -b && npx vite build

# ---- server and CLI (Rust)
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p od-server -p od-cli \
 && mkdir -p /out && cp target/release/opendirectory-server target/release/od /out/

# ---- runtime
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /var/lib/opendirectory --shell /usr/sbin/nologin opendirectory \
 && mkdir -p /var/lib/opendirectory \
 && chown opendirectory:opendirectory /var/lib/opendirectory
COPY --from=build /out/opendirectory-server /out/od /usr/local/bin/
COPY --from=ui /src/apps/admin-center/dist /usr/share/opendirectory/web
ENV OD_DATA_DIR=/var/lib/opendirectory \
    OD_LISTEN=0.0.0.0:8443 \
    OD_WEB_UI=/usr/share/opendirectory/web
VOLUME ["/var/lib/opendirectory"]
EXPOSE 8443 8444
USER opendirectory
ENTRYPOINT ["/usr/bin/tini", "--", "opendirectory-server"]
CMD ["run"]
