# Fork identity and compatibility

The visible product name remains **SubnetDesk**. This fork uses the following
internal identities to coexist with RustDesk and the original SubnetDesk:

| Resource | Identity |
| --- | --- |
| Windows executable, service, install and configuration directory | `SubnetDesk-weimutter6688` |
| Linux executable, package, systemd service and URL scheme | `subnetdesk-weimutter6688` |
| Rust package | `subnetdesk-weimutter6688` |
| Native Rust library | `libsubnetdesk_weimutter6688` |
| OS application identifier | `io.github.weimutter6688.subnetdesk` |
| Flatpak application identifier | `io.github.weimutter6688.SubnetDesk` |
| Credential and identity keyring services | `io.github.weimutter6688.SubnetDesk.LANCredentials`, `io.github.weimutter6688.SubnetDesk.LANIdentities` |
| Portable extraction directory | `subnetdesk-weimutter6688-portable` |
| Portable executable environment variable | `SUBNETDESK_WEIMUTTER6688_APPNAME` |
| Windows app/privacy window classes | `SUBNETDESK_WEIMUTTER6688_WIN32_WINDOW`, `SubnetDeskWeimutter6688PrivacyWindowClass` |
| mDNS service | `_sd-w6688._tcp.local.` |
| Signed handshake domain | `subnetdesk-weimutter6688-lan-handshake-v3\0` |

The MSI UpgradeCode is derived from the new executable identity. It does not
upgrade or remove an existing RustDesk/original SubnetDesk MSI. Shortcuts and
installation entries use independent internal paths; the app window still shows
SubnetDesk.

If both host services run on the same machine, configure different LAN/Web
listening ports; isolated names do not let two services bind the same address
and port.

**Update both endpoints together.** LAN protocol v3 rejects previous v1/v2
clients. The embedded browser client uses the same v3 handshake. There is no
downgrade fallback or shared discovery namespace.

This is a clean configuration boundary. Existing passwords, trusted device
fingerprints, browser certificates and saved login identities are not silently
imported from another installation. Set the LAN credentials again and verify the
new device fingerprint on first connection. Keep old data if you need to refer
to it, but do not merge whole directories across identities. New installations
default to manual-address connections with discovery disabled, and a disabled
Web gateway with view-only permissions when explicitly enabled.
