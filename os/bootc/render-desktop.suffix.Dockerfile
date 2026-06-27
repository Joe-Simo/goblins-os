# Desktop-proof render stage. Appended to the canonical Containerfile (single
# source of truth) at build time so every OS layer is reused from cache. Never
# shipped: it boots a headless GNOME Shell session and exports composited-desktop
# PNGs (wallpaper + menu bar + dock + window chrome), unlike render.suffix which
# captures isolated app windows under Xvfb.
#
# Build:
#   cat os/bootc/Containerfile os/bootc/render-desktop.suffix.Dockerfile \
#     > /tmp/render-desktop.Dockerfile
#   DOCKER_BUILDKIT=1 docker build -f /tmp/render-desktop.Dockerfile \
#     --target desktop-screenshots --output type=local,dest=os/screenshots/desktop .
FROM goblins-os AS desktop-render
COPY --chmod=0755 os/bootc/render-desktop.sh /usr/local/bin/render-desktop.sh
RUN dnf -y --setopt=retries=20 --setopt=timeout=600 --setopt=minrate=1 install \
      mesa-dri-drivers \
      mesa-libEGL \
      mesa-libgbm \
      mutter \
      gnome-shell-extension-user-theme \
      dconf \
      glib2 \
      curl \
    && dnf clean all \
    && /usr/local/bin/render-desktop.sh

FROM scratch AS desktop-screenshots
COPY --from=desktop-render /out/ /
