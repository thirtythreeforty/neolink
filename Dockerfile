# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only

FROM docker.io/rust:1-alpine AS build
MAINTAINER thirtythreeforty@gmail.com

# Until Alpine merges gst-rtsp-server into a release, pull all Gstreamer packages
# from the "testing" release
RUN apk add --no-cache \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/main \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/testing \
  gst-rtsp-server-dev
RUN apk add --no-cache musl-dev gcc

# Use static linking to work around https://github.com/rust-lang/rust/pull/58575
ENV RUSTFLAGS='-C target-feature=-crt-static'

WORKDIR /usr/local/src/neolink

# Build the main program
COPY . /usr/local/src/neolink
RUN cargo build --release

# Create the release container. Match the base OS used to build
FROM docker.io/alpine:latest

RUN apk add --no-cache \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/main \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/testing \
  libgcc \
  tzdata \
  gstreamer \
  gst-plugins-base \
  gst-plugins-good \
  gst-plugins-bad \
  gst-plugins-ugly \
  gst-rtsp-server

COPY --from=build \
  /usr/local/src/neolink/target/release/neolink \
  /usr/local/bin/neolink
COPY docker/entrypoint.sh /entrypoint.sh

CMD ["/usr/local/bin/neolink", "rtsp", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
