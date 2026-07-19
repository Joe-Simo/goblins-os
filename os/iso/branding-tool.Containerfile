ARG FEDORA_IMAGE=docker.io/library/fedora@sha256:6c75d5bf57cb0fa5aa4b92c6a83c86c791644496d9ac230de7711f5b8ec3b898
FROM ${FEDORA_IMAGE}

ARG FEDORA_IMAGE
ARG SOURCE_COMMIT
ARG CONTAINERFILE_SHA256
RUN printf '%s' "$SOURCE_COMMIT" | grep -Eq '^[0-9a-f]{40}$' \
    && printf '%s' "$CONTAINERFILE_SHA256" | grep -Eq '^[0-9a-f]{64}$' \
    && dnf -y --setopt=install_weak_deps=False install \
      ImageMagick \
      diffutils \
      isomd5sum \
      squashfs-tools \
      xorriso \
    && dnf clean all \
    && rm -rf /var/cache/dnf \
    && install -d -m 0755 /usr/share/goblins-os-installer-branding-tool \
    && { \
      printf 'name\tevr\tarch\tlicense\tvendor\n'; \
      rpm -qa --qf '%{NAME}\t%{EVR}\t%{ARCH}\t%{LICENSE}\t%{VENDOR}\n' \
        | LC_ALL=C sort; \
    } > /usr/share/goblins-os-installer-branding-tool/rpm-packages.tsv \
    && command -v checkisomd5 \
    && command -v cmp \
    && command -v implantisomd5 \
    && command -v magick \
    && command -v mksquashfs \
    && command -v osirrox \
    && command -v unsquashfs \
    && command -v xorriso

LABEL org.opencontainers.image.title="Goblins OS installer branding tool" \
      org.opencontainers.image.description="Immutable reviewed toolchain for remastering Goblins OS Anaconda media" \
      org.opencontainers.image.revision="$SOURCE_COMMIT" \
      org.opencontainers.image.base.name="$FEDORA_IMAGE" \
      io.goblins-os.branding-tool.containerfile.sha256="$CONTAINERFILE_SHA256"
