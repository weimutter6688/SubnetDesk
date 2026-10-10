#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

require_fixed() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq -- "$expected" "$file"; then
    printf 'Missing SubnetDesk branding in %s: %s\n' "$file" "$expected" >&2
    exit 1
  fi
}

require_fixed README.md '# SubnetDesk'
require_fixed Cargo.toml 'description = "SubnetDesk LAN/VPN Remote Desktop"'
require_fixed Cargo.toml 'ProductName = "SubnetDesk"'
require_fixed Cargo.toml 'identifier = "io.github.weimutter6688.subnetdesk"'
require_fixed libs/hbb_common/src/config.rs 'RwLock::new("SubnetDesk-weimutter6688".to_owned())'
require_fixed libs/hbb_common/src/config.rs 'RwLock::new("io.github.weimutter6688".to_owned())'
require_fixed flutter/android/app/build.gradle 'applicationId "io.github.weimutter6688.subnetdesk"'
require_fixed flutter/android/app/src/main/res/values/strings.xml '<string name="app_name">SubnetDesk</string>'
require_fixed flutter/ios/Runner/Info.plist '<string>io.github.weimutter6688.subnetdesk</string>'
require_fixed flutter/ios/Runner/Info.plist '<string>subnetdesk-weimutter6688</string>'
require_fixed flutter/macos/Runner/Configs/AppInfo.xcconfig 'PRODUCT_NAME = SubnetDesk-weimutter6688'
require_fixed flutter/macos/Runner/Configs/AppInfo.xcconfig 'PRODUCT_BUNDLE_IDENTIFIER = io.github.weimutter6688.subnetdesk'
require_fixed flutter/windows/runner/Runner.rc 'VALUE "ProductName", "SubnetDesk"'
require_fixed flutter/linux/CMakeLists.txt 'set(APPLICATION_ID "io.github.weimutter6688.subnetdesk")'
require_fixed flatpak/rustdesk.json '"id": "io.github.weimutter6688.SubnetDesk"'
require_fixed flatpak/io.github.weimutter6688.SubnetDesk.metainfo.xml '<name>SubnetDesk</name>'
require_fixed res/rustdesk.desktop 'Name=SubnetDesk'
require_fixed res/subnetdesk-weimutter6688.service 'Description=SubnetDesk'
require_fixed docs/common-library-provenance.md '64aab1c4585a9459c9849f0d0b53af5bbc40eacd'
if [[ -e .gitmodules ]] || git ls-files --stage libs/hbb_common | grep -q '^160000 '; then
  printf 'Common library must be tracked in the main repository, without a gitlink.\n' >&2
  exit 1
fi
require_fixed build.py 'Build/Products/Release/SubnetDesk-weimutter6688.app'
require_fixed .github/workflows/flutter-build.yml 'Build/Products/Release/SubnetDesk-weimutter6688.app'
require_fixed .github/workflows/flutter-build.yml 'subnetdesk-weimutter6688-${{ env.VERSION }}'

if grep -Fq 'RustDesk.app' .github/workflows/flutter-build.yml; then
  printf 'Release workflow still references RustDesk.app\n' >&2
  exit 1
fi

for stale in \
  flutter/ios/Runner/GoogleService-Info.plist \
  flutter/ios/exportOptions.plist \
  flatpak/com.rustdesk.RustDesk.metainfo.xml \
  .github/workflows/fdroid.yml; do
  if [[ -e "$stale" ]]; then
    printf 'Stale upstream release file is still present: %s\n' "$stale" >&2
    exit 1
  fi
done

"${PYTHON:-python3}" scripts/generate_subnetdesk_icons.py --check
bash scripts/check_linux_service_branding.sh
printf 'SubnetDesk branding and generated icons are consistent.\n'
