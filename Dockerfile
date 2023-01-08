# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard,
#                    Andrew King,
#                    Miroslav Šedivý
# SPDX-License-Identifier: AGPL-3.0-only

FROM docker.io/rust:slim-buster AS build
ARG TARGETPLATFORM

ENV DEBIAN_FRONTEND=noninteractive


WORKDIR /usr/local/src/neolink
COPY . /usr/local/src/neolink

# Build the main program or copy from artifact
# hadolint ignore=DL3008
RUN  echo "TARGETPLATFORM: ${TARGETPLATFORM}"; \
  if [ -f "${TARGETPLATFORM}/neolink" ]; then \
    echo "Restoring from artifact"; \
    mkdir -p /usr/local/src/neolink/target/release/; \
    cp "${TARGETPLATFORM}/neolink" "/usr/local/src/neolink/target/release/neolink"; \
  else \
    echo "Building from scratch"; \
    apt-get update && \
        apt-get install -y --no-install-recommends \
          build-essential \
          libgstrtspserver-1.0-dev \
          libgstreamer1.0-dev \
          libgtk2.0-dev \
          libglib2.0-dev && \
        apt-get clean -y && rm -rf /var/lib/apt/lists/* ; \
    cargo build --release; \
  fi

# Check it works
RUN chmod +x "/usr/local/src/neolink/target/release/neolink" && \
  "/usr/local/src/neolink/target/release/neolink" --version

# Create the release container. Match the base OS used to build
FROM debian:buster-slim
ARG TARGETPLATFORM
ARG REPO
ARG VERSION
ARG OWNER

LABEL description="An image for the neolink program which is a reolink camera to rtsp translator"
LABEL repository="$REPO"
LABEL version="$VERSION"
LABEL maintainer="$OWNER"

# hadolint ignore=DL3008
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

RUN "/usr/local/bin/neolink" --version

CMD ["/usr/local/bin/neolink", "rtsp", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
