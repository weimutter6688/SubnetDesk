# Changelog

All notable SubnetDesk changes are documented in this file.

## [1.3.0.89] - 2026-10-14

- LAN security hardening (P0–P2): handshake errors carry stable `[LAN-E1001..1008]` codes and wipe key material; discovery replies no longer include the MAC address and are rate-limited; the pending-auth and Web gateway limits share one admission controller; listeners and the discovery responder shut down and restart under supervision; connection ids are atomic; the Linux sudo shell-quoting fix.
- **Behavior change:** an invalid CIDR/entry in `lan-allowed-networks` now fails closed — the whole allow-list rejects every peer and the LAN listener refuses to start until the entry is fixed (it used to be silently skipped).
- **Behavior change:** while the shared default access password is still set, logins are accepted only from loopback; set your own password to allow remote LAN access.
- CI quality gates: cargo-audit with a documented baseline, cargo-deny source/ban checks, fuzz smoke tests for the LAN message decoder and endpoint parser, a Clippy correctness gate, and property tests for allow-list parsing and Web origin checks.
- Dependency advisory fixes: rustls 0.23.46, rustls-webpki 0.103.15, openssl 0.10.72, quinn-proto 0.11.15, h2 0.4.16, bytes 1.11.1, crossbeam-channel/epoch, url 2.5.4/idna 1.1 (17 advisories fixed, including RUSTSEC-2026-0346..0349 published 2026-10-14). The rest stay in `.cargo/audit.toml` with reasons, mostly needing Rust ≥1.85.
- Docs: Linux build dependencies (README), Android namespace notes, Rust quality gates.

## [1.3.0.87] - 2026-10-11

- Add GitHub desktop releases for Windows, macOS and Linux on x86_64 and ARM64, without commercial signing certificates.
- Validate package versions and completeness, generate SHA-256 checksums and publish stable releases only after every selected build succeeds.
- Fix freshly generated Flutter bridge class names and Linux container dependency checks.
- Package and launch the renamed macOS service helper; verify DMG contents, CPU architecture and signatures before uploading.
- Forward UTF-8 build output on Windows and verify log handling with restrictive encodings.
- Regenerate the embedded Web protocol for adaptive resolution and network telemetry fields.
- Document manual and tag-triggered releases in `docs/github-releases.md`.

## [1.3.0.85] - 2026-10-10

- Use SubnetDesk consistently for executable, service, configuration and package names; remove the GitHub username from runtime identities.
- Use product-owned application/keyring namespaces, discovery and signed handshake names; update both endpoints to .85.
- Restore the SubnetDesk MSI upgrade identity so existing SubnetDesk installations can be upgraded.
- Retain the .84 security fixes and rebuild Windows installer and portable packages under the SubnetDesk name.

## [1.3.0.84] - 2026-10-10

- Vendored the common library and recorded its provenance and RustDesk fork point.
- Isolated runtime, service, installer, data and keyring identities while retaining the SubnetDesk display name; LAN v3 requires both endpoints to update.
- Removed legacy SHA-256 permanent-password APIs and upstream custom-client signing trust. Automatic updates remain removed.
- Made discovery opt-in and defaulted Web permissions to view-only; the Web gateway remains disabled by default.
- Added verified build downloads, scoped Rust formatting/Clippy gates and read-only upstream security monitoring.
- Fixed recursive input error formatting and uninitialized Windows capture/display outputs.
- Includes the earlier Enter-to-connect, reconnection, network/video and Windows RDP-session transition fixes.

Earlier entries below describe the original fork's historical releases, including update functionality that has since been removed.

## [1.3.0] - 2026-08-27

### Added

- Added signed in-app updates for stable Windows and macOS builds, including startup checks, optional automatic downloads, progress reporting, release notes, and guarded installation.
- Added a persistent LAN password setup prompt so a new installation clearly shows what must be configured before remote access is ready.

### Changed

- Remote-session tabs now prefer a local alias, then the remote device name, and finally the peer address.
- Explicit Web remote-access listeners now support VPN addresses outside the default private ranges when their CIDR is allowed, include those addresses in generated certificates, and provide clearer rejection diagnostics.
- Android release builds now require a fixed signing identity and verify every APK against a pinned certificate fingerprint before publication.

### Fixed

- Fixed Windows remote desktop and camera stalls caused by texture lifecycle races during session changes.
- Fixed Windows portable builds unpacking into RustDesk's data directory instead of an isolated SubnetDesk directory.
- Fixed known LAN peers being rejected before the user could confirm an unexpected device-identity fingerprint.
- Hardened macOS update extraction with unique mount paths, symlink-safe cleanup, code-signature verification, and Gatekeeper validation.

[1.3.0]: https://github.com/zibo-chen/SubnetDesk/compare/v1.2.3...v1.3.0
