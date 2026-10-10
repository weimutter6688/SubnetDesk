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

Tests that manipulate real cursors, screen resolution, certificates or persistent
user PIN configuration are excluded from unattended CI. They need an isolated
interactive test machine. Security protocol/authentication/network tests remain
enabled. Existing timed performance-report tests remain explicitly opt-in.

The initial Clippy pass also uncovered a recursive input-parser error formatter
and uninitialized Windows capture/display output values. These were corrected
instead of waiving the findings. The formatter has a regression test; Windows
capture still needs normal interactive desktop verification after installation.
