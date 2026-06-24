# Goblins OS — VERIFICATION-ONLY kickstart, DARK boot (NOT shipped).
# Same as verify-install.ks but sets the system default color-scheme to prefer-dark
# so the installed system boots in Dark, letting the dark wallpaper + chrome be
# captured at real pixels on REAL GNOME (the headless render harness can't switch
# mutter's background actor to the dark wallpaper; real GNOME honors picture-uri-dark).
ostreecontainer --url=/run/install/repo/container --transport=oci
clearpart --all --initlabel --disklabel=gpt
reqpart --add-boot
part / --fstype=xfs --label=root --grow
lang en_US.UTF-8
keyboard us
timezone --utc Etc/UTC
network --bootproto=dhcp --device=link --activate --onboot=on
reboot --eject
%post
echo "GOBLINS_VERIFY_INSTALL_DONE" > /dev/ttyS0 || true
mkdir -p /etc/dconf/db/local.d
printf '[org/gnome/desktop/interface]\ncolor-scheme="prefer-dark"\n' > /etc/dconf/db/local.d/99-verify-dark
dconf update || true
%end
