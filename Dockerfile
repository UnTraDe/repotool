# syntax=docker/dockerfile:1

# Runtime-only image. The binary is built on the host first:
#
#   cargo build --release
#
# and copied in from target/release/. It must be a Linux binary of the same
# architecture as the NAS (amd64), dynamically linked against a glibc no newer
# than the one in BASE_IMAGE. Trixie (Debian 13) ships glibc 2.41; bookworm's
# 2.36 is too old for a binary built on a current toolchain. Check the floor with:
#   objdump -T target/release/repotool | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1
ARG BASE_IMAGE=debian:trixie-slim
FROM ${BASE_IMAGE}

ARG UID=1000
ARG GID=1000

# fetch/grab/fsck shell out to the `git` binary, so it has to exist at runtime.
# libssl3 was renamed libssl3t64 in the time_t transition (trixie, Ubuntu 24.04),
# so try the new name first and fall back for older bases.
RUN apt-get update && apt-get install -y --no-install-recommends \
        git \
        openssh-client \
        ca-certificates \
        zlib1g \
    && (apt-get install -y --no-install-recommends libssl3t64 \
        || apt-get install -y --no-install-recommends libssl3) \
    && rm -rf /var/lib/apt/lists/* \
    # The archive pool is owned by whatever UID TrueNAS uses; without this git
    # refuses to touch repos it considers "dubious ownership".
    && git config --system --add safe.directory '*'

RUN groupadd -g "${GID}" repotool \
    && useradd -m -u "${UID}" -g "${GID}" -s /usr/sbin/nologin repotool \
    # keep HOME writable even when the container is started with a different --user
    && chmod 0777 /home/repotool

COPY --chmod=0755 target/release/repotool /usr/local/bin/repotool

ENV HOME=/home/repotool \
    RUST_LOG=info \
    GIT_TERMINAL_PROMPT=0

WORKDIR /data
USER repotool

ENTRYPOINT ["/usr/local/bin/repotool"]
CMD ["--help"]
