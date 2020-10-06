# Neolink Docker image build scripts
# Copyright (c) 2020 George Hilliard
# SPDX-License-Identifier: AGPL-3.0-only

FROM --platform=$BUILDPLATFORM docker.io/rust:slim-buster AS build
ARG TARGETPLATFORM
ARG BUILDPLATFORM
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

RUN echo >&2 "\e[0;32mRUNNING: ${TARGETPLATFORM} ${BUILDPLATFORM} ${arch}\e[0m"; \
    if [ "${TARGETPLATFORM}" = "${BUILDPLATFORM}" ]; then \
      target="native"; \
    else \
      target="${TARGETPLATFORM}"; \
    fi; \
    case "${target}" in \
        "linux/amd64") \
            echo >&2 "\e[0;32mBUILDING for ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            rustArch='x86_64-unknown-linux-gnu'; \
            arch="amd64"; \
            gcc=" gcc-x86-64-linux-gnu"; \
            gpp=" g++-x86-64-linux-gnu"; \
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc && \
            export CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc  && \
            export CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++  && \
            export PKG_CONFIG_ALLOW_CROSS=1  && \
            export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig/  && \
            export PKG_CONFIG_LIBDIR=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig/; \
          ;; \
        "linux/arm/v7") \
            echo >&2 "\e[0;32mBUILDING for ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            rustArch='armv7-unknown-linux-gnueabihf'; \
            arch="armhf"; \
            gcc="gcc-arm-linux-gnueabihf"; \
            gpp="g++-arm-linux-gnueabihf"; \
            export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc && \
            export CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc  && \
            export CXX_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-g++  && \
            export PKG_CONFIG_ALLOW_CROSS=1  && \
            export PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig:/usr/share/pkgconfig/  && \
            export PKG_CONFIG_LIBDIR=/usr/lib/arm-linux-gnueabihf/pkgconfig:/usr/share/pkgconfig/; \
          ;; \
        "linux/arm/v6") \
            echo >&2 "\e[0;32mBUILDING for ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            rustArch='arm-unknown-linux-gnueabi'; \
            arch="armel"; \
            gcc="gcc-arm-linux-gnueabi"; \
            gpp="g++-arm-linux-gnueabi"; \
            export CARGO_TARGET_ARM_UNKNOWN_LINUX_GNUEABI_LINKER=arm-linux-gnueabi-gcc && \
            export CC_arm_unknown_linux_gnueabi=arm-linux-gnueabi-gcc  && \
            export CXX_arm_unknown_linux_gnueabi=arm-linux-gnueabi-g++  && \
            export PKG_CONFIG_ALLOW_CROSS=1  && \
            export PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabi/pkgconfig:/usr/share/pkgconfig/  && \
            export PKG_CONFIG_LIBDIR=/usr/lib/arm-linux-gnueabi/pkgconfig:/usr/share/pkgconfig/; \
          ;; \
        "linux/arm64") \
            echo >&2 "\e[0;32mBUILDING for ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            rustArch='aarch64-unknown-linux-gnu'; \
            arch="arm64"; \
            gcc="gcc-aarch64-linux-gnu"; \
            gpp="g++-aarch64-linux-gnu"; \
            export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc && \
            export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc  && \
            export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++  && \
            export PKG_CONFIG_ALLOW_CROSS=1  && \
            export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig/  && \
            export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig/; \
         ;; \
        "linux/386") \
            echo >&2 "\e[0;32mBUILDING for ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            rustArch='i686-unknown-linux-gnu'; \
            arch="i386"; \
            gcc="gcc-i686-linux-gnu"; \
            gpp="g++-i686-linux-gnu"; \
            export CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_LINKER=i686-linux-gnu-gcc && \
            export CC_i686_unknown_linux_gnu=i686-linux-gnu-gcc  && \
            export CXX_i686_unknown_linux_gnu=i686-linux-gnu-g++  && \
            export PKG_CONFIG_ALLOW_CROSS=1  && \
            export PKG_CONFIG_PATH=/usr/lib/i386-linux-gnu/pkgconfig:/usr/share/pkgconfig/  && \
            export PKG_CONFIG_LIBDIR=/usr/lib/i386-linux-gnu/pkgconfig:/usr/share/pkgconfig/; \
          ;; \
        "native") \
            echo >&2 "\e[0;32mNot cross compiling: ${TARGETPLATFORM} on ${BUILDPLATFORM}\e[0m"; \
            dpkgArch="$(dpkg --print-architecture)"; \
            arch="${dpkgArch##*-}"; \
            case "${arch}" in \
                amd64) rustArch='x86_64-unknown-linux-gnu' ;; \
                armhf) rustArch='armv7-unknown-linux-gnueabihf' ;; \
                arm64) rustArch='aarch64-unknown-linux-gnu' ;; \
                i386) rustArch='i686-unknown-linux-gnu' ;; \
                *) echo >&2 "\e[0;31mUnsupported architecture: ${dpkgArch}\e[0m"; exit 1 ;; \
            esac; \
            gcc="gcc" \
            gpp="g++" \
          ;; \
        *) echo >&2 "\e[0;31mUnsupported architecture: ${TARGETPLATFORM} ${BUILDPLATFORM} ${arch}\e[0m"; exit 1 ;; \
    esac; \
    dpkg --add-architecture "${arch}" && \
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
    cargo build --release --all-features --target="${rustArch}" && \
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
