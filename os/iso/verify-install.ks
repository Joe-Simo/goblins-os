# Goblins OS — VERIFICATION-ONLY kickstart (NOT shipped).
# The shipped ISO deliberately leaves disk selection interactive (never auto-wipes).
# This kickstart automates a clean install onto a scratch VM disk so the full
# install -> reboot -> installed desktop chain can be exercised headlessly in qemu.
# It deploys from the embedded OCI image on the ISO (works offline) and does NOT run
# the local-registry `bootc switch` (which only existed for the dev build registry).
ostreecontainer --url=/run/install/repo/container --transport=oci
ignoredisk --only-use=vda
zerombr
clearpart --all --initlabel --disklabel=gpt --drives=vda
bootloader --location=mbr --boot-drive=vda
reqpart --add-boot
part / --fstype=xfs --label=root --grow --size=1024 --ondisk=vda
lang en_US.UTF-8
keyboard us
timezone --utc Etc/UTC
network --bootproto=dhcp --device=link --activate --onboot=on
reboot --eject
%post
echo "GOBLINS_VERIFY_INSTALL_DONE" > /dev/ttyS0 || true
%end
