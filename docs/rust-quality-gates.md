# Rust quality gates

`ci.yml` runs Rust build/tests, an explicit common-library test pass, Clippy, and
`scripts/check_rust_quality.py` (rustfmt check). The compiler and Clippy versions
are pinned to Rust 1.82.0, matching the verified build-input manifest.

The module list in `check_rust_quality.py` owns the strict fork gate. Each listed
module declares `#![deny(warnings, clippy::all)]`, equivalent to treating warnings
as errors within that module. CI checks the declaration and formatting, and the
actual compilation/Clippy steps enforce the lints. This includes LAN handshake,
discovery, identities, server/connect policy, diagnostics, endpoint history and
TCP connection selection. Clippy's inherited-code lint group is allowed at the
command line; the modules' explicit deny declarations override that setting.
Inherited RustDesk compiler warnings outside those modules remain visible. This
is deliberately a scoped gate, not a claim that the entire upstream codebase is
warning-free. Extend the owned list when adding a new fork module.

Tests that manipulate real cursors, screen resolution or certificates are
excluded from unattended CI. They need an isolated interactive test machine. The
unlock-PIN persistence test holds `CONFIG_STATE_TEST_LOCK` and restores the config
file through `ConfigFileRestoreGuard`, so it is safe to re-enable on an ephemeral
runner (part of the pending workflow change). Security protocol/authentication/network tests remain
enabled. Existing timed performance-report tests remain explicitly opt-in.

The initial Clippy pass also uncovered a recursive input-parser error formatter
and uninitialized Windows capture/display output values. These were corrected
instead of waiving the findings. The formatter has a regression test; Windows
capture still needs normal interactive desktop verification after installation.

## Additional gates (2026-10)

The workflow wiring for these gates is in `.github/workflows/ci.yml` and must be
pushed with a token that has the `workflow` scope; until then they can be run
locally with the same commands (`cargo audit`, `cargo deny check sources bans`,
`cargo +nightly fuzz run <target>`).

- Inherited code is now also gated on `clippy::correctness` (deny). The one
  exception, `not_unsafe_ptr_arg_deref`, is an inherited X11 FFI wrapper in
  `libs/scrap/src/x11/server.rs`. Enabling the gate found an invalid regex in
  `src/platform/gtk_sudo.rs::quote_shell_arg` that silently disabled shell
  quoting; it is fixed and covered by a unit test.
- `cargo audit` fails on any RustSec advisory that is not in the reviewed
  baseline `.cargo/audit.toml`. The baseline lists the advisories already present
  on 2026-10-14 (including rustls/rustls-webpki, openssl, quinn-proto, h2); each
  entry should be removed when its dependency is upgraded.
- `cargo deny check sources bans` (`deny.toml`) rejects any registry or git
  source that is not already reviewed.
- `fuzz/` holds cargo-fuzz targets for the LAN protobuf message decoder and the
  endpoint parser; the CI job runs each for 60 seconds on nightly.
- `src/admission.rs` and `src/lan_config.rs` joined the strict-module list.
