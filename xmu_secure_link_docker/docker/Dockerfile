# syntax=docker/dockerfile:1
#
# Single-stage image: no compilation.
# The user confirmed a prebuilt Linux binary already ships in the GitHub Release,
# so we just download that self-contained tarball (binary + bundled .so deps).
#
# IMPORTANT: base must be ubuntu:24.04.
#   The released binary links GLIBC_2.39 -> debian:bookworm (2.36) fails to start.
#   ubuntu:24.04 provides glibc 2.39 (matches the CI runner) and has microsocks in universe.

FROM ubuntu:24.04

# Which release asset to pull. Override with --build-arg if a newer release lands.
ARG SL_VERSION=v0.1.0
ARG SL_ASSET=xmu_secure_link-x86_64-unknown-linux-gnu.tar.gz
ARG SL_BASE_URL=https://github.com/XMU-MoYu-Club/xmu_secure_link/releases/download

ENV DEBIAN_FRONTEND=noninteractive

# Runtime deps only:
#   ca-certificates, curl -> TLS + download the release
#   iproute2               -> `ip` / `ss` route + socket inspection in entrypoint
#   iputils-ping, dnsutils -> diagnostics inside the container
#   microsocks             -> the SOCKS5 server (universe)
#   libstdc++6             -> required by libovpnffi.so (the OpenVPN3 C++ FFI lib)
# libgcc_s / glibc / libssl / libcrypto / lz4 / jsoncpp / xxhash are bundled in the
# tarball, so we do NOT install them from apt.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        iproute2 \
        iputils-ping \
        dnsutils \
        microsocks \
        libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/xmu_secure_link

# Download + unpack the release into /opt/xmu_secure_link (flat layout:
# ./xmu_secure_link + ./*.so*).
RUN curl -fSL "${SL_BASE_URL}/${SL_VERSION}/${SL_ASSET}" -o /tmp/sl.tar.gz \
    && tar xzf /tmp/sl.tar.gz -C /opt/xmu_secure_link \
    && rm -f /tmp/sl.tar.gz \
    && chmod +x /opt/xmu_secure_link/xmu_secure_link

# The binary uses RUNPATH=$ORIGIN, which is NOT transitive: libovpnffi.so's own
# deps (libssl.so.3, libcrypto.so.3, ...) are not found via the binary's RUNPATH.
# Setting LD_LIBRARY_PATH makes the whole bundle resolvable.
ENV LD_LIBRARY_PATH=/opt/xmu_secure_link

LABEL org.opencontainers.image.title="xmu_secure_link_docker" \
      org.opencontainers.image.description="SOCKS5 middle layer over xmu_secure_link (SecureLink OpenVPN3), split-tunnel: campus/ACL targets (e.g. *.xmu.edu.cn) exit via the campus VPN, everything else direct. Exposes 127.0.0.1:1080 SOCKS5." \
      org.opencontainers.image.licenses="GPL-3.0-only" \
      org.opencontainers.image.source="https://github.com/XMU-MoYu-Club/xmu_secure_link" \
      org.opencontainers.image.url="https://hub.docker.com/r/vintcessun/xmu_secure_link_docker" \
      org.opencontainers.image.version="v0.1.0"

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
