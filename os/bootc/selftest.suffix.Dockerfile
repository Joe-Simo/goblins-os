# Install + services self-test stage. Appended to the canonical Containerfile at
# build time so every OS layer is reused from cache. Never shipped; it only
# exercises the installed OS to prove the contract holds, the daemon serves, and
# the persistent resident answers IPC. A non-zero self-test fails the build.
FROM goblins-os AS selftest
RUN dnf -y install curl jq socat && dnf clean all
COPY os/bootc/run-selftest.sh /usr/local/bin/run-selftest.sh
RUN chmod +x /usr/local/bin/run-selftest.sh && /usr/local/bin/run-selftest.sh
