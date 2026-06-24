# Design-proof render stage. Appended to the canonical Containerfile (single
# source of truth) at build time so every OS layer is reused from cache. This
# stage is never shipped; it only renders the native apps and exports PNGs.
FROM goblins-os AS render
ARG GOBLINS_OS_RENDER_SCOPE=all
RUN dnf -y install \
      xorg-x11-server-Xvfb \
      ImageMagick \
      xdotool \
      curl \
      jq \
      google-noto-sans-fonts \
      abattis-cantarell-fonts \
      dejavu-sans-fonts \
    && dnf clean all
COPY os/bootc/render-screens.sh /usr/local/bin/render-screens.sh
RUN chmod +x /usr/local/bin/render-screens.sh \
    && GOBLINS_OS_RENDER_SCOPE="$GOBLINS_OS_RENDER_SCOPE" /usr/local/bin/render-screens.sh

FROM scratch AS screenshots
COPY --from=render /out/ /
