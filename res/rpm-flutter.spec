Name:       subnetdesk-weimutter6688
Version:    1.3.0
Release:    0
Summary:    RPM package
License:    GPL-3.0
URL:        https://github.com/zibo-chen/SubnetDesk
Vendor:     SubnetDesk contributors
Requires:   gtk3 libxcb libXfixes alsa-lib libva pam gstreamer1-plugins-base
Recommends: libayatana-appindicator-gtk3 libxdo
Provides:   libdesktop_drop_plugin.so()(64bit), libdesktop_multi_window_plugin.so()(64bit), libfile_selector_linux_plugin.so()(64bit), libflutter_custom_cursor_plugin.so()(64bit), libflutter_linux_gtk.so()(64bit), libscreen_retriever_plugin.so()(64bit), libtray_manager_plugin.so()(64bit), liburl_launcher_linux_plugin.so()(64bit), libwindow_manager_plugin.so()(64bit), libwindow_size_plugin.so()(64bit), libtexture_rgba_renderer_plugin.so()(64bit)

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

# %global __python %{__python3}

%install

mkdir -p "%{buildroot}/usr/share/subnetdesk-weimutter6688" && cp -r ${HBB}/flutter/build/linux/x64/release/bundle/* -t "%{buildroot}/usr/share/subnetdesk-weimutter6688"
mkdir -p "%{buildroot}/usr/bin"
install -Dm 644 $HBB/res/subnetdesk-weimutter6688.service -t "%{buildroot}/usr/share/subnetdesk-weimutter6688/files"
install -Dm 644 $HBB/res/rustdesk.desktop "%{buildroot}/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688.desktop"
install -Dm 644 $HBB/res/rustdesk-link.desktop "%{buildroot}/usr/share/subnetdesk-weimutter6688/files/subnetdesk-weimutter6688-link.desktop"
install -Dm 644 $HBB/res/128x128@2x.png "%{buildroot}/usr/share/icons/hicolor/256x256/apps/subnetdesk-weimutter6688.png"
install -Dm 644 $HBB/res/scalable.svg "%{buildroot}/usr/share/icons/hicolor/scalable/apps/subnetdesk-weimutter6688.svg"

%files
/usr/share/subnetdesk-weimutter6688/*
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
ln -sf /usr/share/subnetdesk-weimutter6688/subnetdesk-weimutter6688 /usr/bin/subnetdesk-weimutter6688
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
    rm /usr/bin/subnetdesk-weimutter6688 || true
    rmdir /usr/share/subnetdesk-weimutter6688 || true
    rm /usr/share/applications/subnetdesk-weimutter6688.desktop || true
    rm /usr/share/applications/subnetdesk-weimutter6688-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
