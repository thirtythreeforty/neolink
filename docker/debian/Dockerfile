# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only
# syntax=docker/dockerfile:experimental

FROM --platform=$BUILDPLATFORM docker.io/rust:slim-buster AS build
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
    PATH=/usr/local/cargo/bin:$PATH

RUN --mount=type=tmpfs,destination=/usr/local/cargo/ \
    echo >&2 "\e[0;32mRUNNING: ${TARGETPLATFORM} ${BUILDPLATFORM} ${arch}\e[0m"; \
    apt-get update && \
    apt-get install --assume-yes --no-install-recommends \
      build-essential \
      "${gcc}" "${gpp}" \
      libgstrtspserver-1.0-dev:${arch} \
      libgstreamer1.0-dev:${arch} \
      libgtk2.0-dev:${arch} && \
    /bin/rm -rf /var/cache/apt && \
    /bin/rm -rf /var/lib/apt/lists/* && \
    rustup target add "${rustArch}" && \
    cargo build --release --all-features && \
    cp "/usr/local/src/neolink/target/${rustArch}/release/neolink" /neolink

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
  /neolink \
  /usr/local/bin/neolink
COPY docker/debian/entrypoint.sh /entrypoint.sh

# Check that we can run the binary
# RUN ldd /usr/local/bin/neolink >&2 && /usr/local/bin/neolink --help >&2

CMD ["/usr/local/bin/neolink", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
