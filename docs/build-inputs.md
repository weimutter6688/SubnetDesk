# Verified build inputs

Direct executable/archive downloads in `flutter-build.yml` go through
`scripts/download_verified.py`. The reviewed URL and SHA-256 for each supported
input are committed in `scripts/build-downloads.json`. Unknown keys, changed
payloads and redirects from HTTPS to HTTP fail the build before extraction.

USBMMIDD v2, apple-codesign 0.22.0 and NASM 2.16.03 hashes were computed from fresh
downloads on 2026-10-10. The USB archive also matches the archive used for previous
local Windows packages. The pinned Rust 1.82.0 archive hashes and Flutter 3.44.1 /
Dart 3.12.1 hashes were obtained from their official published checksum records.
Changing a version requires reviewing and updating this manifest. A moving
release tag cannot replace a payload unnoticed because its bytes must still
match the committed checksum.

Legacy Sciter jobs remain explicitly disabled and contain no download or build
steps. They cannot pull `sciter.dll` or `libsciter-gtk.so` from a floating branch.
AppImage recipes require authenticated package indexes. Cargo builds continue to
use `--locked`; crate checksums and Git dependency revisions live in Cargo.lock.
Actions are pinned to commits. Package managers and pinned setup actions still
fetch their own dependencies; this manifest covers direct workflow downloads,
not an assertion that every transitive build input is vendored or audited.

Run the tamper/redirect regression tests with
`python scripts/test_download_verified.py`. To verify a local archive without
network access, use `python scripts/download_verified.py KEY --output PATH
--verify-only`.
