# Deploying repotool to TrueNAS SCALE

Runs the archive commands (`fetch`, `grab`, `fsck`, `scan`) directly on the NAS against a local
dataset, instead of over NFS from another machine.

TrueNAS SCALE 25.04+ uses Docker as its app engine, but the Apps UI can only *pull* images — it
cannot build them. So the image is built on a Linux workstation and shipped to the NAS as a
tarball.

## Prerequisites

- **An amd64 Linux build host.** TrueNAS SCALE is x86-64 only. A binary built on an arm64 machine
  (Raspberry Pi, Apple Silicon) will not run on the NAS.
- **SSH enabled on the NAS.** *System Settings → Services → SSH*. The account you log in with
  needs sudo, or log in as `root` if password login for root is enabled.
- **The UID/GID that owns the archive dataset.** The container runs as this user so that files it
  writes keep the right ownership. For the current archive
  (`/mnt/red-cluster1/backup/sources`) that is **1000:3001** (`tomer:tomer`). Re-check with:

  ```bash
  stat -c '%u %g  %U:%G  %a  %n' /mnt/red-cluster1/backup/sources
  find /mnt/red-cluster1/backup/sources ! -user 1000 -o ! -perm -u+w -printf '%m %u:%g %p\n' | head
  ```

  The first gives the numbers; the second must print nothing — anything listed is an object the
  container cannot write, which would fail a fetch partway through. Note that part of the tree
  carries group `1000`, a GID with no group entry on the NAS (an artifact of the repos having been
  written over NFS from the RPi, where `tomer`'s primary group is 1000). That is harmless: the
  directories are mode 755, so writes come from the owner bits, and the owner UID is 1000
  throughout. Running as `1000:3001` gradually converts new objects to the named group.

## 1. Build the binary and the image

On the Linux workstation, in a checkout of this repo:

```bash
cargo build --release
```

Confirm the binary's glibc floor is within what the base image provides (trixie ships 2.41):

```bash
objdump -T target/release/repotool | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1
```

If that prints something higher than 2.41, raise `BASE_IMAGE` to a newer base
(`--build-arg BASE_IMAGE=debian:forky-slim`) or build inside a container instead.

Then confirm the binary needs nothing the base image lacks:

```bash
ldd target/release/repotool
```

Everything listed has to exist in `debian:trixie-slim` (plus the packages the Dockerfile installs).
`libc`, `libm`, `libgcc_s`, `libz`, `libssl.so.3`/`libcrypto.so.3`, and `libssh2.so.1` are all
covered. A `libgit2.so.*` line is not — that means the build linked the host's libgit2 instead of
vendoring it, and the container will fail at startup with
`error while loading shared libraries: libgit2.so.1.9`. `git2` is configured with
`vendored-libgit2` in `Cargo.toml` to prevent exactly that; if the line reappears, the feature is
not taking effect.

Then build the image, passing the dataset's owner so the in-image user matches:

```bash
docker build --build-arg UID=1000 --build-arg GID=3001 -t repotool:latest .
```

Smoke-test it locally before shipping:

```bash
docker run --rm repotool:latest --version
```

## 2. Ship it to the NAS

Stream the image over SSH — no registry, no intermediate file:

```bash
docker save repotool:latest | gzip | ssh truenas.local 'sudo docker load'
```

If the pipe is awkward (sudo prompting for a password over a pipe, flaky link), stage it instead:

```bash
docker save repotool:latest | gzip > repotool.tar.gz && scp repotool.tar.gz truenas.local:/mnt/red-cluster1/tmp/
```

then on the NAS:

```bash
sudo docker load < /mnt/red-cluster1/tmp/repotool.tar.gz
```

Verify it landed:

```bash
ssh truenas.local 'sudo docker image ls repotool'
```

## 3. Run it

One-shot, straight from the shell on the NAS. `--user` must match the dataset owner, and the
dataset is mounted at `/data` because that is the image's working directory:

```bash
sudo docker run --rm --user 1000:3001 -v /mnt/red-cluster1/backup/sources:/data repotool:latest fetch --base-dir /data --archive /data/repo-archive.txt /data/repos
```

To avoid retyping the mount and user flags, copy `docker-compose.yml` to the NAS, set
`ARCHIVE_PATH` / `REPOTOOL_UID` / `REPOTOOL_GID` in a `.env` beside it, and use:

```bash
sudo docker compose run --rm repotool fetch --base-dir /data /data/repos
```

## 4. Schedule recurring fetches (optional)

Add a cron job under *System Settings → Advanced → Cron Jobs*, running as `root`, with the
`docker run` command from step 3 as the command. Keep `RUST_LOG=info` (the image's default) so
output lands in the job's mail/log.

## Notes

- Manually loaded images live in the apps pool (`ix-apps`). They survive reboots, but a pool
  reconfiguration or an aggressive `docker image prune` will remove them — keep the tarball, or
  just rebuild.
- Re-deploying is the same three steps. Docker replaces the `repotool:latest` tag on load; the old
  image layers stay behind as untagged leftovers, so `sudo docker image prune` occasionally.
- Tag with a version (`repotool:0.7.2`) instead of `latest` if you want to keep a rollback target
  on the NAS.
- For `git@` remotes, mount a key into the container (`-v /mnt/red-cluster1/appdata/repotool/ssh:/home/repotool/.ssh:ro`)
  — the image includes `openssh-client` but carries no keys.
