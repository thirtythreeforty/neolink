# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard,
#                    Andrew King,
#                    Miroslav Šedivý
# SPDX-License-Identifier: AGPL-3.0-only

FROM docker.io/rust:slim-buster AS build
LABEL authours="George Hilliard <thirtythreeforty@gmail.com>"

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      libgstrtspserver-1.0-dev \
      libgstreamer1.0-dev \
      libgtk2.0-dev \
      libglib2.0-dev && \
    apt-get clean -y && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/src/neolink

# Build the main program
COPY . /usr/local/src/neolink
RUN cargo build --release

# Create the release container. Match the base OS used to build
FROM debian:buster-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        libgstrtspserver-1.0-0 \
        libgstreamer1.0-0 \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad && \
    apt-get clean -y && rm -rf /var/lib/apt/lists/*

COPY --from=build \
  /usr/local/src/neolink/target/release/neolink \
  /usr/local/bin/neolink
COPY docker/entrypoint.sh /entrypoint.sh

CMD ["/usr/local/bin/neolink", "rtsp", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
