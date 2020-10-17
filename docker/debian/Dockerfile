# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only
# syntax=docker/dockerfile:experimental

FROM docker.io/debian:buster-slim AS build
ARG TARGETPLATFORM
ARG REPO
ARG VERSION
ARG OWNER

LABEL description="An image for the neolink program which is a reolink camera to rtsp translator"
LABEL repository="$REPO"
LABEL version="$VERSION"
LABEL maintainer="$OWNER"


# ORIGINALLY I TRIED TO BUILD IN A QEMU CONTAINER.
# THIS WOULD HAVE BEEN IDEAL BECAUSE THEN WE COULD BUILD FOR ALL
# ARCH SUPPORTED BY QEMU, DEBIAN AND RUST WITHOUT ANY COMPLICATED CROSS COMPILES.
# HOWEVER I KEPT GETTING THE FOLLOWING ERROR DURING COMPILE
# could not read directory '/usr/local/cargo/registry/index/github.com-1285ae84e5963aae/.git//refs': Value too large for defined data type; class=Os (2)
# Google suggest I should either update the kernel or my fs neither of which I can do on github actions
# SO INSTEAD I CROSS COMPILE USING platform=$BUILDPLATFORM and deploy on platform=$TARGETPLATFORM

WORKDIR /usr/local/src/neolink

# Build the main program
COPY . /usr/local/src/neolink

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    CURL_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt

RUN --mount=type=tmpfs,destination=/usr/local/cargo/registry/ \
    --mount=type=tmpfs,destination=/usr/local/cargo/git/ \
    echo >&2 "\e[0;32mBUILDING FOR: ${TARGETPLATFORM}\e[0m"; \
    echo >&2 "\e[0;33mUpdating apt\e[0m"; \
    apt-get update && \
    echo >&2 "\e[0;33mInstalling packages\e[0m"; \
    apt-get install --assume-yes --no-install-recommends \
      build-essential \
      curl ca-certificates build-essential \
      libgstrtspserver-1.0-dev \
      libgstreamer1.0-dev \
      libgtk2.0-dev && \
    /bin/rm -rf /var/cache/apt && \
    /bin/rm -rf /var/lib/apt/lists/* && \
    update-ca-certificates && \
    echo >&2 "\e[0;33mInstalling rust\e[0m"; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustinit.sh && \
    sh rustinit.sh --verbose -y && \
    echo >&2 "\e[0;33mBuilding neolink\e[0m"; \
    echo >&2 "\e[0;34mPATH: ${PATH} \e[0m"; \
    echo >&2 "\e[0;35mmount: $(mount | grep tmpfs) \e[0m"; \
    echo >&2 "\e[0;36mls: $(ls -lha /usr/local/cargo/bin) \e[0m"; \
    cargo build --release --all-features && \
    cp "/usr/local/src/neolink/target/release/neolink" /neolink

# Create the release container.
FROM --platform=$TARGETPLATFORM docker.io/debian:buster-slim

ARG TARGETPLATFORM
ARG REPO
ARG VERSION
ARG OWNER

LABEL description="An image for the neolink program which is a reolink camera to rtsp translator"
LABEL repository="$REPO"
LABEL version="$VERSION"
LABEL maintainer="$OWNER"

RUN apt-get update && \
  apt-get install --assume-yes --no-install-recommends \
    libgstrtspserver-1.0-0 \
    libgstreamer1.0-0 \
    libgstreamer-plugins-bad1.0-0 \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    libglib2.0-0 && \
  /bin/rm -rf /var/cache/apt && \
  /bin/rm -rf /var/lib/apt/lists/*

COPY --from=build \
  /usr/local/src/neolink/target/release/neolink \
  /usr/local/bin/neolink
COPY docker/debian/entrypoint.sh /entrypoint.sh

# Check that we can run the binary
# RUN ldd /usr/local/bin/neolink >&2 && /usr/local/bin/neolink --help >&2

CMD ["/usr/local/bin/neolink", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
