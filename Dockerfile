# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only

FROM docker.io/rust:1-alpine AS build
MAINTAINER thirtythreeforty@gmail.com

RUN apk add --no-cache -X http://dl-cdn.alpinelinux.org/alpine/edge/testing \
  gst-rtsp-server-dev
RUN apk add --no-cache musl-dev gcc

# Use static linking to work around https://github.com/rust-lang/rust/pull/58575
ENV RUSTFLAGS='-C target-feature=-crt-static'

# Compile dependencies before main app to save rebuild time
# https://github.com/errmac-v/cargo-build-dependencies
RUN cargo install cargo-build-dependencies
RUN mkdir /usr/local/src \
  && cd /usr/local/src \
  && USER=root cargo new --bin neolink
WORKDIR /usr/local/src/neolink
COPY Cargo.toml Cargo.lock ./
RUN cargo build-dependencies --release

# Build the main program
COPY . /usr/local/src/neolink
RUN cargo build --release

# Create the release container. Match the base OS used to build
FROM docker.io/alpine:latest

RUN apk add --no-cache -X http://dl-cdn.alpinelinux.org/alpine/edge/testing gst-rtsp-server
RUN apk add libgcc

COPY --from=build \
  /usr/local/src/neolink/target/release/neolink \
  /usr/local/bin/neolink
COPY docker/entrypoint.sh /entrypoint.sh

CMD ["/usr/local/bin/neolink", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554 
