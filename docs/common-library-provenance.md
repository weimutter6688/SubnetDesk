# Common library provenance

`libs/hbb_common` is maintained as source files in this repository. A clone of
SubnetDesk includes the complete common library; no separate repository or
submodule authentication is required.

The initial import preserves all 32 tracked files from
`https://github.com/zibo-chen/hbb_common.git` at commit
`64aab1c4585a9459c9849f0d0b53af5bbc40eacd`, including the LAN-only changes already
used by SubnetDesk. Subsequent changes are reviewed and committed in this
repository. The original authorship notices are retained. The common library is
part of the application distributed under the repository's AGPL-3.0 license.

The previous local Git history is preserved in an ignored backup bundle during
the import. It is not needed to build the application. Cargo's other dependencies
remain governed by their own licenses and the revisions/checksums in Cargo.lock;
vendoring this library does not vendor all third-party dependencies.
