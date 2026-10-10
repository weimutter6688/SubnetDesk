# Fork identity and compatibility

The product and executable are **SubnetDesk**. Runtime identities use the
product name without a GitHub username, and stay separate from RustDesk:

| Resource | Identity |
| --- | --- |
| Windows executable, service, install and configuration directory | `SubnetDesk` |
| Linux executable, package, systemd service and URL scheme | `subnetdesk` |
| Rust package | `subnetdesk` |
| Native Rust library | `libsubnetdesk` |
| OS application identifier | `com.subnetdesk.app` |
| Flatpak application identifier | `com.subnetdesk.SubnetDesk` |
| Credential and identity keyring services | `com.subnetdesk.SubnetDesk.LANCredentials`, `com.subnetdesk.SubnetDesk.LANIdentities` |
| Portable extraction directory | `subnetdesk-portable` |
| Portable executable environment variable | `SUBNETDESK_APPNAME` |
| Windows app/privacy window classes | `SUBNETDESK_WIN32_WINDOW`, `SubnetDeskPrivacyWindowClass` |
| mDNS service | `_subnetdesk._tcp.local.` |
| Signed handshake domain | `subnetdesk-lan-handshake-v3\0` |

The MSI UpgradeCode is `dc0e791c-3b4d-5a51-9aa8-629d81975868`, derived
from `SubnetDesk.exe`. It retains SubnetDesk's installer identity so an existing
SubnetDesk MSI can be upgraded; it does not share RustDesk's identity. The service,
installation and configuration directories also use SubnetDesk. This release
keeps earlier SubnetDesk configuration under that name; it is not designed to
coexist with another SubnetDesk host on the same machine. Separate remote-control
services need different listening ports.

**Update both endpoints to .85 and reload browser tabs.** LAN protocol v3 rejects
previous v1/v2 clients. Version .85 removes the username from the signed handshake
prefix and discovery namespace, so .84 and .85 cannot be mixed even though both
use protocol v3. Signature verification has no fallback to the old prefix.

The keyring and OS application identifiers use the product namespace. Device
identity secrets and saved keyring credentials under earlier namespaces are not
silently imported. Verify the device fingerprint on first connection and save
login credentials again as needed. Data from .84's temporary naming is not
migrated automatically. New installations default to manual-address connections
with discovery disabled, and a disabled Web gateway with view-only permissions
when explicitly enabled.
