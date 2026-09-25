# syntax=docker/dockerfile:1
ARG RUST_VERSION=1
ARG NODE_VERSION=22
ARG DEBIAN_VERSION=bookworm

################################################################################
# Stage 1 - public web UI (React → /web/dist, served from wwwroot/)
################################################################################
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-${DEBIAN_VERSION}-slim AS web

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build \
    && test -f dist/index.html \
    && test -f dist/openapi.yaml \
    && test -f dist/sw.js

################################################################################
# Stage 1a - admin panel (React → /src/wwwroot/admin, served at the admin path)
################################################################################
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-${DEBIAN_VERSION}-slim AS admin

WORKDIR /src/admin
COPY admin/package.json admin/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY admin/ ./
RUN npm run build && test -f /src/wwwroot/admin/index.html

################################################################################
# Stage 1b - documentation (Docusaurus → /wwwroot/docs, served at /docs/)
################################################################################
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-${DEBIAN_VERSION}-slim AS docs

WORKDIR /src/docs
COPY docs/package.json docs/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY docs/ ./
RUN npm run build && test -f /src/wwwroot/docs/index.html

################################################################################
# Stage 2 - server binary
################################################################################
FROM rust:${RUST_VERSION}-${DEBIAN_VERSION} AS build

# Version info is normally taken from git; pass these when building without .git
ARG CRABINDEX_VERSION=
ARG CRABINDEX_GIT_SHA=
ARG CRABINDEX_GIT_BRANCH=

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    CRABINDEX_VERSION="${CRABINDEX_VERSION}" \
    CRABINDEX_GIT_SHA="${CRABINDEX_GIT_SHA}" \
    CRABINDEX_GIT_BRANCH="${CRABINDEX_GIT_BRANCH}" \
    cargo build --release --locked -p crabindex \
    && install -D -m 0755 target/release/crabindex /dist/crabindex

################################################################################
# Stage 3 - runtime
################################################################################
FROM debian:${DEBIAN_VERSION}-slim AS runtime

ARG CRABINDEX_VERSION=dev

LABEL org.opencontainers.image.title="CrabIndex" \
      org.opencontainers.image.description="CrabIndex - torrent tracker aggregator & file database" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.revision="${CRABINDEX_VERSION}"

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl dumb-init tzdata; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd -g 1000 crabindex; \
    useradd -u 1000 -g crabindex -d /app -s /usr/sbin/nologin -M crabindex; \
    mkdir -p /app/Data/fdb /app/Data/temp /app/Data/log /app/Data/tracks /app/config /app/defaults; \
    chown -R crabindex:crabindex /app; \
    chmod -R 750 /app

WORKDIR /app

COPY --from=build --chown=crabindex:crabindex /dist/crabindex /app/crabindex
COPY --from=web --chown=crabindex:crabindex /web/dist /app/wwwroot
COPY --from=admin --chown=crabindex:crabindex /src/wwwroot/admin /app/wwwroot/admin
COPY --from=docs --chown=crabindex:crabindex /src/wwwroot/docs /app/wwwroot/docs
COPY --chown=crabindex:crabindex Data/ /app/Data/
# First-run defaults when /app/Data is an empty bind mount
COPY --chown=crabindex:crabindex Data/example.yaml /app/defaults/init.yaml
COPY --chown=crabindex:crabindex Data/example.conf /app/defaults/init.conf
COPY --chown=crabindex:crabindex --chmod=550 entrypoint.sh /entrypoint.sh

ENV CRABINDEX_VERSION="${CRABINDEX_VERSION}" \
    TZ=UTC \
    UMASK=0027

USER crabindex:crabindex

VOLUME ["/app/Data", "/app/config"]

EXPOSE 9117/tcp

HEALTHCHECK --interval=30s --timeout=15s --start-period=45s --retries=3 --start-interval=5s \
    CMD curl -f -s --max-time 10 http://127.0.0.1:9117/health || exit 1

ENTRYPOINT ["dumb-init", "--", "/entrypoint.sh"]
CMD ["./crabindex"]
