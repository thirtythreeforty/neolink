# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only

#################################
##            SETUP            ##
#################################
# This is the generic base for  #
# all other dockers to follow   #
#################################
FROM docker.io/alpine:edge AS setup
ARG TARGETPLATFORM

RUN apk add --no-cache -X http://dl-cdn.alpinelinux.org/alpine/edge/testing libgcc \
  tzdata \
  gstreamer \
  gst-plugins-base \
  gst-plugins-good \
  gst-plugins-bad \
  gst-plugins-ugly \
  gst-rtsp-server


#################################
##            BUILD            ##
#################################
# Install dev packages and      #
# with cargo                    #
#################################
FROM setup AS build
ARG TARGETPLATFORM

# Until Alpine merges gst-rtsp-server into a release, pull all Gstreamer packages
# from the "testing" release
RUN apk add --no-cache \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/main \
    -X http://dl-cdn.alpinelinux.org/alpine/edge/testing \
  gst-rtsp-server-dev
RUN apk add --no-cache musl-dev gcc
RUN apk add --no-cache rust cargo

# Use static linking to work around https://github.com/rust-lang/rust/pull/58575
ENV RUSTFLAGS='-C target-feature=-crt-static'

WORKDIR /usr/local/src/neolink

# Build the main program
COPY . /usr/local/src/neolink
RUN cargo build --release


#################################
##            PUBLISH          ##
#################################
# Copy from build neolink and   #
# prepares for execution        #
#################################
FROM setup
ARG TARGETPLATFORM
ARG REPO
ARG VERSION
ARG OWNER

LABEL description="An image for the neolink program which is a reolink camera to rtsp translator"
LABEL repository="$REPO"
LABEL version="$VERSION"
LABEL maintainer="$OWNER"

COPY --from=build \
  /usr/local/src/neolink/target/release/neolink \
  /usr/local/bin/neolink

COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

CMD ["/usr/local/bin/neolink", "--config", "/etc/neolink.toml"]
ENTRYPOINT ["/entrypoint.sh"]
EXPOSE 8554
