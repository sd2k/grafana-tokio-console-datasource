# syntax=docker/dockerfile:1.3

ARG RUST_VERSION=1.60
FROM node:lts-alpine AS yarn-builder
ENV YARN_CACHE_FOLDER=/opt/yarncache

WORKDIR /app/tokio-console-datasource

# Install yarn dependencies.
COPY ./package.json ./yarn.lock /app/tokio-console-datasource/
RUN --mount=type=cache,target=/opt/yarncache yarn install --frozen-lockfile

# Build plugin frontend.
COPY ./README.md ./CHANGELOG.md ./LICENSE ./jest.config.js ./.prettierrc.js ./tsconfig.json /app/tokio-console-datasource/
COPY src /app/tokio-console-datasource/src
RUN yarn build

FROM rust:${RUST_VERSION}-alpine AS rust-builder

RUN apk add --no-cache musl-dev protoc && \
  rustup component add rustfmt

WORKDIR /usr/src/backend

COPY ./backend /usr/src/backend

RUN \
  --mount=type=cache,id=tokio-console-datasource-target-build-cache,target=/usr/src/backend/target/release/build \
  --mount=type=cache,id=tokio-console-datasource-target-build-deps,target=/usr/src/backend/target/release/deps \
  --mount=type=cache,id=tokio-console-datasource-target-build-incremental,target=/usr/src/backend/target/release/incremental \
  --mount=type=cache,id=tokio-console-datasource-cargo-git-cache,target=/usr/local/cargo/git \
  --mount=type=cache,id=tokio-console-datasource-cargo-registry-cache,target=/usr/local/cargo/registry \
  RUSTFLAGS="--cfg tokio_unstable" cargo build --release

FROM sd2k/grafana:table-charts

# Used to get the target plugin binary name.
ARG TARGETPLATFORM

# Copy plugin files into custom location, to avoid conflicting with contents of /var/lib/grafana. Point
# Grafana to this directory as additional plugin path with the GF_PATHS_PLUGINS env var.
ENV CUSTOM_PLUGIN_DIR /home/grafana/plugins
RUN mkdir -p ${CUSTOM_PLUGIN_DIR}
COPY ./provisioning /etc/grafana/provisioning
COPY --chown=grafana --from=yarn-builder /app/tokio-console-datasource/dist ${CUSTOM_PLUGIN_DIR}/bsull-console-datasource/dist
COPY --chown=grafana --from=rust-builder /usr/src/backend/target/release/grafana-tokio-console-datasource ${CUSTOM_PLUGIN_DIR}/bsull-console-datasource/dist/grafana-tokio-console-datasource
RUN GOARCH=$(echo ${TARGETPLATFORM} | sed 's|/|_|') \
  && mv ${CUSTOM_PLUGIN_DIR}/bsull-console-datasource/dist/grafana-tokio-console-datasource ${CUSTOM_PLUGIN_DIR}/bsull-console-datasource/dist/grafana-tokio-console-datasource_${GOARCH}

ENV GF_DEFAULT_APP_MODE development
ENV GF_AUTH_ANONYMOUS_ENABLED true
ENV GF_AUTH_ANONYMOUS_ORG_ROLE Admin
ENV GF_PATHS_PLUGINS ${CUSTOM_PLUGIN_DIR}

