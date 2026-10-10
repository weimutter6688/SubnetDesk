#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

require_fixed() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq -- "$expected" "$file"; then
    printf 'Missing Linux service contract in %s: %s\n' "$file" "$expected" >&2
    exit 1
  fi
}

reject_pattern() {
  local pattern="$1"
  shift
  if rg -n -- "$pattern" "$@"; then
    printf 'Legacy RustDesk service activation remains in Linux packaging.\n' >&2
    exit 1
  fi
}

service_resource=res/subnetdesk-weimutter6688.service
debian_scripts=(
  res/DEBIAN/preinst
  res/DEBIAN/postinst
  res/DEBIAN/prerm
)
rpm_specs=(
  res/rpm-flutter.spec
  res/rpm-flutter-suse.spec
  res/rpm.spec
  res/rpm-suse.spec
)
arch_files=(res/PKGBUILD res/pacman_install)
packaging_files=(build.py "${debian_scripts[@]}" "${rpm_specs[@]}" "${arch_files[@]}")

if [[ ! -f "$service_resource" ]]; then
  printf 'Missing canonical Linux systemd unit: %s\n' "$service_resource" >&2
  exit 1
fi
if [[ -e res/rustdesk.service ]]; then
  printf 'Legacy systemd unit source must be removed: res/rustdesk.service\n' >&2
  exit 1
fi

require_fixed "$service_resource" 'Description=SubnetDesk'
require_fixed "$service_resource" 'ExecStart=/usr/bin/subnetdesk-weimutter6688 --service'
require_fixed "$service_resource" 'ExecStop=/usr/bin/pkill -f "subnetdesk-weimutter6688 --"'
require_fixed build.py 'Package: subnetdesk-weimutter6688'
require_fixed flutter/linux/CMakeLists.txt 'set(BINARY_NAME "subnetdesk-weimutter6688")'
require_fixed res/rustdesk.desktop 'Exec=subnetdesk-weimutter6688 %u'
require_fixed res/rustdesk.desktop 'Icon=subnetdesk-weimutter6688'
require_fixed res/DEBIAN/postinst '/usr/lib/systemd/system/subnetdesk-weimutter6688.service'
require_fixed res/DEBIAN/postinst 'systemctl enable subnetdesk-weimutter6688'
require_fixed res/DEBIAN/postinst 'systemctl start subnetdesk-weimutter6688'
require_fixed res/DEBIAN/preinst 'systemctl stop subnetdesk-weimutter6688'
require_fixed res/DEBIAN/prerm 'systemctl stop subnetdesk-weimutter6688'
require_fixed res/DEBIAN/prerm 'systemctl disable subnetdesk-weimutter6688'

for file in "${rpm_specs[@]}"; do
  require_fixed "$file" '$HBB/res/subnetdesk-weimutter6688.service'
  require_fixed "$file" '/etc/systemd/system/subnetdesk-weimutter6688.service'
  require_fixed "$file" 'systemctl enable subnetdesk-weimutter6688'
  require_fixed "$file" 'systemctl start subnetdesk-weimutter6688'
  require_fixed "$file" 'systemctl stop subnetdesk-weimutter6688'
  require_fixed "$file" 'systemctl disable subnetdesk-weimutter6688'
done

require_fixed res/PKGBUILD '$HBB/res/subnetdesk-weimutter6688.service'
require_fixed res/pacman_install '/etc/systemd/system/subnetdesk-weimutter6688.service'
require_fixed res/pacman_install 'systemctl enable subnetdesk-weimutter6688'
require_fixed res/pacman_install 'systemctl start subnetdesk-weimutter6688'
require_fixed res/pacman_install 'systemctl stop subnetdesk-weimutter6688'
require_fixed res/pacman_install 'systemctl disable subnetdesk-weimutter6688'

reject_pattern 'res/rustdesk\.service' "${packaging_files[@]}"
reject_pattern 'cp .*rustdesk\.service .*/(etc|usr/lib)/systemd' "${packaging_files[@]}"
reject_pattern 'systemctl +(enable|start|stop|disable) +rustdesk([ .;]|$)' "${packaging_files[@]}"
reject_pattern '/usr/bin/rustdesk' "${debian_scripts[@]}" "${rpm_specs[@]}" "${arch_files[@]}" "$service_resource"
reject_pattern '/usr/share/rustdesk' "${packaging_files[@]}"
reject_pattern 'Package: rustdesk' build.py
reject_pattern '^Name: +rustdesk$' "${rpm_specs[@]}"
reject_pattern '^pkgname=rustdesk$' res/PKGBUILD

printf 'Linux systemd service branding check passed.\n'
