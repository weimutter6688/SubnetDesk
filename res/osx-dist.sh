#!/usr/bin/env bash

echo $MACOS_CODESIGN_IDENTITY
cargo install flutter_rust_bridge_codegen --version 1.80.1 --features uuid --locked
cd flutter; flutter pub get; cd -
~/.cargo/bin/flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs --dart-output ./flutter/lib/generated_bridge.dart --c-output ./flutter/macos/Runner/bridge_generated.h
./build.py --flutter
rm subnetdesk-weimutter6688-$VERSION.dmg
# security find-identity -v
codesign --force --options runtime -s $MACOS_CODESIGN_IDENTITY --deep --strict ./flutter/build/macos/Build/Products/Release/SubnetDesk-weimutter6688.app -vvv
create-dmg --icon "SubnetDesk-weimutter6688.app" 200 190 --hide-extension "SubnetDesk-weimutter6688.app" --window-size 800 400 --app-drop-link 600 185 subnetdesk-weimutter6688-$VERSION.dmg ./flutter/build/macos/Build/Products/Release/SubnetDesk-weimutter6688.app
codesign --force --options runtime -s $MACOS_CODESIGN_IDENTITY --deep --strict subnetdesk-weimutter6688-$VERSION.dmg -vvv
# notarize the subnetdesk-weimutter6688-${{ env.VERSION }}.dmg
rcodesign notary-submit --api-key-path ~/.p12/api-key.json  --staple subnetdesk-weimutter6688-$VERSION.dmg
