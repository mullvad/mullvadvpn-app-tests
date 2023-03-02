FROM debian:bullseye-slim

RUN echo "deb http://deb.debian.org/debian bullseye-backports main" > /etc/apt/sources.list.d/backports.list

RUN apt update && apt install -y \
    git gcc libprotobuf-dev curl python3 iproute2 procps \
    libdbus-1-dev protobuf-compiler pkgconf \
    libpcap-dev nftables qemu-system-x86 dnsmasq \
    e2tools gcc-mingw-w64-x86-64 mtools ovmf swtpm/bullseye-backports
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

RUN rustup target add x86_64-pc-windows-gnu

WORKDIR /build
