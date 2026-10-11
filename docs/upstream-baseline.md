# Upstream baseline and security review

For the step-by-step import, validation and release procedure, see the
[upstream update guide (Chinese)](upstream-update-guide.md).

The fork point is RustDesk commit
[`5abf4e9724dc9a8c9b45dfe13e71af11616eb2c8`](https://github.com/rustdesk/rustdesk/commit/5abf4e9724dc9a8c9b45dfe13e71af11616eb2c8),
"Fix disabled installation bypass (#15598)", committed on 2026-07-16.
It is the parent of the first LAN-only commit
`de81051cd4723d8f2d081b43c34b931d4b2b55de`. Its identity and subject were verified
against the official RustDesk GitHub API on 2026-10-10. Later fixes imported by
the original fork are separate changes; this fork point is not the latest fully
integrated RustDesk security baseline.

Machine-readable records are in `upstream-baseline.json`. Common-library origin
and import revision are recorded in `common-library-provenance.md`.

`upstream-security.yml` runs a read-only review queue every Monday and on demand.
It collects upstream commit messages and public repository advisories and saves
an artifact. This takes effect after pushing to the default branch with Actions
enabled. The monitor does not open issues, contact users, merge code or perform
runtime updates. API failure fails the job; unavailable advisory support is
reported explicitly. Keyword/date filtering is a triage aid, so reviewers must
also examine the full upstream comparison and security advisories.

For each imported security fix, record the source commit, affected feature,
applicability to LAN-only builds and regression validation in the change's
commit/PR. Do not assume that removing public relay/rendezvous code removes risks
in shared capture, input, file-transfer or authenticated session code. Follow the
[upstream security policy](https://github.com/rustdesk/rustdesk/security/policy)
for published advisories and reporting channels.
