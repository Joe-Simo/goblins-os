# Packaging-contract verification stage. Appended to the canonical Containerfile
# at build time so CI can prove the installed image contract without exporting the
# full bootc image into the runner's Docker daemon.
FROM goblins-os AS verify
RUN /usr/libexec/goblins-os/goblins-os-verify | tee /tmp/goblins-os-verify.log \
    && grep -q 'blocked=0' /tmp/goblins-os-verify.log
