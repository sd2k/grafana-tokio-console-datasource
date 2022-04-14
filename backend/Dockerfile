FROM rust:1.60-alpine

WORKDIR /usr/src/backend

RUN apk add --no-cache musl-dev protoc && \
  rustup component add rustfmt

RUN wget https://github.com/watchexec/cargo-watch/releases/download/v8.1.1/cargo-watch-v8.1.1-x86_64-unknown-linux-musl.tar.xz && \
  tar -xJf cargo-watch-v8.1.1-x86_64-unknown-linux-musl.tar.xz && \
  mv cargo-watch-v8.1.1-x86_64-unknown-linux-musl/cargo-watch /usr/local/bin && \
  rm -r cargo-watch-v8.1.1-x86_64-unknown-linux-musl cargo-watch-v8.1.1-x86_64-unknown-linux-musl.tar.xz

COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock* ./
COPY src ./src
