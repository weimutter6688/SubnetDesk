Name:       subnetdesk-weimutter6688
Version:    1.1.9
Release:    0
Summary:    RPM package
License:    GPL-3.0
Requires:   gtk3 libxcb1 libXfixes3 alsa-utils libXtst6 libva2 pam gstreamer-plugins-base gstreamer-plugin-pipewire
Recommends: libayatana-appindicator3-1 xdotool

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/share/subnetdesk-weimutter6688/
mkdir -p %{buildroot}/usr/share/subnetdesk-weimutter6688/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/rustdesk %{buildroot}/usr/bin/subnetdesk-weimutter6688
install $HBB/libsciter-gtk.so %{buildroot}/usr/share/subnetdesk-weimutter6688/libsciter-gtk.so
install $HBB/res/subnetdesk-weimutter6688.service %{buildroot}/usr/share/subnetdesk-weimutter6688/files/
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/subnetdesk-weimutter6688.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/subnetdesk-weimutter6688.svg
install $HBB/res/rustdesk.desktop %{buildroot}/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.desktop
install $HBB/res/rustdesk-link.desktop %{buildroot}/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688-link.desktop

%files
/usr/bin/subnetdesk-weimutter6688
/usr/share/subnetdesk-weimutter6688/libsciter-gtk.so
/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.service
/usr/share/icons/hicolor/256x256/apps/subnetdesk-weimutter6688.png
/usr/share/icons/hicolor/scalable/apps/subnetdesk-weimutter6688.svg
/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.desktop
/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688-link.desktop

%changelog
# let's skip this for now

%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop subnetdesk-weimutter6688 || true
  ;;
esac

%post
cp /usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.service /etc/systemd/system/subnetdesk-weimutter6688.service
cp /usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.desktop /usr/share/applications/
cp /usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688-link.desktop /usr/share/applications/
systemctl daemon-reload
systemctl enable subnetdesk-weimutter6688
systemctl start subnetdesk-weimutter6688
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop subnetdesk-weimutter6688 || true
    systemctl disable subnetdesk-weimutter6688 || true
    rm /etc/systemd/system/subnetdesk-weimutter6688.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/share/applications/subnetdesk-weimutter6688.desktop || true
    rm /usr/share/applications/subnetdesk-weimutter6688-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
